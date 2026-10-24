//! Construct selected graphs and reconstruct them from final HIR.
//!
//! Graph identities connect source records, values, units, and transfers without assigning
//! stack positions. [`Selector`] derives nodes and lowering definitions from registered HIR
//! rules; test fixtures can construct graphs directly. Independent source checking remains
//! responsible for semantic coverage and control-flow correspondence.
use alloc::{collections::BTreeMap, vec::Vec};

use crate::plan::*;

/// Collect selected nodes by identity before attaching their executable control tree.
/// Duplicate identities are rejected, but successful construction is not source verification.
#[cfg(test)]
pub(crate) struct GraphBuilder {
    origin: ProcedureOrigin,
    abi: ProcedureAbi,
    values: BTreeMap<ValueId, SelectedValue>,
    sources: BTreeMap<SourceStepId, SourceRecord>,
    segments: BTreeMap<SegmentId, SelectedSegment>,
    edges: BTreeMap<EdgeId, TransferEdge>,
}
#[cfg(test)]
impl GraphBuilder {
    /// Start an empty graph with the source origin and externally visible procedure ABI.
    pub fn new(origin: ProcedureOrigin, abi: ProcedureAbi) -> Self {
        Self {
            origin,
            abi,
            values: BTreeMap::new(),
            sources: BTreeMap::new(),
            segments: BTreeMap::new(),
            edges: BTreeMap::new(),
        }
    }

    /// Insert a value, rejecting reuse of its identity.
    pub fn add_value(&mut self, value: SelectedValue) -> Result<(), PlanError> {
        insert(&mut self.values, value.id, value)
    }

    /// Attach source provenance under a fresh coverage identity.
    pub fn add_source(&mut self, id: SourceStepId, source: SourceRecord) -> Result<(), PlanError> {
        insert(&mut self.sources, id, source)
    }

    /// Insert a segment while preserving the order of its lowering units.
    pub fn add_segment(&mut self, segment: SelectedSegment) -> Result<(), PlanError> {
        insert(&mut self.segments, segment.id, segment)
    }

    /// Insert a transfer; its bindings and relation to the control tree still require checking.
    pub fn add_edge(&mut self, edge: TransferEdge) -> Result<(), PlanError> {
        insert(&mut self.edges, edge.id, edge)
    }

    /// Canonicalize storage order without changing executable order or omitting any node.
    /// The independent source checker still validates all bindings and control alternatives.
    /// The first executable segment is stored first; a tree without that segment is rejected.
    pub fn finish(mut self, structure: ControlTree) -> Result<SelectedProgram, PlanError> {
        fn entry(tree: &ControlTree) -> Option<SegmentId> {
            match tree {
                ControlTree::Segment(id) | ControlTree::Loop { header: id, .. } => Some(*id),
                ControlTree::Sequence(children) => children.iter().find_map(entry),
                _ => None,
            }
        }
        let entry = entry(&structure)
            .and_then(|id| self.segments.remove(&id))
            .ok_or_else(|| invalid("selected structure lacks its entry segment"))?;
        let mut segments = Vec::with_capacity(self.segments.len() + 1);
        segments.push(entry);
        segments.extend(self.segments.into_values());
        Ok(SelectedProgram {
            origin: self.origin,
            abi: self.abi,
            values: self.values,
            source_map: self.sources,
            segments,
            edges: self.edges.into_values().collect(),
            structure,
        })
    }
}

#[cfg(test)]
fn insert<K: Ord, V>(map: &mut BTreeMap<K, V>, key: K, value: V) -> Result<(), PlanError> {
    match map.entry(key) {
        alloc::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        alloc::collections::btree_map::Entry::Occupied(_) => {
            Err(invalid("duplicate selected graph identity"))
        }
    }
}
#[cfg(test)]
fn invalid(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::InvalidSelectedLowering, message)
}

use alloc::{boxed::Box, collections::BTreeSet, vec};

use midenc_dialect_scf as scf;
use midenc_hir::{
    BlockRef, CallConv, Immediate, Op, Operation, OperationRef, RegionRef, Report, Spanned, Type,
    Value, ValueRef, dialects::builtin,
};

use super::{HirEvidence, SelectEnv, SelectionRule, TableLookup};
use crate::{
    HirSelection,
    lower::units::{HirBody, HirUnit, calls::Indirect},
    masm::Instruction as I,
    verify::{CallArgument, CallSite, DefinitionEvidence, SelectorEvidence, UnitEvidence},
};

#[derive(Clone)]
struct Destination {
    segment: SegmentId,
    parameters: Vec<ValueId>,
    kind: EdgeKind,
}
struct Flow {
    tree: Vec<ControlTree>,
    current: Option<SegmentId>,
}
struct UnitSelection {
    inputs: Vec<Atom>,
    stored: Vec<StoredInput>,
    outputs: Vec<Atom>,
    body: HirBody,
    proof: UnitEvidence,
    effects: LoweringEffects,
    scratch: Vec<ScratchRequirement>,
    scratch_effects: Vec<ScratchEffect>,
}

/// Reconstruct selected semantics and executable unit definitions from registered final HIR.
///
/// The traversal retains source coverage even for terminal branches with no continuation.
/// Values and transfers remain symbolic; neither legacy scheduling nor physical allocation
/// participates. Source-owned evidence is collected alongside, rather than read from, units.
pub(crate) struct Selector<'a, 'e> {
    env: &'a SelectEnv<'e>,
    results: Vec<Type>,
    fragment_return: Option<Destination>,
    program: SelectedProgram,
    values: BTreeMap<ValueRef, ValueId>,
    sources: midenc_hir::FxHashMap<OperationRef, SourceStepId>,
    proofs: BTreeMap<SelectedStepId, UnitEvidence>,
    definitions: Vec<HirUnit>,
    controls: u32,
    observations: Vec<crate::lower::debug::Observation>,
    inline_calls: bool,
}
impl<'a, 'e> Selector<'a, 'e> {
    /// Verify final-HIR structure and select every supported operation in the function.
    /// Reject declarations, unsupported forms, inconsistent source types or ABIs, and layouts
    /// whose lane counts, identities, or source-local frame cannot be represented by the target.
    pub fn select(
        function: &builtin::Function,
        env: &'a SelectEnv<'e>,
    ) -> Result<HirEvidence, Report> {
        Self::select_inner(function, env)
            .map_err(|error| at_operation(error, function.as_operation()))
    }

    /// Construct evidence, leaving function-wide failures to the source-context boundary.
    fn select_inner(
        function: &builtin::Function,
        env: &'a SelectEnv<'e>,
    ) -> Result<HirEvidence, Report> {
        function.as_operation().recursively_verify()?;
        if function.as_operation().region(0).is_empty() {
            return Err(super::invalid("cannot select a function declaration"));
        }
        let signature = function.signature();
        require_types(
            function.entry_block().borrow().argument_types(),
            signature.params().iter().map(|p| p.ty.clone()),
            "function entry parameters disagree with declared signature",
        )?;
        let convention = if crate::lower::units::calls::is_kernel(function) {
            CallingConvention::Kernel
        } else {
            calling_convention(&signature.cc)?
        };
        let abi = ProcedureAbi::new(
            convention,
            lanes(signature.params().iter().map(|p| p.ty.clone()))?,
            lanes(signature.results().iter().map(|p| p.ty.clone()))?,
        )?;
        let mut this = Self::new(
            ProcedureOrigin::Hir(function.as_operation_ref()),
            abi,
            signature.results().iter().map(|p| p.ty.clone()).collect(),
            crate::emitter::has_inline_call_chain(function.as_operation()),
            env,
        );
        this.cover(function.as_operation())?;
        let entry = this.segment(
            function
                .entry_block()
                .borrow()
                .arguments()
                .iter()
                .map(|a| *a as ValueRef)
                .collect(),
        )?;
        let flow = this.block(function.entry_block(), entry, None)?;
        if flow.current.is_some() {
            return Err(super::invalid("function does not end in a supported terminal"));
        }
        this.program.structure = ControlTree::Sequence(flow.tree);
        let mut offsets = vec![];
        let mut extent = 0u32;
        for local in function.locals().iter() {
            offsets.push(extent);
            let width = u32::try_from(local.size_in_felts())
                .map_err(|_| super::invalid("source local size exceeds address domain"))?;
            extent = extent
                .checked_add(width)
                .ok_or_else(|| super::invalid("source local extent overflows"))?;
        }
        if extent > MAX_FRAME_ELEMENTS {
            return Err(super::invalid("source local frame exceeds target limit"));
        }
        this.finish(SourceLocalLayout { offsets, extent })
    }

    fn new(
        origin: ProcedureOrigin,
        abi: ProcedureAbi,
        results: Vec<Type>,
        inline_calls: bool,
        env: &'a SelectEnv<'e>,
    ) -> Self {
        Self {
            env,
            results,
            fragment_return: None,
            program: SelectedProgram {
                origin,
                abi,
                values: BTreeMap::new(),
                source_map: BTreeMap::new(),
                segments: vec![],
                edges: vec![],
                structure: ControlTree::Sequence(vec![]),
            },
            values: BTreeMap::new(),
            sources: midenc_hir::FxHashMap::default(),
            proofs: BTreeMap::new(),
            definitions: vec![],
            controls: 0,
            observations: vec![],
            inline_calls,
        }
    }

