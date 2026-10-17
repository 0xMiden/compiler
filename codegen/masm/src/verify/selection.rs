//! Source correspondence is checked before any physical contract becomes a premise.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::ops::Deref;

use miden_core::Felt;

use super::{VerificationError, error};
use crate::{masm::Instruction, plan::*};

/// Implemented by registered generated definitions, never by an allocation plan.
/// The definition reconstructs stages and bindings from its semantic parameters.
pub(crate) trait GeneratedDefinition {
    /// Identity matched against the selected procedure's generated origin.
    fn id(&self) -> GeneratedDefinitionId;
    /// Reconstruct the complete semantic stages and proof relations, rejecting unsupported
    /// parameter sets rather than weakening the registered definition.
    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError>;
}

/// Trusted source from which selection evidence is reconstructed, supplied independently of
/// the allocation plan being checked.
pub(crate) enum SourceContext<'a> {
    Generated(&'a dyn GeneratedDefinition),
    /// Reserved HIR source context; source checking rejects this variant until HIR rules
    /// can reconstruct the selected program.
    Hir,
}

/// Complete source-derived expectation used to check selection. Implementors must derive
/// this from registered semantics and parameters rather than copying an untrusted plan.
#[derive(Debug, Clone)]
pub(crate) struct DefinitionEvidence {
    /// Exact source-owned frame prefix to preserve during assignment.
    pub source_locals: SourceLocalLayout,
    pub program: SelectedProgram,
    pub units: BTreeMap<SelectedStepId, UnitEvidence>,
}

/// Source-derived relations and internal control/call contracts absent from the allocator's
/// physical choices. These become private evidence in the checked selected program.
#[derive(Debug, Clone, Default)]
pub(crate) struct UnitEvidence {
    pub aliases: LaneAliases,
    pub equal_outputs: Vec<(u8, u8)>,
    pub scratch_outputs: Vec<(FrameRef, u8)>,
    /// Schemas for loops wholly internal to this unit, keyed by annotated control identity.
    pub loops: BTreeMap<ControlId, LoopSchema>,
    /// Ordered source-authorized call sites, with order interpreted within each path.
    pub calls: Vec<CallSite>,
}

/// One static invocation in a registered unit algorithm. Sites are ordered within their
/// internal control path, so replay checks target, argument identity, and result publication
/// against the exact site that executes.
#[derive(Debug, Clone)]
pub(crate) struct CallSite {
    /// Resolved target identity, including invocation family.
    pub instruction: Instruction,
    pub abi: ProcedureAbi,
    /// Exact exposed arguments, excluding padding and indirect selector.
    pub inputs: Vec<CallArgument>,
    /// Nested internal branch/loop route on which this static site occurs.
    pub path: Vec<CallPath>,
    /// Registered return-normalization helper; legal only at context/root boundaries.
    pub truncate: bool,
    pub outputs: Vec<(FrameRef, u8)>,
    pub publications: Vec<(Atom, u8)>,
    /// Dynamic selector proof: the declared frame reference and its four expected root lanes.
    /// Replay checks both that the selector points here and that the cells contain these values.
    pub selector: Option<(FrameRef, Vec<StoredValue>)>,
}

/// Source-derived constraint on one raw call argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallArgument {
    /// Argument equals the named atom or literal.
    Exact(StoredValue),
    /// Unit-local arithmetic is part of the registered algorithm, not an SSA binding.
    Local,
}

/// One nesting step identifying an internal call path within a lowering unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CallPath {
    Then(ControlId),
    Else(ControlId),
    Loop(ControlId),
}

/// Source-derived relation for one physical position at an internal loop header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopPosition {
    /// Preserve the token at this location on forward entry.
    Invariant,
    /// Simultaneous occurrences of a carried parameter must agree.
    Carry(u32),
}

/// Inductive schema for a loop internal to one lowering unit. Listed stack positions are
/// top-first; unlisted stack suffix positions remain invariant. Cells use symbolic frame refs.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoopSchema {
    pub stack: Vec<LoopPosition>,
    pub cells: Vec<(FrameRef, LoopPosition)>,
}

/// Only source checking constructs this wrapper. Its contents cannot be mutated by allocation.
#[derive(Debug, Clone)]
pub(crate) struct CheckedSelectedProgram {
    source_locals: SourceLocalLayout,
    program: SelectedProgram,
    units: BTreeMap<SelectedStepId, UnitEvidence>,
}

impl CheckedSelectedProgram {
    /// Sealed source-local layout used to check the assigned physical frame.
    pub(super) fn source_locals(&self) -> &SourceLocalLayout {
        &self.source_locals
    }

