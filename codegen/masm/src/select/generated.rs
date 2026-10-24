//! Source authority for compiler-generated root procedures.
//!
//! Generated roots have no HIR body to reconstruct. Their registered definition instead derives
//! the selected graph, call evidence, and executable stages from the same semantic parameters.
//! Stage order and the final VM stack cleanup are part of that definition's contract.
use alloc::{collections::BTreeMap, vec, vec::Vec};

use miden_core::Felt;
use midenc_hir::{Report, Type};

use crate::{
    lower::{
        expand::{AnnotatedMasmBody, ExpansionEnv, annotate_raw},
        raw,
        units::{LoweringBindings, LoweringDefinition},
    },
    masm::{self, Instruction},
    plan::*,
    verify::{
        CallArgument, CallSite, DefinitionEvidence, GeneratedDefinition, UnitEvidence,
        VerificationError,
    },
};

/// Semantic inputs for a root wrapper which initializes, invokes an entrypoint, and cleans up.
/// Implements source reconstruction through `GeneratedDefinition`; `definitions()` supplies
/// the corresponding executable lowerings for registration before expansion.
pub(crate) struct GeneratedRoot {
    /// Optional initialization called before the harness and entrypoint, with no ABI lanes.
    pub initializer: Option<masm::InvocationTarget>,
    /// Optional test harness setup, also called with no ABI lanes.
    pub harness: Option<masm::InvocationTarget>,
    /// Entrypoint invoked after any setup stages.
    pub entry: masm::InvocationTarget,
    /// Entrypoint argument/result counts; root selection requires at most sixteen arguments.
    pub entry_abi: ProcedureAbi,
}
/// One closed executable stage of a [`GeneratedRoot`].
/// Its private stage identity fixes both the lowering kind and the matching generated source step.
pub(crate) struct RootStage {
    stage: u32,
    target: masm::InvocationTarget,
    abi: ProcedureAbi,
}
impl GeneratedRoot {
    /// Construct present setup stages, the entry call, and mandatory stack truncation in order.
    /// Stage identities remain fixed when optional setup stages are absent.
    pub fn definitions(&self) -> Vec<RootStage> {
        let mut stages = Vec::new();
        for (stage, target) in [
            (0, self.initializer.as_ref()),
            (1, self.harness.as_ref()),
            (2, Some(&self.entry)),
        ] {
            if let Some(target) = target {
                stages.push(RootStage {
                    stage,
                    target: target.clone(),
                    abi: if stage == 2 {
                        self.entry_abi
                    } else {
                        ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap()
                    },
                });
            }
        }
        stages.push(RootStage {
            stage: 3,
            target: raw::target("::miden::core::sys::truncate_stack"),
            abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
        });
        stages
    }
}
impl GeneratedDefinition for GeneratedRoot {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(1000)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() || self.entry_abi.arguments() > 16 {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "root parameters exceed the supported input window",
            ));
        }
        self.entry_abi.invocation(Invocation::Exec)?;
        // Inputs, entry results and cleanup results occupy consecutive disjoint ranges.
        // Check the entire range before constructing identities or narrowing host indices.
        let entry_base = 16u32;
        let cleanup_base = entry_base
            .checked_add(self.entry_abi.results())
            .filter(|base| base.checked_add(16).is_some())
            .ok_or_else(|| {
                PlanError::new(
                    DiagnosticCategory::InvalidSelectedLowering,
                    "generated root value identity range overflows",
                )
            })?;
        let atom = |i| Atom::new(ValueId(i), 0);
        let mut values = BTreeMap::new();
        for i in 0..16 {
            values.insert(
                ValueId(i),
                SelectedValue {
                    id: ValueId(i),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(0),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: None,
                },
            );
        }
        for i in 0..self.entry_abi.results() {
            values.insert(
                ValueId(entry_base + i),
                SelectedValue {
                    id: ValueId(entry_base + i),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(2),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: None,
                },
            );
        }
        let after = (0..self.entry_abi.results())
            .map(|i| atom(entry_base + i))
            .chain((self.entry_abi.arguments()..16).map(atom))
            .collect::<Vec<_>>();
        for i in 0..16 {
            values.insert(
                ValueId(cleanup_base + i),
                SelectedValue {
                    id: ValueId(cleanup_base + i),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(3),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: if usize::try_from(i).is_ok_and(|index| index >= after.len()) {
                        Some(Reconstruction::Literal(vec![Felt::ZERO]))
                    } else {
                        None
                    },
                },
            );
        }
        let mut units = Vec::new();
        let mut proofs = BTreeMap::new();
        let mut source_map = BTreeMap::new();
        for definition in self.definitions() {
            let id = SelectedStepId(definition.stage);
            let source = SourceStepId(definition.stage);
            let inputs = match definition.stage {
                2 => (0..self.entry_abi.arguments()).map(atom).collect::<Vec<_>>(),
                3 => after.clone(),
                _ => vec![],
            };
            let outputs = match definition.stage {
                2 => {
                    (0..self.entry_abi.results()).map(|i| atom(entry_base + i)).collect::<Vec<_>>()
                }
                3 => (0..16).map(|i| atom(cleanup_base + i)).collect::<Vec<_>>(),
                _ => vec![],
            };
            let cleanup = definition.stage == 3;
            let aliases = if cleanup {
                LaneAliases::new(outputs.iter().zip(&inputs).map(|(o, i)| (*o, *i)).collect())
            } else {
                LaneAliases::default()
            };
            let mut call_inputs = inputs
                .iter()
                .map(|a| CallArgument::Exact(StoredValue::Atom(*a)))
                .collect::<Vec<_>>();
            if cleanup {
                call_inputs.clear();
            }
            let proof = UnitEvidence {
                aliases,
                calls: vec![CallSite {
                    instruction: Instruction::Exec(definition.target.clone()),
                    abi: definition.abi,
                    inputs: call_inputs,
                    path: vec![],
                    truncate: cleanup,
                    outputs: vec![],
                    publications: if cleanup {
                        vec![]
                    } else {
                        outputs.iter().zip(0u32..).map(|(a, i)| (*a, i)).collect()
                    },
                    selector: None,
                }],
                ..Default::default()
            };
            let unit = SelectedLowering {
                id,
                kind: definition.kind(),
                source,
                semantic_inputs: inputs.iter().map(|a| a.value).collect(),
                stack_inputs: inputs,
                stored_inputs: vec![],
                outputs,
                publication: OutputPublication::Stack,
                address_results: vec![],
                effects: LoweringEffects {
                    memory: if definition.stage < 2 {
                        SourceMemoryEffects::WRITE
                    } else {
                        SourceMemoryEffects::default()
                    },
                    ..Default::default()
                },
                scratch: vec![],
                scratch_effects: vec![],
            };
            source_map.insert(
                source,
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: self.id(),
                        stage: definition.stage,
                    },
                    location: Default::default(),
                },
            );
            proofs.insert(id, proof);
            units.push(unit);
        }
        Ok(DefinitionEvidence {
            source_locals: Default::default(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![SelectedSegment {
                    id: SegmentId(0),
                    parameters: (0..16).map(ValueId).collect(),
                    content_parameters: vec![],
                    units,
                }],
                edges: vec![TransferEdge {
                    id: EdgeId(0),
                    source: SegmentId(0),
                    destination: None,
                    kind: EdgeKind::Return,
                    bindings: (0..16)
                        .map(|i| ParameterBinding {
                            destination: ValueId(cleanup_base + i),
                            source: ValueId(cleanup_base + i),
                        })
                        .collect(),
                    content_bindings: vec![],
                }],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::Root, 16, 16)?,
                source_map,
            },
            units: proofs,
        })
    }
}
impl LoweringDefinition for RootStage {
    fn kind(&self) -> LoweringKind {
        LoweringKind(1100 + self.stage)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut b = raw::Builder::new(Default::default());
        // Materialize only established VM floor zeros when the meaningful root state is short.
        // Truncation then preserves the same top sixteen and removes every overflow lane.
        if self.stage == 3 && unit.stack_inputs.len() < 16 {
            b.extend(raw::pad_inputs(unit.stack_inputs.len(), b.span));
        }
        b.exec(self.target.clone());
        Ok(annotate_raw(&b.ops, unit.id))
    }
}