    /// Complete the shared source traversal without assigning any physical locations.
    fn finish(mut self, source_locals: SourceLocalLayout) -> Result<HirEvidence, Report> {
        let mut selected_segments = BTreeSet::new();
        fn segments(tree: &ControlTree, selected: &mut BTreeSet<SegmentId>) {
            match tree {
                ControlTree::Segment(id) => {
                    selected.insert(*id);
                }
                ControlTree::Sequence(children) => {
                    for child in children {
                        segments(child, selected);
                    }
                }
                ControlTree::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    segments(then_body, selected);
                    segments(else_body, selected);
                }
                ControlTree::Loop { before, body, .. } => {
                    segments(before, selected);
                    segments(body, selected);
                }
                ControlTree::Edge(_) => {}
            }
        }
        segments(&self.program.structure, &mut selected_segments);
        if self
            .program
            .segments
            .iter()
            .any(|segment| !selected_segments.contains(&segment.id) && !segment.units.is_empty())
        {
            return Err(super::invalid("executable selected segment omitted from structure"));
        }
        // An all-terminal branch needs no continuation. Source coverage remains complete.
        self.program.segments.retain(|segment| selected_segments.contains(&segment.id));
        Ok(HirEvidence {
            source: DefinitionEvidence {
                source_locals,
                program: self.program,
                units: self.proofs,
            },
            definitions: self.definitions,
            observations: self.observations,
        })
    }

    /// Select all initializer fragments and generated stages into one value/source namespace.
    /// Actual global returns transfer to typed store continuations; allocation runs only once.
    pub fn generated(
        definition: GeneratedDefinitionId,
        stages: &[super::generated::Initialization],
        env: &'a SelectEnv<'e>,
    ) -> Result<HirEvidence, Report> {
        let abi = ProcedureAbi::new(CallingConvention::SameContext, 0, 0)?;
        let mut this = Self::new(
            ProcedureOrigin::Generated {
                definition,
                parameters: vec![],
            },
            abi,
            vec![],
            false,
            env,
        );
        let mut current = this.segment_ids(vec![])?;
        let mut tree = vec![ControlTree::Segment(current)];
        for (ordinal, stage) in stages.iter().enumerate() {
            let stage_id = u32::try_from(ordinal)
                .map_err(|_| super::invalid("generated stage identity overflow"))?;
            if let super::generated::Initialization::Global(global) = stage {
                let global = global.borrow();
                global.as_operation().recursively_verify()?;
                this.cover(global.as_operation())?;
                if global.initializer().is_empty() {
                    continue;
                }
                let ty = (*global.get_ty()).clone();
                let layout = ValueLayout::new(ty.clone())?;
                this.results = vec![ty.clone()];
                this.inline_calls = crate::emitter::has_inline_call_chain(global.as_operation());
                let continuation = this.segment_ids(vec![])?;
                let value = ValueId(index(this.program.values.len())?);
                this.program.values.insert(
                    value,
                    SelectedValue {
                        id: value,
                        origin: ValueOrigin::Parameter {
                            segment: continuation,
                            index: 0,
                        },
                        layout,
                        reconstruction: None,
                    },
                );
                this.program.segments[continuation.0 as usize].parameters.push(value);
                this.fragment_return = Some(Destination {
                    segment: continuation,
                    parameters: vec![value],
                    kind: EdgeKind::Forward,
                });
                let flow =
                    this.block(global.initializer().entry().as_block_ref(), current, None)?;
                if flow.current.is_some() {
                    return Err(super::invalid("global initializer lacks a terminal result"));
                }
                tree.extend(flow.tree.into_iter().skip(1));
                tree.push(ControlTree::Segment(continuation));
                current = continuation;
                this.fragment_return = None;
                let address = env
                    .link_info
                    .and_then(|link| {
                        link.globals_layout().get_computed_addr(global.as_global_var_ref())
                    })
                    .ok_or_else(|| {
                        super::invalid("global initializer requires its linked storage address")
                    })?;
                let source = this.generated_source(definition, stage_id)?;
                let inputs = this.atoms(&[value]);
                let mut memory = crate::lower::units::memory::Memory::store_immediate(
                    global.as_operation().context_rc(),
                    ty,
                    address,
                    global.span(),
                )?;
                memory.bind(&inputs, &[]);
                let proof = memory.proof.clone();
                let effects = memory.effects.clone();
                this.add_unit(
                    current,
                    source,
                    UnitSelection {
                        inputs,
                        stored: vec![],
                        outputs: vec![],
                        body: HirBody::Memory(Box::new(memory)),
                        proof,
                        effects,
                        scratch: vec![],
                        scratch_effects: vec![],
                    },
                )?;
            } else {
                let source = this.generated_source(definition, stage_id)?;
                let construction = stage.construct(env)?;
                this.add_unit(
                    current,
                    source,
                    UnitSelection {
                        inputs: vec![],
                        stored: vec![],
                        outputs: vec![],
                        body: HirBody::Plain(construction.instructions),
                        proof: construction.proof,
                        effects: construction.effects,
                        scratch: vec![],
                        scratch_effects: vec![],
                    },
                )?;
            }
        }
        tree.push(ControlTree::Edge(this.edge(current, None, EdgeKind::Return, &[])?));
        this.program.structure = ControlTree::Sequence(tree);
        this.finish(SourceLocalLayout::default())
    }

    fn generated_source(
        &mut self,
        definition: GeneratedDefinitionId,
        stage: u32,
    ) -> Result<SourceStepId, Report> {
        let source = SourceStepId(index(self.program.source_map.len())?);
        self.program.source_map.insert(
            source,
            SourceRecord {
                origin: SourceOrigin::Generated { definition, stage },
                location: SourceLocation::default(),
            },
        );
        Ok(source)
    }

    fn cover(&mut self, operation: &Operation) -> Result<(), Report> {
        let source = SourceStepId(index(self.program.source_map.len())?);
        if self.sources.insert(operation.as_operation_ref(), source).is_some() {
            return Err(at_operation(
                super::invalid("HIR operation occurs more than once"),
                operation,
            ));
        }
        self.program.source_map.insert(source, record(operation));
        for region in operation.regions() {
            // Each owned region has its own deterministic coverage identity, even when omitted.
            let id = SourceStepId(index(self.program.source_map.len())?);
            self.program.source_map.insert(id, record(operation));
            for block in region.body() {
                for op in block.body() {
                    self.cover(&op)?;
                }
            }
        }
        Ok(())
    }

    fn value(&mut self, value: ValueRef) -> Result<ValueId, Report> {
        if let Some(id) = self.values.get(&value) {
            return Ok(*id);
        }
        let id = ValueId(index(self.program.values.len())?);
        self.program.values.insert(
            id,
            SelectedValue {
                id,
                origin: ValueOrigin::Hir(value),
                layout: ValueLayout::new(value.borrow().ty().clone())?,
                reconstruction: None,
            },
        );
        self.values.insert(value, id);
        Ok(id)
    }

    fn ids(&mut self, values: impl IntoIterator<Item = ValueRef>) -> Result<Vec<ValueId>, Report> {
        values.into_iter().map(|v| self.value(v)).collect()
    }

    fn atoms(&self, values: &[ValueId]) -> Vec<Atom> {
        values
            .iter()
            .flat_map(|v| {
                (0..self.program.values[v].layout.lanes()).map(|lane| Atom::new(*v, lane))
            })
            .collect()
    }

    fn temporary(&mut self, source: SourceStepId, ty: Type) -> Result<ValueId, Report> {
        let id = ValueId(index(self.program.values.len())?);
        self.program.values.insert(
            id,
            SelectedValue {
                id,
                origin: ValueOrigin::Temporary {
                    step: source,
                    index: id.0,
                },
                layout: ValueLayout::new(ty)?,
                reconstruction: None,
            },
        );
        Ok(id)
    }

    fn segment(&mut self, parameters: Vec<ValueRef>) -> Result<SegmentId, Report> {
        let params = self.ids(parameters)?;
        self.segment_ids(params)
    }

    fn segment_ids(&mut self, parameters: Vec<ValueId>) -> Result<SegmentId, Report> {
        let id = SegmentId(index(self.program.segments.len())?);
        self.program.segments.push(SelectedSegment {
            id,
            parameters,
            content_parameters: vec![],
            units: vec![],
        });
        Ok(id)
    }

    fn control(&mut self) -> Result<ControlId, Report> {
        let id = ControlId(self.controls);
        self.controls = self
            .controls
            .checked_add(1)
            .ok_or_else(|| super::invalid("control identity overflow"))?;
        Ok(id)
    }

    fn edge(
        &mut self,
        source: SegmentId,
        target: Option<&Destination>,
        kind: EdgeKind,
        returned: &[ValueId],
    ) -> Result<EdgeId, Report> {
        let destination = target.map(|d| d.segment);
        let params = target.map_or(returned, |d| d.parameters.as_slice());
        if params.len() != returned.len() {
            return Err(super::invalid("source/destination arity mismatch"));
        }
        for (p, v) in params.iter().zip(returned) {
            if self.program.values[p].layout != self.program.values[v].layout {
                return Err(super::invalid("source/destination layout mismatch"));
            }
        }
        let id = EdgeId(index(self.program.edges.len())?);
        self.program.edges.push(TransferEdge {
            id,
            source,
            destination,
            kind,
            bindings: params
                .iter()
                .zip(returned)
                .map(|(p, v)| ParameterBinding {
                    destination: *p,
                    source: *v,
                })
                .collect(),
            content_bindings: vec![],
        });
        Ok(id)
    }

    fn plain(
        &mut self,
        segment: SegmentId,
        source: SourceStepId,
        inputs: Vec<Atom>,
        outputs: Vec<Atom>,
        body: Vec<I>,
        aliases: LaneAliases,
    ) -> Result<SelectedStepId, Report> {
        let id = SelectedStepId(index(self.definitions.len())?);
        let proof = UnitEvidence {
            aliases,
            ..Default::default()
        };
        self.add_unit(
            segment,
            source,
            UnitSelection {
                inputs,
                stored: vec![],
                outputs,
                body: HirBody::Plain(body),
                proof,
                effects: LoweringEffects::default(),
                scratch: vec![],
                scratch_effects: vec![],
            },
        )?;
        Ok(id)
    }

    fn add_unit(
        &mut self,
        segment: SegmentId,
        source: SourceStepId,
        selection: UnitSelection,
    ) -> Result<SelectedStepId, Report> {
        let UnitSelection {
            inputs,
            stored,
            outputs,
            body,
            proof,
            effects,
            scratch,
            scratch_effects,
        } = selection;
        let id = SelectedStepId(index(self.definitions.len())?);
        let kind = LoweringKind(
            2000u32
                .checked_add(id.0)
                .ok_or_else(|| super::invalid("lowering kind overflow"))?,
        );
        let semantic_inputs = inputs
            .iter()
            .map(|a| a.value)
            .chain(stored.iter().filter_map(|s| match s.value {
                StoredValue::Atom(a) => Some(a.value),
                _ => None,
            }))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.program.segments[segment.0 as usize].units.push(SelectedLowering {
            id,
            source,
            kind,
            semantic_inputs,
            stack_inputs: inputs,
            stored_inputs: stored,
            outputs,
            publication: OutputPublication::Stack,
            address_results: vec![],
            effects,
            scratch,
            scratch_effects,
        });
        self.definitions.push(HirUnit { kind, body });
        self.proofs.insert(id, proof);
        Ok(id)
    }

    fn constant(
        &mut self,
        segment: SegmentId,
        source: SourceStepId,
        output: ValueId,
        immediate: Immediate,
    ) -> Result<(), Report> {
        if self.program.values[&output].layout.ty() != &immediate.ty() {
            return Err(super::invalid("literal result type disagrees with immediate type"));
        }
        let raw = literal(immediate)?;
        if raw.len() != self.program.values[&output].layout.lanes() as usize {
            return Err(super::invalid("literal layout mismatch"));
        }
        self.program.values.get_mut(&output).unwrap().reconstruction =
            Some(Reconstruction::Literal(raw.clone()));
        let body = raw
            .iter()
            .rev()
            .map(|v| Primitive::Push(*v).instruction())
            .collect::<Result<Vec<_>, _>>()?;
        self.plain(segment, source, vec![], self.atoms(&[output]), body, LaneAliases::default())?;
        Ok(())
    }

    /// Finish context returns in a separate segment so its incoming repair first removes
    /// all dead owned occurrences. Truncation then affects only VM floor padding, preserves
    /// the declared result prefix, and cannot invalidate an allocator-visible suffix.
    fn returned(
        &mut self,
        current: SegmentId,
        source: SourceStepId,
        values: &[ValueId],
        flow: &mut Flow,
    ) -> Result<(), Report> {
        if let Some(destination) = self.fragment_return.clone() {
            flow.tree.push(ControlTree::Edge(self.edge(
                current,
                Some(&destination),
                destination.kind,
                values,
            )?));
            return Ok(());
        }
        let (segment, returned) = if matches!(
            self.program.abi.convention(),
            CallingConvention::Context | CallingConvention::Kernel
        ) {
            let segment = self.segment_ids(vec![])?;
            let destination = Destination {
                segment,
                parameters: vec![],
                kind: EdgeKind::Forward,
            };
            flow.tree.push(ControlTree::Edge(self.edge(
                current,
                Some(&destination),
                destination.kind,
                &[],
            )?));
            flow.tree.push(ControlTree::Segment(segment));
            let mut outputs = vec![];
            for value in values {
                outputs
                    .push(self.temporary(source, self.program.values[value].layout.ty().clone())?);
            }
            let inputs = self.atoms(values);
            let output_atoms = self.atoms(&outputs);
            let target = crate::lower::raw::target("::miden::core::sys::truncate_stack");
            self.add_unit(
                segment,
                source,
                UnitSelection {
                    inputs: inputs.clone(),
                    stored: vec![],
                    outputs: output_atoms.clone(),
                    body: HirBody::Plain(vec![I::Exec(target.clone())]),
                    proof: UnitEvidence {
                        aliases: LaneAliases::new(output_atoms.into_iter().zip(inputs).collect()),
                        calls: vec![CallSite {
                            instruction: I::Exec(target),
                            abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0)?,
                            inputs: vec![],
                            path: vec![],
                            truncate: true,
                            outputs: vec![],
                            publications: vec![],
                            selector: None,
                        }],
                        ..Default::default()
                    },
                    effects: LoweringEffects::default(),
                    scratch: vec![],
                    scratch_effects: vec![],
                },
            )?;
            (segment, outputs)
        } else {
            (current, values.to_vec())
        };
        flow.tree
            .push(ControlTree::Edge(self.edge(segment, None, EdgeKind::Return, &returned)?));
        Ok(())
    }

    fn block(
        &mut self,
        block: BlockRef,
        start: SegmentId,
        yielded: Option<Destination>,
    ) -> Result<Flow, Report> {
        let mut operation = None;
        self.block_inner(block, start, yielded, &mut operation)
            .map_err(|error| match operation {
                Some(operation) => at_operation(error, &operation.borrow()),
                None => error,
            })
    }

    /// Retain the current source operation for failures from this block's dispatch.
    /// Recursive blocks use `block` so their precise source context survives unwinding.
    fn block_inner(
        &mut self,
        block: BlockRef,
        start: SegmentId,
        yielded: Option<Destination>,
        operation_context: &mut Option<OperationRef>,
    ) -> Result<Flow, Report> {
        let ops = block.borrow().body().iter().map(|o| o.as_operation_ref()).collect::<Vec<_>>();
        let mut flow = Flow {
            tree: vec![ControlTree::Segment(start)],
            current: Some(start),
        };
        for operation in ops {
            let Some(current) = flow.current else {
                break;
            };
            *operation_context = Some(operation);
            let op = operation.borrow();
            let source = self.sources[&operation];
            if self.inline_calls {
                self.observations.extend(crate::lower::debug::Observation::inline_chain(
                    &op,
                    current,
                    self.program.segments[current.0 as usize].units.len(),
                ));
            }
            let lowering = op.as_trait::<dyn HirSelection>().ok_or_else(|| {
                super::invalid(alloc::format!("unsupported final HIR operation: {}", op.name()))
            })?;
            match lowering.selection_rule() {
                SelectionRule::Return => {
                    require_types(
                        operand_types(&op),
                        self.results.iter().cloned(),
                        "return tuple disagrees with declared function signature",
                    )?;
                    let values =
                        self.ids(op.operands().all().iter().map(|v| v.borrow().as_value_ref()))?;
                    self.returned(current, source, &values, &mut flow)?;
                    flow.current = None;
                }
                SelectionRule::ReturnImmediate(immediate) => {
                    require_types(
                        [immediate.ty()],
                        self.results.iter().cloned(),
                        "immediate return disagrees with declared function signature",
                    )?;
                    let output = self.temporary(source, immediate.ty())?;
                    self.constant(current, source, output, immediate)?;
                    self.returned(current, source, &[output], &mut flow)?;
                    flow.current = None;
                }
                SelectionRule::Yield => {
                    let target = yielded
                        .as_ref()
                        .ok_or_else(|| super::invalid("yield outside supported region"))?;
                    let values =
                        self.ids(op.operands().all().iter().map(|v| v.borrow().as_value_ref()))?;
                    flow.tree.push(ControlTree::Edge(self.edge(
                        current,
                        Some(target),
                        target.kind,
                        &values,
                    )?));
                    flow.current = None;
                }
                SelectionRule::Condition => {
                    if yielded.is_some()
                        || op.parent_op().is_none_or(|parent| !parent.borrow().is::<scf::While>())
                    {
                        return Err(super::invalid("condition outside while before region"));
                    }
                    return Ok(flow);
                }
                SelectionRule::Trap => {
                    let trap = vec![
                        Primitive::Push(miden_core::Felt::ZERO).instruction()?,
                        I::AssertWithError(
                            alloc::sync::Arc::<str>::from("entered unreachable code").into(),
                        ),
                    ];
                    let id =
                        self.plain(current, source, vec![], vec![], trap, LaneAliases::default())?;
                    let unit = self.program.segments[current.0 as usize]
                        .units
                        .iter_mut()
                        .find(|u| u.id == id)
                        .unwrap();
                    unit.effects.may_trap = true;
                    unit.effects.normal_return = false;
                    flow.tree.push(ControlTree::Edge(self.edge(
                        current,
                        None,
                        EdgeKind::Trap,
                        &[],
                    )?));
                    flow.current = None;
                }
                SelectionRule::If => self.select_if(&op, current, &mut flow)?,
                SelectionRule::Switch => self.select_switch(&op, current, &mut flow)?,
                SelectionRule::While => self.select_while(&op, current, &mut flow)?,
                SelectionRule::TerminalBranch => self.terminal_branch(&op, current, &mut flow)?,
                SelectionRule::Dyncall | SelectionRule::ExecIndirect => {
                    self.indirect(&op, current, &mut flow)?
                }
                SelectionRule::DirectCall => self.direct(&op, current)?,
                SelectionRule::Fpi => self.fpi(&op, current)?,
                SelectionRule::ProcedureRoot => {
                    let reference = op.downcast_ref::<midenc_dialect_hir::ProcedureRoot>().unwrap();
                    let callee = crate::legalization::validate_procedure_root(reference)?;
                    let module = if op.nearest_parent_op::<builtin::GlobalVariable>().is_some() {
                        None
                    } else {
                        op.nearest_symbol_table()
                    };
                    let target =
                        crate::lower::invocation_target_from_symbol(callee, module, op.span());
                    let target = if let Some(owners) = self.env.owners {
                        owners.target(target)?
                    } else {
                        target
                    };
                    let outputs = self.results(&op)?;
                    if outputs.len() != 4
                        || outputs
                            .iter()
                            .any(|value| self.program.values[value].layout.ty() != &Type::Felt)
                    {
                        return Err(super::invalid(
                            "procedure root requires exactly four felt results",
                        ));
                    }
                    self.plain(
                        current,
                        source,
                        vec![],
                        self.atoms(&outputs),
                        vec![I::ProcRef(target)],
                        LaneAliases::default(),
                    )?;
                }
                SelectionRule::Debug => {
                    let lanes = if let Some(debug) =
                        op.downcast_ref::<midenc_hir::dialects::debuginfo::DebugValue>()
                    {
                        let value = self.value(debug.value().as_value_ref())?;
                        self.atoms(&[value])
                    } else {
                        vec![]
                    };
                    self.observations.push(crate::lower::debug::Observation::from_source(
                        &op,
                        current,
                        self.program.segments[current.0 as usize].units.len(),
                        lanes,
                        self.env,
                    ));
                }
                rule => {
                    let inputs =
                        self.ids(op.operands().all().iter().map(|v| v.borrow().as_value_ref()))?;
                    let outputs =
                        self.ids(op.results().all().iter().map(|v| v.borrow().as_value_ref()))?;
                    match rule {
                        SelectionRule::Intrinsic(kind)
                            if kind != crate::lower::units::intrinsics::Kind::UnpackWord =>
                        {
                            let definition =
                                crate::lower::units::intrinsics::Intrinsic::from_source(kind, &op)?;
                            let effects = definition.effects.clone();
                            let proof = definition
                                .bind_evidence(&self.atoms(&inputs), &self.atoms(&outputs));
                            self.add_unit(
                                current,
                                source,
                                UnitSelection {
                                    inputs: self.atoms(&inputs),
                                    stored: vec![],
                                    outputs: self.atoms(&outputs),
                                    body: HirBody::Intrinsic(alloc::boxed::Box::new(definition)),
                                    proof,
                                    effects,
                                    scratch: vec![],
                                    scratch_effects: vec![],
                                },
                            )?;
                        }
                        SelectionRule::Memory(kind) => {
                            let mut definition =
                                crate::lower::units::memory::Memory::from_source(kind, &op)?;
                            let input_atoms = self.atoms(&inputs);
                            let output_atoms = self.atoms(&outputs);
                            definition.bind(&input_atoms, &output_atoms);
                            let proof = core::mem::take(&mut definition.proof);
                            let effects = definition.effects.clone();
                            self.add_unit(
                                current,
                                source,
                                UnitSelection {
                                    inputs: input_atoms,
                                    stored: vec![],
                                    outputs: output_atoms,
                                    body: HirBody::Memory(alloc::boxed::Box::new(definition)),
                                    proof,
                                    effects,
                                    scratch: vec![],
                                    scratch_effects: vec![],
                                },
                            )?;
                        }
                        SelectionRule::Arithmetic(kind) => {
                            let mut definition =
                                crate::lower::units::arithmetic::Arithmetic::from_source(
                                    kind, &op, self.env,
                                )?;
                            let input_atoms = definition
                                .input_order
                                .iter()
                                .flat_map(|i| self.atoms(&[inputs[*i]]))
                                .collect::<Vec<_>>();
                            let output_atoms = self.atoms(&outputs);
                            if let Some(immediate) = definition.literal {
                                self.program.values.get_mut(&outputs[0]).unwrap().reconstruction =
                                    Some(Reconstruction::Literal(literal(immediate)?));
                            }
                            definition.bind(&input_atoms, &output_atoms);
                            let proof = core::mem::take(&mut definition.proof);
                            let effects = definition.effects.clone();
                            self.add_unit(
                                current,
                                source,
                                UnitSelection {
                                    inputs: input_atoms,
                                    stored: vec![],
                                    outputs: output_atoms,
                                    body: HirBody::Arithmetic(alloc::boxed::Box::new(definition)),
                                    proof,
                                    effects,
                                    scratch: vec![],
                                    scratch_effects: vec![],
                                },
                            )?;
                        }
                        SelectionRule::Constant(value) if outputs.len() == 1 => {
                            self.constant(current, source, outputs[0], value)?
                        }
                        SelectionRule::Alias
                        | SelectionRule::Split
                        | SelectionRule::Join
                        | SelectionRule::Intrinsic(
                            crate::lower::units::intrinsics::Kind::UnpackWord,
                        ) => {
                            validate_alias(&op, rule)?;
                            let mut input_atoms = self.atoms(&inputs);
                            let mut output_atoms = self.atoms(&outputs);
                            if matches!(rule, SelectionRule::Join) {
                                input_atoms =
                                    inputs.iter().rev().flat_map(|v| self.atoms(&[*v])).collect();
                            }
                            if matches!(rule, SelectionRule::Split) {
                                output_atoms =
                                    outputs.iter().rev().flat_map(|v| self.atoms(&[*v])).collect();
                            }
                            if input_atoms.len() != output_atoms.len() || input_atoms.is_empty() {
                                return Err(super::invalid(
                                    "alias changes raw representation width",
                                ));
                            }
                            let aliases = LaneAliases::new(
                                output_atoms
                                    .iter()
                                    .copied()
                                    .zip(input_atoms.iter().copied())
                                    .collect(),
                            );
                            for output in outputs {
                                let map = self
                                    .atoms(&[output])
                                    .iter()
                                    .map(|a| {
                                        aliases.pairs().iter().find(|(o, _)| o == a).unwrap().1
                                    })
                                    .collect();
                                self.program.values.get_mut(&output).unwrap().reconstruction =
                                    Some(Reconstruction::Alias(map));
                            }
                            let id = self.plain(
                                current,
                                source,
                                input_atoms,
                                output_atoms,
                                vec![],
                                aliases,
                            )?;
                            // Keep semantic dependencies and the empty source-owned unit, but
                            // do not force representation aliases into a physical result prefix.
                            let unit = self.program.segments[current.0 as usize]
                                .units
                                .last_mut()
                                .expect("plain appended the alias unit");
                            debug_assert_eq!(unit.id, id);
                            unit.publication = OutputPublication::Aliases;
                            unit.stack_inputs.clear();
                        }
                        _ => return Err(super::invalid("invalid registered source shape")),
                    }
                }
            }
        }
        Ok(flow)
    }

    fn region_entry(&self, region: RegionRef) -> Result<BlockRef, Report> {
        let region = region.borrow();
        if region.body().len() != 1 {
            return Err(super::invalid("supported structured region requires one block"));
        }
        region
            .entry_block_ref()
            .ok_or_else(|| super::invalid("missing structured region entry"))
    }

    fn results(&mut self, op: &Operation) -> Result<Vec<ValueId>, Report> {
        self.ids(op.results().all().iter().map(|r| r.borrow().as_value_ref()))
    }

    fn continuation(&mut self, op: &Operation) -> Result<Destination, Report> {
        let parameters = self.results(op)?;
        Ok(Destination {
            segment: self.segment_ids(parameters.clone())?,
            parameters,
            kind: EdgeKind::Forward,
        })
    }

    fn select_if(
        &mut self,
        op: &Operation,
        current: SegmentId,
        flow: &mut Flow,
    ) -> Result<(), Report> {
        let branch = op.downcast_ref::<scf::If>().unwrap();
        let condition = self.value(branch.condition().as_value_ref())?;
        let continuation = self.continuation(op)?;
        let mut arms = vec![];
        for region in [branch.then_body().as_region_ref(), branch.else_body().as_region_ref()] {
            if region.borrow().is_empty() {
                if !continuation.parameters.is_empty() {
                    return Err(super::invalid("missing else requires no results"));
                }
                arms.push((continuation.segment, vec![]));
            } else {
                let entry = self.region_entry(region)?;
                if entry.borrow().num_arguments() != 0 {
                    return Err(super::invalid("if regions cannot bind parameters"));
                }
                let segment = self.segment(vec![])?;
                arms.push((segment, self.block(entry, segment, Some(continuation.clone()))?.tree));
            }
        }
        let on_true = self.edge(
            current,
            Some(&Destination {
                segment: arms[0].0,
                parameters: vec![],
                kind: EdgeKind::True,
            }),
            EdgeKind::True,
            &[],
        )?;
        let on_false = self.edge(
            current,
            Some(&Destination {
                segment: arms[1].0,
                parameters: vec![],
                kind: EdgeKind::False,
            }),
            EdgeKind::False,
            &[],
        )?;
        flow.tree.push(ControlTree::If {
            id: self.control()?,
            predicate: Atom::new(condition, 0),
            on_true,
            then_body: Box::new(ControlTree::Sequence(arms.remove(0).1)),
            on_false,
            else_body: Box::new(ControlTree::Sequence(arms.remove(0).1)),
        });
        // Keep reserved continuation IDs stable. An unreachable empty continuation is omitted at finish.
        if self.program.edges.iter().any(|e| e.destination == Some(continuation.segment)) {
            flow.tree.push(ControlTree::Segment(continuation.segment));
            flow.current = Some(continuation.segment);
        } else {
            flow.current = None;
        }
        Ok(())
    }

    fn select_while(
        &mut self,
        op: &Operation,
        current: SegmentId,
        flow: &mut Flow,
    ) -> Result<(), Report> {
        let loop_op = op.downcast_ref::<scf::While>().unwrap();
        let before_block = self.region_entry(loop_op.before().as_region_ref())?;
        let after_block = self.region_entry(loop_op.after().as_region_ref())?;
        let header = self
            .segment(before_block.borrow().arguments().iter().map(|a| *a as ValueRef).collect())?;
        let init = self.ids(loop_op.inits().iter().map(|a| a.borrow().as_value_ref()))?;
        let header_dest = Destination {
            segment: header,
            parameters: self.program.segments[header.0 as usize].parameters.clone(),
            kind: EdgeKind::Backedge,
        };
        let enter = self.edge(current, Some(&header_dest), EdgeKind::Forward, &init)?;
        flow.tree.push(ControlTree::Edge(enter));
        let before = self.block(before_block, header, None)?;
        let Some(condition_site) = before.current else {
            flow.tree.extend(before.tree);
            flow.current = None;
            return Ok(());
        };
        let condition = before_block
            .borrow()
            .terminator()
            .ok_or_else(|| super::invalid("while lacks condition"))?;
        let condition = condition.borrow();
        let condition = condition
            .downcast_ref::<scf::Condition>()
            .ok_or_else(|| super::invalid("while before does not end with condition"))?;
        let predicate = self.value(condition.condition().as_value_ref())?;
        let forwarded =
            self.ids(condition.forwarded().iter().map(|a| a.borrow().as_value_ref()))?;
        let continuation = self.continuation(op)?;
        let after = self
            .segment(after_block.borrow().arguments().iter().map(|a| *a as ValueRef).collect())?;
        let after_dest = Destination {
            segment: after,
            parameters: self.program.segments[after.0 as usize].parameters.clone(),
            kind: EdgeKind::True,
        };
        let on_true = self.edge(condition_site, Some(&after_dest), EdgeKind::True, &forwarded)?;
        let on_false =
            self.edge(condition_site, Some(&continuation), EdgeKind::False, &forwarded)?;
        let body = self.block(after_block, after, Some(header_dest))?;
        let backedges = self
            .program
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Backedge && e.destination == Some(header))
            .map(|e| e.id)
            .collect();
        flow.tree.push(ControlTree::Loop {
            id: self.control()?,
            condition: self.control()?,
            header,
            before: Box::new(ControlTree::Sequence(before.tree)),
            predicate: Atom::new(predicate, 0),
            on_true,
            on_false,
            body: Box::new(ControlTree::Sequence(body.tree)),
            backedges,
        });
        flow.tree.push(ControlTree::Segment(continuation.segment));
        flow.current = Some(continuation.segment);
        Ok(())
    }

    fn compare(
        &mut self,
        segment: SegmentId,
        source: SourceStepId,
        selector: ValueId,
        instructions: Vec<I>,
    ) -> Result<Atom, Report> {
        let result = self.temporary(source, Type::I1)?;
        // Range dispatch uses checked U32 comparisons; equality-only dispatch does not trap.
        // Keep their source permission consistent with the existing physical instruction rules.
        let mut effects = LoweringEffects::default();
        for instruction in &instructions {
            effects.may_trap |= TargetEffects::instruction(instruction)?.may_trap;
        }
        self.add_unit(
            segment,
            source,
            UnitSelection {
                inputs: vec![Atom::new(selector, 0)],
                stored: vec![],
                outputs: vec![Atom::new(result, 0)],
                body: HirBody::Plain(instructions),
                proof: UnitEvidence::default(),
                effects,
                scratch: vec![],
                scratch_effects: vec![],
            },
        )?;
        Ok(Atom::new(result, 0))
    }

    fn switch_entry(&self, region: RegionRef) -> Result<BlockRef, Report> {
        let block = self.region_entry(region)?;
        if block.borrow().num_arguments() != 0 {
            return Err(super::invalid("switch regions cannot bind parameters"));
        }
        Ok(block)
    }

    fn switch_arm(
        &mut self,
        region: RegionRef,
        destination: &Destination,
    ) -> Result<(SegmentId, ControlTree), Report> {
        let block = self.switch_entry(region)?;
        let segment = self.segment(vec![])?;
        let flow = self.block(block, segment, Some(destination.clone()))?;
        Ok((segment, ControlTree::Sequence(flow.tree)))
    }

    fn split(
        &mut self,
        segment: SegmentId,
        predicate: Atom,
        then_entry: SegmentId,
        then_tree: ControlTree,
        else_entry: SegmentId,
        else_tree: ControlTree,
    ) -> Result<ControlTree, Report> {
        let a = self.edge(
            segment,
            Some(&Destination {
                segment: then_entry,
                parameters: vec![],
                kind: EdgeKind::True,
            }),
            EdgeKind::True,
            &[],
        )?;
        let b = self.edge(
            segment,
            Some(&Destination {
                segment: else_entry,
                parameters: vec![],
                kind: EdgeKind::False,
            }),
            EdgeKind::False,
            &[],
        )?;
        Ok(ControlTree::If {
            id: self.control()?,
            predicate,
            on_true: a,
            then_body: Box::new(then_tree),
            on_false: b,
            else_body: Box::new(else_tree),
        })
    }

    fn linear_switch(
        &mut self,
        cases: &[(u32, RegionRef)],
        default: RegionRef,
        selector: ValueId,
        source: SourceStepId,
        segment: SegmentId,
        destination: &Destination,
    ) -> Result<ControlTree, Report> {
        let Some(((case, region), rest)) = cases.split_first() else {
            let block = self.switch_entry(default)?;
            return Ok(ControlTree::Sequence(
                self.block(block, segment, Some(destination.clone()))?.tree,
            ));
        };
        let predicate = self.compare(
            segment,
            source,
            selector,
            vec![I::EqImm(miden_core::Felt::new_unchecked(*case as u64).into())],
        )?;
        let (a, taken) = self.switch_arm(*region, destination)?;
        let b = self.segment(vec![])?;
        let next = self.linear_switch(rest, default, selector, source, b, destination)?;
        let branch = self.split(segment, predicate, a, taken, b, next)?;
        Ok(ControlTree::Sequence(vec![ControlTree::Segment(segment), branch]))
    }

    fn binary_switch(
        &mut self,
        cases: &[(u32, RegionRef)],
        selector: ValueId,
        source: SourceStepId,
        segment: SegmentId,
        destination: &Destination,
    ) -> Result<ControlTree, Report> {
        if cases.len() == 1 {
            let block = self.switch_entry(cases[0].1)?;
            return Ok(ControlTree::Sequence(
                self.block(block, segment, Some(destination.clone()))?.tree,
            ));
        }
        let mid = cases.len() / 2;
        let predicate = self.compare(
            segment,
            source,
            selector,
            vec![
                Primitive::Push(miden_core::Felt::new_unchecked(cases[mid].0 as u64))
                    .instruction()?,
                I::U32Lt,
            ],
        )?;
        let a = self.segment(vec![])?;
        let b = self.segment(vec![])?;
        let left = self.binary_switch(&cases[..mid], selector, source, a, destination)?;
        let right = self.binary_switch(&cases[mid..], selector, source, b, destination)?;
        let branch = self.split(segment, predicate, a, left, b, right)?;
        Ok(ControlTree::Sequence(vec![ControlTree::Segment(segment), branch]))
    }

    fn select_switch(
        &mut self,
        op: &Operation,
        current: SegmentId,
        flow: &mut Flow,
    ) -> Result<(), Report> {
        let switch = op.downcast_ref::<scf::IndexSwitch>().unwrap();
        let selector = self.value(switch.selector().as_value_ref())?;
        let source = self.sources[&op.as_operation_ref()];
        let destination = self.continuation(op)?;
        if op.regions().len() != switch.num_cases() + 1 {
            return Err(super::invalid("missing switch case regions"));
        }
        let mut cases = switch
            .cases()
            .iter()
            .enumerate()
            .map(|(index, value)| (*value, switch.get_case_region(index)))
            .collect::<Vec<_>>();
        cases.sort_by_key(|(value, _)| *value);
        if cases.windows(2).any(|pair| pair[0].0 == pair[1].0)
            || op.regions().len() != cases.len() + 1
        {
            return Err(super::invalid("duplicate or missing switch case regions"));
        }
        let default = switch.default_region().as_region_ref();
        let contiguous = cases.windows(2).all(|pair| pair[0].0.checked_add(1) == Some(pair[1].0));
        if cases.len() < 3 || !contiguous {
            let mut tree =
                self.linear_switch(&cases, default, selector, source, current, &destination)?;
            // The caller already owns the dispatch segment.
            if let ControlTree::Sequence(children) = &mut tree {
                children.remove(0);
            }
            flow.tree.push(tree);
        } else {
            let low = cases[0].0;
            let high = cases.last().unwrap().0;
            let predicate = self.compare(
                current,
                source,
                selector,
                vec![
                    I::Dup0,
                    Primitive::Push(miden_core::Felt::new_unchecked(low as u64)).instruction()?,
                    I::U32Lt,
                    I::EqImm(miden_core::Felt::ZERO.into()),
                    I::Swap1,
                    Primitive::Push(miden_core::Felt::new_unchecked(high as u64)).instruction()?,
                    I::U32Gt,
                    I::EqImm(miden_core::Felt::ZERO.into()),
                    I::And,
                ],
            )?;
            let a = self.segment(vec![])?;
            let b = self.segment(vec![])?;
            let tree = self.binary_switch(&cases, selector, source, a, &destination)?;
            let fallback = ControlTree::Sequence(
                self.block(self.switch_entry(default)?, b, Some(destination.clone()))?.tree,
            );
            let branch = self.split(current, predicate, a, tree, b, fallback)?;
            flow.tree.push(branch);
        }
        if self.program.edges.iter().any(|e| e.destination == Some(destination.segment)) {
            flow.tree.push(ControlTree::Segment(destination.segment));
            flow.current = Some(destination.segment);
        } else {
            flow.current = None;
        }
        Ok(())
    }

    fn terminal_branch(
        &mut self,
        op: &Operation,
        current: SegmentId,
        flow: &mut Flow,
    ) -> Result<(), Report> {
        let branch = op.downcast_ref::<midenc_dialect_cf::CondBr>().unwrap();
        let condition = self.value(branch.condition().as_value_ref())?;
        let shared = branch.then_dest().successor() == branch.else_dest().successor();
        let mut shared_destination = None;
        let mut arms = vec![];
        for operand in [branch.then_dest(), branch.else_dest()] {
            let block = operand.successor();
            if block.borrow().num_successors() != 0 {
                return Err(super::invalid(
                    "residual cf.cond_br only supports terminal exit blocks",
                ));
            }
            let segment = match shared_destination {
                Some(segment) => segment,
                None => self
                    .segment(block.borrow().arguments().iter().map(|a| *a as ValueRef).collect())?,
            };
            if shared {
                shared_destination = Some(segment);
            }
            let parameters = self.program.segments[segment.0 as usize].parameters.clone();
            let values = self.ids(operand.arguments.iter().map(|v| v.borrow().as_value_ref()))?;
            let kind = if arms.is_empty() {
                EdgeKind::True
            } else {
                EdgeKind::False
            };
            let edge = self.edge(
                current,
                Some(&Destination {
                    segment,
                    parameters,
                    kind,
                }),
                kind,
                &values,
            )?;
            let tree = if shared && !arms.is_empty() {
                vec![]
            } else {
                let terminal = self.block(block, segment, None)?;
                if terminal.current.is_some() {
                    return Err(super::invalid("residual branch destination is not terminal"));
                }
                terminal.tree
            };
            arms.push((edge, tree));
        }
        let (on_true, a) = arms.remove(0);
        let (on_false, b) = arms.remove(0);
        // A shared successor is a continuation: each arm binds its incoming tuple,
        // then its parameters and body are defined exactly once after the dispatch.
        let (a, b, continuation) = if shared {
            (vec![], vec![], a)
        } else {
            (a, b, vec![])
        };
        flow.tree.push(ControlTree::If {
            id: self.control()?,
            predicate: Atom::new(condition, 0),
            on_true,
            then_body: Box::new(ControlTree::Sequence(a)),
            on_false,
            else_body: Box::new(ControlTree::Sequence(b)),
        });
        flow.tree.extend(continuation);
        flow.current = None;
        Ok(())
    }

    fn fpi(&mut self, op: &Operation, segment: SegmentId) -> Result<(), Report> {
        use crate::lower::units::fpi::{Fpi, Operands};
        let call = op.downcast_ref::<midenc_dialect_hir::ExecFpi>().unwrap();
        let prefix = self.ids(call.prefix().iter().map(|v| v.borrow().as_value_ref()))?;
        let payload = self.ids(call.inputs().iter().map(|v| v.borrow().as_value_ref()))?;
        let outputs = self.results(op)?;
        if prefix.len() != 6
            || prefix
                .iter()
                .chain(&payload)
                .chain(&outputs)
                .any(|v| self.program.values[v].layout.ty() != &Type::Felt)
        {
            return Err(super::invalid(
                "FPI requires six felt prefix operands and felt payload/results",
            ));
        }
        let prefix = self.atoms(&prefix);
        let payload = self.atoms(&payload);
        let outputs = self.atoms(&outputs);
        let source = self.sources[&op.as_operation_ref()];
        let id = SelectedStepId(index(self.definitions.len())?);
        let (unit, proof) = Fpi.select_semantic(
            id,
            source,
            Operands {
                account_suffix: prefix[0],
                account_prefix: prefix[1],
                procedure_root: [prefix[2], prefix[3], prefix[4], prefix[5]],
                payload: &payload,
            },
            &outputs,
        )?;
        self.add_unit(
            segment,
            source,
            UnitSelection {
                inputs: unit.stack_inputs,
                stored: unit.stored_inputs,
                outputs: unit.outputs,
                body: HirBody::Fpi,
                proof,
                effects: unit.effects,
                scratch: unit.scratch,
                scratch_effects: unit.scratch_effects,
            },
        )?;
        Ok(())
    }

    fn direct(&mut self, op: &Operation, segment: SegmentId) -> Result<(), Report> {
        use crate::lower::units::calls;
        let call = op.as_trait::<dyn midenc_hir::CallOpInterface>().unwrap();
        let (resolved, target) = calls::resolve_direct(call, self.env)?;
        let symbol = resolved.target().as_symbol_ref();
        let symbol = symbol.borrow();
        if calls::is_truncate(&*symbol) {
            return Err(super::invalid("truncate_stack requires registered return cleanup"));
        }
        let signature = resolved.signature();
        let convention = if calls::is_kernel(&*symbol) {
            CallingConvention::Kernel
        } else {
            calling_convention(&signature.cc)?
        };
        let abi = ProcedureAbi::new(
            convention,
            lanes(signature.params().iter().map(|p| p.ty.clone()))?,
            lanes(signature.results().iter().map(|p| p.ty.clone()))?,
        )?;
        let instruction = if op.is::<midenc_dialect_hir::Exec>() {
            I::Exec(target)
        } else if op.is::<midenc_dialect_hir::Call>() {
            I::Call(target)
        } else {
            I::SysCall(target)
        };
        let definition = calls::Direct { abi, instruction };
        require_types(
            result_types(op),
            signature.results().iter().map(|p| p.ty.clone()),
            "direct results disagree with target signature",
        )?;
        let mut arguments =
            self.ids(op.operands().all().iter().map(|v| v.borrow().as_value_ref()))?;
        if arguments.len() != signature.params().len() {
            return Err(super::invalid("direct argument count disagrees with target signature"));
        }
        let source = self.sources[&op.as_operation_ref()];
        for (index, (argument, param)) in arguments.iter_mut().zip(signature.params()).enumerate() {
            let ty = self.program.values[argument].layout.ty().clone();
            if param.is_sret_param()
                && (index != 0 || !signature.results().is_empty() || !ty.is_pointer())
            {
                return Err(super::invalid(
                    "sret requires first pointer argument and no direct results",
                ));
            }
            if ty == param.ty {
                continue;
            }
            use builtin::attributes::ArgumentExtension;
            let valid = match param.extension() {
                ArgumentExtension::None => false,
                ArgumentExtension::Zext => {
                    ty.is_unsigned_integer()
                        && param.ty.is_unsigned_integer()
                        && ty.size_in_bits() <= param.ty.size_in_bits()
                }
                ArgumentExtension::Sext => {
                    ty.is_integer()
                        && param.ty.is_signed_integer()
                        && if ty.is_unsigned_integer() {
                            ty.size_in_bits() < param.ty.size_in_bits()
                        } else {
                            ty.size_in_bits() <= param.ty.size_in_bits()
                        }
                }
            };
            if !valid {
                return Err(super::invalid("invalid direct argument extension"));
            }
            let mut conversion = crate::lower::units::arithmetic::Arithmetic::extension(
                op,
                ty,
                param.ty.clone(),
                param.extension(),
            )?;
            let converted = self.temporary(source, param.ty.clone())?;
            let inputs = self.atoms(&[*argument]);
            let outputs = self.atoms(&[converted]);
            conversion.bind(&inputs, &outputs);
            let proof = core::mem::take(&mut conversion.proof);
            let effects = conversion.effects.clone();
            self.add_unit(
                segment,
                source,
                UnitSelection {
                    inputs,
                    stored: vec![],
                    outputs,
                    body: HirBody::Arithmetic(Box::new(conversion)),
                    proof,
                    effects,
                    scratch: vec![],
                    scratch_effects: vec![],
                },
            )?;
            *argument = converted;
        }
        let outputs = self.results(op)?;
        let id = SelectedStepId(index(self.definitions.len())?);
        let (unit, proof) = definition.select(
            LoweringKind(id.0),
            id,
            source,
            &self.atoms(&arguments),
            &self.atoms(&outputs),
        )?;
        self.add_unit(
            segment,
            source,
            UnitSelection {
                inputs: unit.stack_inputs,
                stored: unit.stored_inputs,
                outputs: unit.outputs,
                body: HirBody::Direct(definition),
                proof,
                effects: unit.effects,
                scratch: unit.scratch,
                scratch_effects: unit.scratch_effects,
            },
        )?;
        Ok(())
    }

    fn indirect(
        &mut self,
        op: &Operation,
        current: SegmentId,
        flow: &mut Flow,
    ) -> Result<(), Report> {
        let source = self.sources[&op.as_operation_ref()];
        let signature = if let Some(call) = op.downcast_ref::<midenc_dialect_hir::Dyncall>() {
            call.get_signature().clone()
        } else {
            op.downcast_ref::<midenc_dialect_hir::ExecIndirect>()
                .unwrap()
                .get_signature()
                .clone()
        };
        require_types(
            result_types(op),
            signature.results().iter().map(|p| p.ty.clone()),
            "indirect results disagree with declared signature",
        )?;
        let outputs = self.results(op)?;
        let output_atoms = self.atoms(&outputs);
        let stage = SelectedStepId(index(self.definitions.len())?);
        let dispatch = SelectedStepId(
            stage
                .0
                .checked_add(1)
                .ok_or_else(|| super::invalid("phase identity overflows"))?,
        );
        let next = self.segment(vec![])?;
        if let Some(call) = op.downcast_ref::<midenc_dialect_hir::Dyncall>() {
            if call.get_signature().cc != CallConv::ComponentModel {
                return Err(super::invalid("dyncall requires context convention"));
            }
            let roots = self.ids(call.root().iter().map(|v| v.borrow().as_value_ref()))?;
            let arguments = self.ids(call.arguments().iter().map(|v| v.borrow().as_value_ref()))?;
            let root = self.atoms(&roots);
            let args = self.atoms(&arguments);
            if root.len() != 4 {
                return Err(super::invalid("dynamic root requires exactly four felt lanes"));
            }
            let abi = ProcedureAbi::new(
                CallingConvention::Context,
                lanes(signature.params().iter().map(|p| p.ty.clone()))?,
                lanes(signature.results().iter().map(|p| p.ty.clone()))?,
            )?;
            let object = ScratchId(source.0);
            let scratch = ScratchRequirement {
                id: object,
                size: 4,
                alignment: 4,
                contiguous: true,
                address_space: AddressSpace::ProcedureLocal,
                lifetime: ScratchLifetime::Spanning,
                uses: vec![stage, dispatch],
            };
            let facts = root
                .iter()
                .enumerate()
                .map(|(i, a)| ScratchCellFact {
                    object,
                    offset: i as u32,
                    content: ContentRequirement::Atom(*a),
                })
                .collect::<Vec<_>>();
            self.add_unit(
                current,
                source,
                UnitSelection {
                    inputs: root.clone(),
                    stored: vec![],
                    outputs: vec![],
                    body: HirBody::Indirect(Indirect::StageRoot { object }),
                    proof: UnitEvidence::default(),
                    effects: LoweringEffects {
                        may_trap: true,
                        ..Default::default()
                    },
                    scratch: vec![scratch.clone()],
                    scratch_effects: facts.iter().copied().map(ScratchEffect::Write).collect(),
                },
            )?;
            abi.invocation(Invocation::DynCall)?;
            let proof = UnitEvidence {
                calls: vec![CallSite {
                    instruction: I::DynCall,
                    abi,
                    inputs: args
                        .iter()
                        .map(|a| CallArgument::Exact(StoredValue::Atom(*a)))
                        .collect(),
                    path: vec![],
                    truncate: false,
                    outputs: vec![],
                    publications: output_atoms.iter().copied().zip(0u32..).collect(),
                    selector: Some(SelectorEvidence::FrameRoot {
                        reference: FrameRef::Scratch { object, offset: 0 },
                        root: root.iter().copied().map(StoredValue::Atom).collect(),
                    }),
                }],
                ..Default::default()
            };
            self.add_unit(
                next,
                source,
                UnitSelection {
                    inputs: args,
                    stored: vec![],
                    outputs: output_atoms,
                    body: HirBody::Indirect(Indirect::DispatchRoot {
                        object,
                        results: abi.results(),
                    }),
                    proof,
                    effects: LoweringEffects::default(),
                    scratch: vec![scratch],
                    scratch_effects: facts.into_iter().map(ScratchEffect::Read).collect(),
                },
            )?;
        } else {
            let call = op.downcast_ref::<midenc_dialect_hir::ExecIndirect>().unwrap();
            if calling_convention(&call.get_signature().cc)? != CallingConvention::SameContext {
                return Err(super::invalid(
                    "indirect exec target must use same-context convention",
                ));
            }
            // HIR verification above resolves live entries and checks complete signatures, not tags alone.
            let table = op
                .nearest_symbol_table()
                .and_then(|s| {
                    s.borrow().as_symbol_table().and_then(|s| s.resolve(call.table().path()))
                })
                .ok_or_else(|| super::invalid("unresolved indirect function table"))?;
            let table = table.borrow();
            let table = table
                .as_symbol_operation()
                .downcast_ref::<builtin::FunctionTable>()
                .ok_or_else(|| super::invalid("indirect symbol is not a function table"))?;
            for entry in table
                .live_entries()
                .map_err(|_| super::invalid("invalid table body"))?
                .into_values()
            {
                let entry = entry.borrow();
                if *entry.get_type_tag() != *call.get_type_tag() {
                    continue;
                }
                let callee = entry.resolve_callable().map_err(|e| super::invalid(e.to_string()))?;
                let symbol = callee.target().as_symbol_ref();
                let symbol = symbol.borrow();
                if crate::lower::units::calls::is_truncate(&*symbol) {
                    return Err(super::invalid(
                        "truncate_stack requires registered return cleanup",
                    ));
                }
                let convention = if crate::lower::units::calls::is_kernel(&*symbol) {
                    CallingConvention::Kernel
                } else {
                    calling_convention(&callee.signature().cc)?
                };
                ProcedureAbi::new(convention, 0, 0)?.invocation(Invocation::DynExec)?;
            }
            let slots = *table.get_num_slots();
            let base = self
                .env
                .link_info
                .and_then(|info| {
                    info.function_tables().element_addr_of(table.as_function_table_ref())
                })
                .ok_or_else(|| super::invalid("indirect table has no linked address"))?;
            let tag = *call.get_type_tag();
            let index_value = self.value(call.index().as_value_ref())?;
            let arguments = self.ids(call.arguments().iter().map(|v| v.borrow().as_value_ref()))?;
            let args = self.atoms(&arguments);
            let address = self.temporary(source, Type::U32)?;
            let address = Atom::new(address, 0);
            let abi = ProcedureAbi::new(
                CallingConvention::SameContext,
                lanes(signature.params().iter().map(|p| p.ty.clone()))?,
                lanes(signature.results().iter().map(|p| p.ty.clone()))?,
            )?;
            self.add_unit(
                current,
                source,
                UnitSelection {
                    inputs: vec![Atom::new(index_value, 0)],
                    stored: vec![],
                    outputs: vec![address],
                    body: HirBody::Indirect(Indirect::Lookup { slots, base, tag }),
                    proof: UnitEvidence::default(),
                    effects: LoweringEffects {
                        memory: SourceMemoryEffects::READ,
                        may_trap: true,
                        ..Default::default()
                    },
                    scratch: vec![],
                    scratch_effects: vec![],
                },
            )?;
            let proof = UnitEvidence {
                calls: vec![CallSite {
                    instruction: I::DynExec,
                    abi,
                    inputs: args
                        .iter()
                        .map(|a| CallArgument::Exact(StoredValue::Atom(*a)))
                        .collect(),
                    path: vec![],
                    truncate: false,
                    outputs: vec![],
                    publications: output_atoms.iter().copied().zip(0u32..).collect(),
                    selector: Some(SelectorEvidence::CheckedTableSlot(TableLookup {
                        operation: op.as_operation_ref(),
                        table: table.as_operation_ref(),
                        lookup: stage,
                        address,
                        base,
                        slots,
                        tag,
                        signature: call.get_signature().clone(),
                    })),
                }],
                ..Default::default()
            };
            self.add_unit(
                next,
                source,
                UnitSelection {
                    inputs: args,
                    stored: vec![StoredInput {
                        id: StoredInputId(0),
                        value: StoredValue::Atom(address),
                    }],
                    outputs: output_atoms,
                    body: HirBody::Indirect(Indirect::DispatchTable),
                    proof,
                    effects: LoweringEffects::default(),
                    scratch: vec![],
                    scratch_effects: vec![],
                },
            )?;
        }
        let edge = self.edge(
            current,
            Some(&Destination {
                segment: next,
                parameters: vec![],
                kind: EdgeKind::Forward,
            }),
            EdgeKind::Forward,
            &[],
        )?;
        flow.tree.push(ControlTree::Edge(edge));
        flow.tree.push(ControlTree::Segment(next));
        flow.current = Some(next);
        Ok(())
    }
}
/// Construction-time inference is not an immutable verifier. Compare declared source
/// authorities explicitly; never repair malformed final HIR by rerunning inference.
fn require_types(
    actual: impl IntoIterator<Item = Type>,
    expected: impl IntoIterator<Item = Type>,
    message: &'static str,
) -> Result<(), Report> {
    if actual.into_iter().eq(expected) {
        Ok(())
    } else {
        Err(super::invalid(message))
    }
}
fn operand_types(op: &Operation) -> Vec<Type> {
    op.operands()
        .all()
        .iter()
        .map(|v| v.borrow().as_value_ref().borrow().ty().clone())
        .collect()
}
fn result_types(op: &Operation) -> Vec<Type> {
    op.results().all().iter().map(|v| v.borrow().ty().clone()).collect()
}
fn limb_group(whole: &Type, limb: &Type, count: usize) -> bool {
    matches!(
        (whole, limb, count),
        (Type::I64 | Type::U64, Type::Felt | Type::I32 | Type::U32, 2)
            | (Type::I128 | Type::U128, Type::I64 | Type::U64, 2)
            | (Type::I128 | Type::U128, Type::Felt | Type::I32 | Type::U32, 4)
    )
}
fn validate_alias(op: &Operation, rule: SelectionRule) -> Result<(), Report> {
    use midenc_dialect_arith::{Join, Split};
    use midenc_dialect_hir::{Bitcast, IntToPtr, PtrToInt};
    match rule {
        SelectionRule::Split => {
            let split = op.downcast_ref::<Split>().unwrap();
            let whole = split.operand().ty();
            let limb = split.get_limb_ty();
            if !limb_group(&whole, &limb, op.num_results()) {
                return Err(super::invalid("unsupported split limb grouping"));
            }
            require_types(
                result_types(op),
                core::iter::repeat_n(limb.clone(), op.num_results()),
                "split results disagree with declared limb type",
            )
        }
        SelectionRule::Join => {
            let join = op.downcast_ref::<Join>().unwrap();
            let whole = join.get_ty();
            let limbs = operand_types(op);
            if limbs.first().is_none_or(|limb| {
                !limb_group(&whole, limb, limbs.len()) || limbs.iter().any(|other| other != limb)
            }) {
                return Err(super::invalid("unsupported join limb grouping"));
            }
            require_types(
                result_types(op),
                [whole.clone()],
                "join result disagrees with declared target type",
            )
        }
        SelectionRule::Intrinsic(kind) => {
            crate::lower::units::intrinsics::Intrinsic::from_source(kind, op)?;
            Ok(())
        }
        SelectionRule::Alias => {
            let target = if let Some(cast) = op.downcast_ref::<PtrToInt>() {
                cast.get_ty()
            } else if let Some(cast) = op.downcast_ref::<IntToPtr>() {
                cast.get_ty()
            } else {
                op.downcast_ref::<Bitcast>().unwrap().get_ty()
            };
            require_types(
                result_types(op),
                [target.clone()],
                "cast result disagrees with declared target type",
            )
        }
        _ => unreachable!("only registered aliases reach alias validation"),
    }
}