    /// Retrieve the registered proof relations for a unit, rejecting missing coverage.
    pub(super) fn evidence(
        &self,
        step: SelectedStepId,
    ) -> Result<&UnitEvidence, VerificationError> {
        self.units.get(&step).ok_or_else(|| error("missing registered unit evidence"))
    }
}

impl Deref for CheckedSelectedProgram {
    type Target = SelectedProgram;

    fn deref(&self) -> &Self::Target {
        &self.program
    }
}

/// Reconstruct source evidence, require exact agreement with the proposed selection, and
/// validate graph coverage, definitions, reconstruction, scratch, and structured control.
/// Success seals the program so allocation can borrow its semantics without rewriting them.
pub(crate) fn check_selection(
    program: SelectedProgram,
    source: &SourceContext<'_>,
) -> Result<CheckedSelectedProgram, VerificationError> {
    let (definition, parameters) = match (&program.origin, source) {
        (
            ProcedureOrigin::Generated {
                definition,
                parameters,
            },
            SourceContext::Generated(source),
        ) if *definition == source.id() => (*source, parameters),
        _ => return Err(error("unregistered or incompatible source definition")),
    };
    let expected = definition.instantiate(parameters)?;
    if program != expected.program {
        return Err(error(
            "selected phases, uses, effects, coverage or control disagree with source",
        ));
    }
    validate_graph(&program, &expected.units)?;
    Ok(CheckedSelectedProgram {
        source_locals: expected.source_locals,
        program,
        units: expected.units,
    })
}

fn validate_graph(
    program: &SelectedProgram,
    units: &BTreeMap<SelectedStepId, UnitEvidence>,
) -> Result<(), VerificationError> {
    let mut segments = BTreeSet::new();
    let mut steps = BTreeSet::new();
    let mut edges = BTreeSet::new();
    for (id, value) in &program.values {
        if *id != value.id {
            return Err(error("value ID disagrees with definition"));
        }
    }
    for segment in &program.segments {
        if !segments.insert(segment.id) {
            return Err(error("duplicate segment"));
        }
        for parameter in &segment.parameters {
            if !program.values.contains_key(parameter) {
                return Err(error("undefined parameter"));
            }
        }
        for unit in &segment.units {
            if !steps.insert(unit.id)
                || !units.contains_key(&unit.id)
                || !program.source_map.contains_key(&unit.source)
            {
                return Err(error("duplicate phase or missing source/definition coverage"));
            }
            for input in &unit.semantic_inputs {
                if !program.values.contains_key(input) {
                    return Err(error("undefined semantic input"));
                }
            }
            for atom in unit.stack_inputs.iter().chain(unit.outputs.iter()).chain(
                unit.stored_inputs.iter().filter_map(|input| match &input.value {
                    StoredValue::Atom(atom) => Some(atom),
                    _ => None,
                }),
            ) {
                validate_atom(program, *atom)?;
            }
            for requirement in &unit.scratch {
                requirement.validate()?;
                let wrong_owner = match requirement.lifetime {
                    ScratchLifetime::Unit(owner) => {
                        owner != unit.id || requirement.uses != [unit.id]
                    }
                    ScratchLifetime::Spanning => false,
                };
                if !requirement.uses.contains(&unit.id) || wrong_owner {
                    return Err(error("scratch ownership does not match consuming unit"));
                }
            }
            for effect in &unit.scratch_effects {
                let (object, offset, width) = match effect {
                    ScratchEffect::Read(fact) | ScratchEffect::Write(fact) => {
                        (fact.object, fact.offset, 1)
                    }
                    ScratchEffect::Borrow {
                        object,
                        offset,
                        size,
                        ..
                    }
                    | ScratchEffect::PossibleWrite {
                        object,
                        offset,
                        size,
                    } => (*object, *offset, *size),
                };
                let requirement = unit
                    .scratch
                    .iter()
                    .find(|requirement| requirement.id == object)
                    .ok_or_else(|| error("scratch effect references undeclared object"))?;
                if width == 0 || offset.checked_add(width).is_none_or(|end| end > requirement.size)
                {
                    return Err(error("scratch effect exceeds declared object"));
                }
            }
            for (output, input) in units[&unit.id].aliases.pairs() {
                if !unit.outputs.contains(output)
                    || !unit.stack_inputs.contains(input)
                        && !unit
                            .stored_inputs
                            .iter()
                            .any(|stored| stored.value == StoredValue::Atom(*input))
                {
                    return Err(error(
                        "registered publication relation references undeclared input/output",
                    ));
                }
            }
        }
    }
    if steps.len() != units.len() {
        return Err(error("orphan unit definition"));
    }
    for edge in &program.edges {
        if !edges.insert(edge.id)
            || !segments.contains(&edge.source)
            || edge.destination.is_some_and(|id| !segments.contains(&id))
        {
            return Err(error("invalid transfer endpoint"));
        }
        let mut destinations = BTreeSet::new();
        for binding in &edge.bindings {
            if !destinations.insert(binding.destination)
                || !program.values.contains_key(&binding.source)
            {
                return Err(error("duplicate or undefined edge binding"));
            }
            if let Some(destination) = edge.destination {
                let target =
                    program.segments.iter().find(|segment| segment.id == destination).unwrap();
                if !target.parameters.contains(&binding.destination)
                    || program.values[&binding.source].layout
                        != program.values[&binding.destination].layout
                {
                    return Err(error("edge binding disagrees with parameter layout"));
                }
            }
        }
        if let Some(destination) = edge.destination {
            let target = program.segments.iter().find(|segment| segment.id == destination).unwrap();
            if destinations.len() != target.parameters.len() {
                return Err(error("omitted edge parameter/capture"));
            }
        }
        if edge.destination.is_none() && !matches!(edge.kind, EdgeKind::Return | EdgeKind::Trap) {
            return Err(error("unsupported terminal edge"));
        }
    }
    let mut used_segments = BTreeSet::new();
    let mut used_edges = BTreeSet::new();
    walk_tree(&program.structure, &mut used_segments, &mut used_edges)?;
    if used_segments != segments || used_edges != edges {
        return Err(error("structured skeleton and graph disagree"));
    }
    validate_flow(program)?;
    super::reconstruction::validate(program)?;
    super::storage::validate(program)?;
    Ok(())
}