/// An ordered generated initialization stage. The enum fixes the executable construction;
/// callers supply semantic linked facts rather than raw bodies or candidate proof annotations.
#[derive(Clone)]
pub(crate) enum Initialization {
    /// Traverse the actual initializer region and store its result at the linked byte address.
    Global(midenc_hir::dialects::builtin::GlobalVariableRef),
    /// Initialize the heap from its linked byte address.
    Heap {
        base: u32,
    },
    /// Load committed advice into a word-aligned native-element destination; words counts words.
    Data {
        commitment: [Felt; 4],
        destination: u32,
        words: u32,
    },
    Table(TableFragment),
    /// Invoke a transparent compiler fragment or a no-argument component start procedure.
    Invoke(masm::InvocationTarget),
}
/// One module-owned function-table fragment, including its child-fragment dependencies.
/// Actual live table entries determine writes; private procedure references stay in their owner.
#[derive(Clone)]
pub(crate) struct TableFragment {
    pub owner: midenc_hir::OperationRef,
    /// Table bases are word-aligned native-element addresses in the linked layout.
    pub tables: Vec<(midenc_hir::dialects::builtin::FunctionTableRef, u32)>,
    /// Execute child fragments after this owner has installed its own live entries.
    pub children: Vec<masm::InvocationTarget>,
}
/// Shared generated initializer authority. HIR regions and generated setup stages are selected
/// together, so allocation and frame assignment see all fragments once in their original order.
pub(crate) struct GeneratedInitializer<'a, 'e> {
    pub stages: Vec<Initialization>,
    pub env: &'a super::SelectEnv<'e>,
}
impl GeneratedInitializer<'_, '_> {
    /// Reconstruct semantic evidence and executable definitions in the same source traversal.
    pub fn evidence(&self) -> Result<super::HirEvidence, Report> {
        super::graph::Selector::generated(self.id(), &self.stages, self.env)
    }
}
impl GeneratedDefinition for GeneratedInitializer<'_, '_> {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(1001)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(generated_error("initializer accepts no extra parameters"));
        }
        self.evidence().map(|e| e.source).map_err(|e| generated_error(e.to_string()))
    }

    fn observations(
        &self,
        parameters: &[Felt],
    ) -> Result<Vec<crate::lower::debug::Observation>, VerificationError> {
        if !parameters.is_empty() {
            return Err(generated_error("initializer accepts no extra parameters"));
        }
        self.evidence()
            .map(|e| e.observations)
            .map_err(|e| generated_error(e.to_string()))
    }
}
/// Fixed straight-line construction paired with source-derived helper contracts.
pub(super) struct StageConstruction {
    pub instructions: Vec<Instruction>,
    pub proof: UnitEvidence,
    pub effects: LoweringEffects,
}
fn generated_error(message: impl Into<alloc::string::String>) -> PlanError {
    PlanError::new(DiagnosticCategory::InvalidSelectedLowering, message)
}
fn stage_call(
    target: masm::InvocationTarget,
    inputs: Vec<CallArgument>,
    results: u32,
) -> Result<CallSite, PlanError> {
    Ok(CallSite {
        instruction: Instruction::Exec(target),
        abi: ProcedureAbi::new(CallingConvention::SameContext, inputs.len() as u32, results)?,
        inputs,
        path: vec![],
        truncate: false,
        outputs: vec![],
        publications: vec![],
        selector: None,
    })
}
impl Initialization {
    /// Construct only the finite generated stage selected by these source parameters.
    pub(super) fn construct(
        &self,
        env: &super::SelectEnv<'_>,
    ) -> Result<StageConstruction, Report> {
        use midenc_hir::Spanned;
        let mut b = raw::Builder::new(Default::default());
        let mut proof = UnitEvidence::default();
        let exact = |value: Felt| CallArgument::Exact(StoredValue::Literal(value));
        match self {
            Self::Global(_) => {
                return Err(generated_error("global regions require source traversal").into());
            }
            Self::Heap { base } => {
                b.emit_push(*base);
                let target = raw::target("::intrinsics::mem::heap_init");
                proof.calls.push(stage_call(target.clone(), vec![exact((*base).into())], 0)?);
                b.exec(target);
            }
            Self::Data {
                commitment,
                destination,
                words,
            } => {
                if !destination.is_multiple_of(4)
                    || u64::from(*destination) + 4 * u64::from(*words) > 1u64 << 32
                {
                    return Err(generated_error(
                        "data initialization range or word alignment is invalid",
                    )
                    .into());
                }
                for value in commitment.iter().rev() {
                    b.emit_push(*value);
                }
                b.emit(Instruction::EmitImm(
                    masm::Immediate::from(
                        miden_core::events::SystemEvent::MapValueToStack.event_id().as_felt(),
                    )
                    .into(),
                ));
                b.emit_push(*destination);
                b.emit_push(*words);
                let target = raw::target("::miden::core::mem::pipe_preimage_to_memory");
                let inputs = [Felt::from(*words), Felt::from(*destination)]
                    .into_iter()
                    .chain(commitment.iter().copied())
                    .map(exact)
                    .collect();
                proof.calls.push(stage_call(target.clone(), inputs, 1)?);
                b.exec(target);
                b.emit(Instruction::Drop);
            }
            Self::Invoke(target) => {
                proof.calls.push(stage_call(target.clone(), vec![], 0)?);
                b.exec(target.clone());
            }
            Self::Table(fragment) => {
                for (table, base) in &fragment.tables {
                    if !base.is_multiple_of(4) {
                        return Err(generated_error("table fragment base is unaligned").into());
                    }
                    let table = table.borrow();
                    for (slot, entry) in table
                        .live_entries()
                        .map_err(|_| generated_error("unsupported function table entry"))?
                    {
                        let entry = entry.borrow();
                        if slot >= *table.get_num_slots() || *entry.get_type_tag() == 0 {
                            return Err(generated_error(
                                "invalid function table slot or null signature tag",
                            )
                            .into());
                        }
                        let callee = entry
                            .resolve_callee()
                            .ok_or_else(|| generated_error("table entry has no callee"))?;
                        let callee_borrow = callee.borrow();
                        if callee_borrow.is_declaration() {
                            return Err(generated_error(
                                "table entry callee has no executable definition",
                            )
                            .into());
                        }
                        if callee_borrow.as_symbol_operation().nearest_symbol_table()
                            != Some(fragment.owner)
                        {
                            continue;
                        }
                        let target = crate::lower::invocation_target_from_symbol(
                            callee,
                            Some(fragment.owner),
                            entry.span(),
                        );
                        let target = if let Some(owners) = env.owners {
                            owners.target(target)?
                        } else {
                            target
                        };
                        let address = base
                            .checked_add(
                                slot.checked_mul(
                                    crate::linker::FunctionTableLayout::SLOT_SIZE_ELEMENTS,
                                )
                                .ok_or_else(|| generated_error("table slot offset overflow"))?,
                            )
                            .filter(|address| address.checked_add(7).is_some())
                            .ok_or_else(|| generated_error("table slot address overflow"))?;
                        b.emit(Instruction::ProcRef(target));
                        b.emit(Instruction::MemStoreWLeImm(address.into()));
                        b.emit(Instruction::DropW);
                        b.emit_push(*entry.get_type_tag());
                        b.emit(Instruction::MemStoreImm((address + 4).into()));
                    }
                }
                for target in &fragment.children {
                    proof.calls.push(stage_call(target.clone(), vec![], 0)?);
                    b.exec(target.clone());
                }
            }
        }
        let instructions = b
            .ops
            .into_iter()
            .map(|op| match op {
                masm::Op::Inst(inst) => inst.into_inner(),
                _ => unreachable!("initialization stages are straight line"),
            })
            .collect();
        Ok(StageConstruction {
            instructions,
            proof,
            effects: LoweringEffects {
                memory: if matches!(self, Self::Invoke(_) | Self::Table(_)) {
                    SourceMemoryEffects::READ_WRITE
                } else {
                    SourceMemoryEffects::WRITE
                },
                may_trap: true,
                normal_return: true,
            },
        })
    }
}

