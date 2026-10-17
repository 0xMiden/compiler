use alloc::{collections::BTreeMap, vec::Vec};

use miden_core::Felt;
use midenc_hir::{OperationRef, ValueRef};

use super::*;

#[derive(Debug, Clone)]
pub(crate) enum ProcedureOrigin {
    Hir(OperationRef),
    Generated {
        definition: GeneratedDefinitionId,
        parameters: Vec<Felt>,
    },
}
#[derive(Debug, Clone)]
pub(crate) enum SourceOrigin {
    Hir(OperationRef),
    Generated {
        definition: GeneratedDefinitionId,
        stage: u32,
    },
}
#[derive(Debug, Clone)]
pub(crate) struct SourceRecord {
    pub origin: SourceOrigin,
    pub location: SourceLocation,
}
#[derive(Debug, Clone)]
pub(crate) enum ValueOrigin {
    Hir(ValueRef),
    Temporary { step: SourceStepId, index: u32 },
    Parameter { segment: SegmentId, index: u32 },
}
#[derive(Debug, Clone)]
pub(crate) struct SelectedValue {
    pub id: ValueId,
    pub origin: ValueOrigin,
    pub layout: ValueLayout,
    pub reconstruction: Option<Reconstruction>,
}
#[derive(Debug, Clone)]
pub(crate) enum Reconstruction {
    /// Literal lanes in the value layout's stack order.
    Literal(Vec<Felt>),
    /// One existing source atom for each output lane.
    Alias(Vec<Atom>),
}

/// Expected contents of a stored operand, independent of the slot chosen to hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredValue {
    Atom(Atom),
    Literal(Felt),
}

/// Named lowering input delivered through storage rather than the operand-stack prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoredInput {
    pub id: StoredInputId,
    pub value: StoredValue,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoweringEffects {
    pub memory: MemoryEffect,
    pub may_trap: bool,
    /// Whether the unit has a normal continuation on which its outputs must be published.
    pub normal_return: bool,
    pub call: Option<CallEffect>,
}

impl Default for LoweringEffects {
    fn default() -> Self {
        Self {
            memory: MemoryEffect::None,
            may_trap: false,
            normal_return: true,
            call: None,
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct SelectedLowering {
    pub id: SelectedStepId,
    pub kind: LoweringKind,
    pub source: SourceStepId,
    /// Complete semantic dependencies, including values absent from the raw stack prefix.
    pub semantic_inputs: Vec<ValueId>,
    /// Exact raw prefix required at entry, nearest stack top first.
    pub stack_inputs: Vec<Atom>,
    /// Inputs whose physical cells are chosen through lowering bindings.
    pub stored_inputs: Vec<StoredInput>,
    pub outputs: Vec<Atom>,
    pub effects: LoweringEffects,
    pub scratch: Vec<ScratchRequirement>,
    pub scratch_effects: Vec<ScratchEffect>,
}
#[derive(Debug, Clone)]
pub(crate) struct SelectedSegment {
    pub id: SegmentId,
    pub parameters: Vec<ValueId>,
    pub units: Vec<SelectedLowering>,
}

/// Semantic role of a transfer in the structured control graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    Forward,
    True,
    False,
    Case(u32),
    Default,
    Backedge,
    Return,
    Trap,
}

/// Whole-value renaming on an edge. All bindings read the predecessor state simultaneously,
/// so swaps and loop-carried permutations do not depend on binding order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParameterBinding {
    pub destination: ValueId,
    pub source: ValueId,
}
#[derive(Debug, Clone)]
pub(crate) struct TransferEdge {
    pub id: EdgeId,
    pub source: SegmentId,
    /// Absent for terminal return or trap transfers.
    pub destination: Option<SegmentId>,
    pub kind: EdgeKind,
    pub bindings: Vec<ParameterBinding>,
}
#[derive(Debug, Clone)]
pub(crate) enum ControlTree {
    /// Execute one straight-line segment.
    Segment(SegmentId),
    /// Execute children in order, subject to their terminal transfers.
    Sequence(Vec<ControlTree>),
    If {
        id: ControlId,
        on_true: EdgeId,
        then_body: alloc::boxed::Box<ControlTree>,
        on_false: EdgeId,
        else_body: alloc::boxed::Box<ControlTree>,
    },
    Loop {
        id: ControlId,
        header: SegmentId,
        body: alloc::boxed::Box<ControlTree>,
        backedges: Vec<EdgeId>,
    },
    Edge(EdgeId),
}
#[derive(Debug, Clone)]
pub(crate) struct SelectedProgram {
    pub origin: ProcedureOrigin,
    pub values: BTreeMap<ValueId, SelectedValue>,
    pub segments: Vec<SelectedSegment>,
    pub edges: Vec<TransferEdge>,
    pub structure: ControlTree,
    pub abi: ProcedureAbi,
    pub source_map: BTreeMap<SourceStepId, SourceRecord>,
}

/// Ownership of lanes beneath an entry contract's explicit semantic prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TailPolicy {
    /// Retain the opaque caller suffix beneath the owned prefix.
    PreserveSuffix,
    /// Entry belongs to a context procedure with a fixed VM argument window.
    ContextWindow,
    /// Entry owns the root VM window, with no ordinary caller suffix.
    RootWindow,
}
#[derive(Debug, Clone)]
pub(crate) struct EntryContract {
    pub stack_top_first: Vec<Atom>,
    /// Symbolic homes whose current contents must equal the named atoms.
    pub required_spill_slots: Vec<(SpillSlotId, Atom)>,
    /// Cell initialization and identity obligations at entry.
    pub required_scratch: Vec<ScratchCellFact>,
    /// Exact scratch pointer provenance; separate from scratch contents.
    pub address_facts: Vec<AddressFact>,
    /// Additional semantic equalities assumed locally and proved on each incoming edge.
    pub aliases: LaneAliases,
    pub tail: TailPolicy,
}