fn walk_tree(
    tree: &ControlTree,
    segments: &mut BTreeSet<SegmentId>,
    edges: &mut BTreeSet<EdgeId>,
) -> Result<(), VerificationError> {
    match tree {
        ControlTree::Segment(id) => {
            if !segments.insert(*id) {
                return Err(error("segment occurs twice in skeleton"));
            }
        }
        ControlTree::Edge(id) => {
            if !edges.insert(*id) {
                return Err(error("edge occurs twice in skeleton"));
            }
        }
        ControlTree::Sequence(children) => {
            for child in children {
                walk_tree(child, segments, edges)?;
            }
        }
        ControlTree::If {
            on_true,
            then_body,
            on_false,
            else_body,
            ..
        } => {
            if !edges.insert(*on_true) || !edges.insert(*on_false) {
                return Err(error("duplicate branch edge"));
            }
            walk_tree(then_body, segments, edges)?;
            walk_tree(else_body, segments, edges)?;
        }
        ControlTree::Loop {
            header,
            body,
            backedges,
            on_true,
            on_false,
            ..
        } => {
            if !edges.insert(*on_true) || !edges.insert(*on_false) {
                return Err(error("duplicate loop condition edge"));
            }
            if !segments.insert(*header) {
                return Err(error("duplicate loop header"));
            }
            walk_tree(body, segments, edges)?;
            for edge in backedges {
                if !edges.contains(edge) {
                    return Err(error("missing backedge in loop body"));
                }
            }
        }
    }
    Ok(())
}

/// Reject atoms whose value or lane is absent from the selected value table.
pub(super) fn validate_atom(
    program: &SelectedProgram,
    atom: Atom,
) -> Result<(), VerificationError> {
    if program
        .values
        .get(&atom.value)
        .is_none_or(|value| atom.lane >= value.layout.lanes())
    {
        return Err(error("undefined raw atom"));
    }
    Ok(())
}