/// Advice-driven memory initialization used by the optional executable test harness.
/// Its complete loop is source-owned and preserves the caller's operand suffix.
pub(crate) struct GeneratedHarness;
impl GeneratedHarness {
    fn proof(&self) -> Result<UnitEvidence, PlanError> {
        use crate::verify::{CallPath, LoopPosition, LoopSchema};
        let mut call = stage_call(
            raw::target("::miden::core::mem::pipe_words_to_memory"),
            vec![CallArgument::Local; 2],
            13,
        )?;
        call.path = vec![CallPath::Loop(ControlId(0))];
        Ok(UnitEvidence {
            calls: vec![call],
            loops: BTreeMap::from([(
                ControlId(0),
                LoopSchema {
                    stack: vec![LoopPosition::Carry(0)],
                    cells: vec![],
                },
            )]),
            ..Default::default()
        })
    }

    fn body(&self) -> Vec<masm::Op> {
        let mut b = raw::Builder::new(Default::default());
        b.emit(Instruction::AdvPush);
        b.emit(Instruction::Dup0);
        b.emit_push(0u32);
        b.emit(Instruction::Gt);
        let mut body = raw::Builder::new(Default::default());
        body.emit(Instruction::SubImm(Felt::ONE.into()));
        body.emit(Instruction::AdvPush);
        body.emit(Instruction::AdvPush);
        body.exec(raw::target("::miden::core::mem::pipe_words_to_memory"));
        for _ in 0..3 {
            body.emit(Instruction::DropW);
        }
        body.emit(Instruction::Drop);
        body.emit(Instruction::Dup0);
        body.emit_push(0u32);
        body.emit(Instruction::Gt);
        b.ops.push(masm::Op::While {
            span: Default::default(),
            body: masm::Block::new(Default::default(), body.ops),
        });
        b.emit(Instruction::Drop);
        b.ops
    }
}
impl GeneratedDefinition for GeneratedHarness {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(1002)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(generated_error("harness accepts no extra parameters"));
        }
        single_generated(self.id(), self.kind(), self.proof()?, SourceMemoryEffects::READ_WRITE)
    }
}
impl LoweringDefinition for GeneratedHarness {
    fn kind(&self) -> LoweringKind {
        LoweringKind(1300)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        Ok(annotate_raw(&self.body(), unit.id))
    }
}
fn single_generated(
    definition: GeneratedDefinitionId,
    kind: LoweringKind,
    proof: UnitEvidence,
    memory: SourceMemoryEffects,
) -> Result<DefinitionEvidence, PlanError> {
    Ok(DefinitionEvidence {
        source_locals: Default::default(),
        units: BTreeMap::from([(SelectedStepId(0), proof)]),
        program: SelectedProgram {
            origin: ProcedureOrigin::Generated {
                definition,
                parameters: vec![],
            },
            values: BTreeMap::new(),
            segments: vec![SelectedSegment {
                id: SegmentId(0),
                parameters: vec![],
                content_parameters: vec![],
                units: vec![SelectedLowering {
                    id: SelectedStepId(0),
                    source: SourceStepId(0),
                    kind,
                    semantic_inputs: vec![],
                    stack_inputs: vec![],
                    stored_inputs: vec![],
                    outputs: vec![],
                    publication: OutputPublication::Stack,
                    address_results: vec![],
                    effects: LoweringEffects {
                        memory,
                        may_trap: true,
                        normal_return: true,
                    },
                    scratch: vec![],
                    scratch_effects: vec![],
                }],
            }],
            edges: vec![TransferEdge {
                id: EdgeId(0),
                source: SegmentId(0),
                destination: None,
                kind: EdgeKind::Return,
                bindings: vec![],
                content_bindings: vec![],
            }],
            structure: ControlTree::Sequence(vec![
                ControlTree::Segment(SegmentId(0)),
                ControlTree::Edge(EdgeId(0)),
            ]),
            abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0)?,
            source_map: BTreeMap::from([(
                SourceStepId(0),
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition,
                        stage: 0,
                    },
                    location: Default::default(),
                },
            )]),
        },
    })
}