fn index(value: usize) -> Result<u32, Report> {
    u32::try_from(value).map_err(|_| super::invalid("selected identity domain exhausted"))
}
fn lanes(types: impl IntoIterator<Item = Type>) -> Result<u32, Report> {
    types.into_iter().try_fold(0u32, |n, ty| {
        n.checked_add(ValueLayout::new(ty)?.lanes() as u32)
            .ok_or_else(|| super::invalid("ABI lane count overflows"))
    })
}
/// Add actual source provenance only when a planning failure has no nested location.
fn at_operation(mut report: Report, op: &Operation) -> Report {
    if let Some(error) = report.downcast_mut::<PlanError>()
        && error.origin == SourceLocation::default()
    {
        error.origin = record(op).location;
    }
    report
}

fn record(op: &Operation) -> SourceRecord {
    SourceRecord {
        origin: SourceOrigin::Hir(op.as_operation_ref()),
        location: SourceLocation {
            span: op.span(),
            inline_chain: {
                use midenc_hir::dialects::debuginfo::attributes::{
                    INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChainAttr,
                };
                op.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME)
                    .and_then(|attr| attr.try_downcast_attr::<InlineCallChainAttr>().ok())
                    .map(|attr| {
                        use miden_assembly_syntax::debuginfo::{ColumnNumber, LineNumber, Uri};
                        attr.borrow()
                            .frames
                            .iter()
                            .map(|frame| {
                                let manager = &op.context().session().source_manager;
                                let span = LineNumber::new(frame.call_line).and_then(|line| {
                                    let file =
                                        manager.get_by_uri(&Uri::new(frame.call_file.as_str()))?;
                                    file.line_column_to_span(
                                        line,
                                        ColumnNumber::new(frame.call_column).unwrap_or_default(),
                                    )
                                });
                                span.unwrap_or(midenc_hir::SourceSpan::UNKNOWN)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            },
        },
    }
}
fn literal(value: Immediate) -> Result<Vec<miden_core::Felt>, Report> {
    use miden_core::Felt;
    let (bits, count) = match value {
        Immediate::Felt(v) => return Ok(vec![v]),
        Immediate::I1(v) => (v as u128, 1),
        Immediate::U8(v) => (v as u128, 1),
        Immediate::I8(v) => (v as u8 as u128, 1),
        Immediate::U16(v) => (v as u128, 1),
        Immediate::I16(v) => (v as u16 as u128, 1),
        Immediate::U32(v) => (v as u128, 1),
        Immediate::I32(v) => (v as u32 as u128, 1),
        Immediate::U64(v) => (v as u128, 2),
        Immediate::I64(v) => (v as u64 as u128, 2),
        Immediate::U128(v) => (v, 4),
        Immediate::I128(v) => (v as u128, 4),
        _ => return Err(super::invalid("unsupported final HIR literal representation")),
    };
    Ok((0..count)
        .map(|lane| Felt::new_unchecked(((bits >> (lane * 32)) & u32::MAX as u128) as u64))
        .collect())
}

fn calling_convention(convention: &CallConv) -> Result<CallingConvention, Report> {
    match convention {
        CallConv::C | CallConv::Fast | CallConv::Wasm => Ok(CallingConvention::SameContext),
        CallConv::ComponentModel => Ok(CallingConvention::Context),
        CallConv::Extern(_) => Err(super::invalid("unregistered source calling convention")),
    }
}