/// Validate executable fallthrough as well as node coverage. A marker for a logical
/// return cannot jump over later MASM, and loop edges must reach their actual header.
fn validate_flow(program: &SelectedProgram) -> Result<(), VerificationError> {
    #[derive(Clone)]
    struct Frontier {
        segment: Option<SegmentId>,
        pending: Vec<EdgeId>,
        entry: bool,
    }
    struct Outcome {
        normal: Option<Frontier>,
        returns: bool,
    }
    fn edge(
        program: &SelectedProgram,
        id: EdgeId,
        frontier: &Frontier,
    ) -> Result<Outcome, VerificationError> {
        let edge = program
            .edges
            .iter()
            .find(|edge| edge.id == id)
            .ok_or_else(|| error("missing structured edge"))?;
        if frontier.segment != Some(edge.source) || !frontier.pending.is_empty() {
            return Err(error("structured edge has wrong source segment"));
        }
        Ok(Outcome {
            normal: edge.destination.map(|_| Frontier {
                segment: None,
                pending: alloc::vec![id],
                entry: false,
            }),
            returns: edge.kind == EdgeKind::Return,
        })
    }
    fn alternatives(
        program: &SelectedProgram,
        on_true: EdgeId,
        on_false: EdgeId,
    ) -> Result<(), VerificationError> {
        for (id, kind) in [(on_true, EdgeKind::True), (on_false, EdgeKind::False)] {
            if program
                .edges
                .iter()
                .find(|edge| edge.id == id)
                .is_none_or(|edge| edge.kind != kind)
            {
                return Err(error("structured alternative disagrees with edge kind"));
            }
        }
        Ok(())
    }
    fn segment(
        program: &SelectedProgram,
        id: SegmentId,
        frontier: Frontier,
    ) -> Result<Frontier, VerificationError> {
        let established = if frontier.entry {
            program.segments.first().is_some_and(|entry| entry.id == id)
        } else {
            !frontier.pending.is_empty()
                && frontier.pending.iter().all(|edge| {
                    program
                        .edges
                        .iter()
                        .find(|item| item.id == *edge)
                        .is_some_and(|edge| edge.destination == Some(id))
                })
        };
        if frontier.segment.is_some() || !established {
            return Err(error("structured segment is not reached by its incoming graph edges"));
        }
        Ok(Frontier {
            segment: Some(id),
            pending: Vec::new(),
            entry: false,
        })
    }
    fn walk(
        program: &SelectedProgram,
        tree: &ControlTree,
        input: Outcome,
    ) -> Result<Outcome, VerificationError> {
        if let ControlTree::Sequence(children) = tree {
            let mut result = input;
            for child in children {
                result = walk(program, child, result)?;
            }
            return Ok(result);
        }
        let frontier = input
            .normal
            .ok_or_else(|| error("executable code follows a terminal selected path"))?;
        match tree {
            ControlTree::Segment(id) => Ok(Outcome {
                normal: Some(segment(program, *id, frontier)?),
                returns: input.returns,
            }),
            ControlTree::Edge(id) => {
                let mut result = edge(program, *id, &frontier)?;
                result.returns |= input.returns;
                Ok(result)
            }
            ControlTree::If {
                on_true,
                then_body,
                on_false,
                else_body,
                ..
            } => {
                alternatives(program, *on_true, *on_false)?;
                let a = walk(program, then_body, edge(program, *on_true, &frontier)?)?;
                let b = walk(program, else_body, edge(program, *on_false, &frontier)?)?;
                if (a.returns && b.normal.is_some()) || (b.returns && a.normal.is_some()) {
                    return Err(error(
                        "logical return would fall through into a normal continuation",
                    ));
                }
                let normal = match (a.normal, b.normal) {
                    (Some(mut a), Some(b)) => {
                        if a.segment.is_some()
                            || b.segment.is_some()
                            || a.pending.is_empty()
                            || b.pending.is_empty()
                        {
                            return Err(error("normal branch lacks outgoing transfer"));
                        }
                        let target = program
                            .edges
                            .iter()
                            .find(|edge| edge.id == a.pending[0])
                            .unwrap()
                            .destination;
                        if b.pending.iter().any(|id| {
                            program.edges.iter().find(|edge| edge.id == *id).unwrap().destination
                                != target
                        }) {
                            return Err(error("normal arms disagree on executable continuation"));
                        }
                        a.pending.extend(b.pending);
                        Some(a)
                    }
                    (a, None) | (None, a) => a,
                };
                Ok(Outcome {
                    normal,
                    returns: input.returns || a.returns || b.returns,
                })
            }
            ControlTree::Loop {
                header,
                on_true,
                on_false,
                body,
                backedges,
                ..
            } => {
                alternatives(program, *on_true, *on_false)?;
                let header = segment(program, *header, frontier)?;
                let after = walk(program, body, edge(program, *on_true, &header)?)?;
                if after.returns {
                    return Err(error("a logical return cannot bypass while flag consumption"));
                }
                if let Some(after) = after.normal
                    && (after.segment.is_some()
                        || after.pending.is_empty()
                        || after.pending.iter().any(|id| {
                            !backedges.contains(id)
                                || program.edges.iter().find(|edge| edge.id == *id).is_none_or(
                                    |edge| {
                                        edge.kind != EdgeKind::Backedge
                                            || edge.destination != header.segment
                                    },
                                )
                        }))
                {
                    return Err(error("loop body does not establish its declared backedges"));
                }
                edge(program, *on_false, &header)
            }
            ControlTree::Sequence(_) => unreachable!(),
        }
    }
    let outcome = walk(
        program,
        &program.structure,
        Outcome {
            normal: Some(Frontier {
                segment: None,
                pending: Vec::new(),
                entry: true,
            }),
            returns: false,
        },
    )?;
    if outcome.normal.is_some() {
        return Err(error("selected procedure lacks an explicit terminal transfer"));
    }
    Ok(())
}