/// A public entry adapter whose component initialization precedes the actual selected HIR body.
/// The original function supplies ABI, locals, return normalization and debug metadata; the
/// generated authority adds only a checked transparent prologue before physical planning.
pub(crate) struct GeneratedExport<'a, 'e> {
    pub function: midenc_hir::dialects::builtin::FunctionRef,
    pub initializer: masm::InvocationTarget,
    pub env: &'a super::SelectEnv<'e>,
}
impl GeneratedExport<'_, '_> {
    pub fn evidence(&self) -> Result<super::HirEvidence, Report> {
        let function = self.function.borrow();
        let mut evidence = super::evidence(&function, self.env)?;
        let program = &mut evidence.source.program;
        program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let step = SelectedStepId(
            u32::try_from(evidence.definitions.len())
                .map_err(|_| generated_error("export phase identity overflow"))?,
        );
        let kind = LoweringKind(
            2000u32
                .checked_add(step.0)
                .ok_or_else(|| generated_error("export lowering identity overflow"))?,
        );
        let source = SourceStepId(
            u32::try_from(program.source_map.len())
                .map_err(|_| generated_error("export source identity overflow"))?,
        );
        program.source_map.insert(
            source,
            SourceRecord {
                origin: SourceOrigin::Generated {
                    definition: self.id(),
                    stage: 0,
                },
                location: Default::default(),
            },
        );
        program.segments[0].units.insert(
            0,
            SelectedLowering {
                id: step,
                source,
                kind,
                semantic_inputs: vec![],
                stack_inputs: vec![],
                stored_inputs: vec![],
                outputs: vec![],
                publication: OutputPublication::Stack,
                address_results: vec![],
                effects: LoweringEffects {
                    memory: SourceMemoryEffects::READ_WRITE,
                    may_trap: true,
                    normal_return: true,
                },
                scratch: vec![],
                scratch_effects: vec![],
            },
        );
        evidence.source.units.insert(
            step,
            UnitEvidence {
                calls: vec![stage_call(self.initializer.clone(), vec![], 0)?],
                ..Default::default()
            },
        );
        let mut b = raw::Builder::new(Default::default());
        b.exec(self.initializer.clone());
        let instructions = b
            .ops
            .into_iter()
            .map(|op| match op {
                masm::Op::Inst(inst) => inst.into_inner(),
                _ => unreachable!(),
            })
            .collect();
        evidence.definitions.push(crate::lower::units::HirUnit {
            kind,
            body: crate::lower::units::HirBody::Plain(instructions),
        });
        for observation in &mut evidence.observations {
            if observation.segment == program.segments[0].id {
                observation.position += 1;
            }
        }
        Ok(evidence)
    }
}
impl GeneratedDefinition for GeneratedExport<'_, '_> {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(1003)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(generated_error("export accepts no extra parameters"));
        }
        self.evidence().map(|e| e.source).map_err(|e| generated_error(e.to_string()))
    }

    fn observations(
        &self,
        parameters: &[Felt],
    ) -> Result<Vec<crate::lower::debug::Observation>, VerificationError> {
        if !parameters.is_empty() {
            return Err(generated_error("export accepts no extra parameters"));
        }
        self.evidence()
            .map(|e| e.observations)
            .map_err(|e| generated_error(e.to_string()))
    }
}
