//! Semantic lowering graph and control structure supplied to allocation after source checking.
use alloc::{collections::BTreeMap, vec::Vec};

use miden_core::Felt;
use midenc_hir::{OperationRef, ValueRef};

use super::*;

/// Source identity used to reconstruct the entire selected procedure independently of allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcedureOrigin {
    Hir(OperationRef),
    Generated {
        definition: GeneratedDefinitionId,
        parameters: Vec<Felt>,
    },
}
/// Origin of one semantic stage, retaining either its HIR operation or generated definition stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceOrigin {
    Hir(OperationRef),
    Generated {
        definition: GeneratedDefinitionId,
        stage: u32,
    },
}
/// Semantic provenance and diagnostic location for a selected source step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRecord {
    pub origin: SourceOrigin,
    pub location: SourceLocation,
}
/// Definition site of a semantic value, including generated intermediates and segment parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValueOrigin {
    Hir(ValueRef),
    Temporary { step: SourceStepId, index: u32 },
    Parameter { segment: SegmentId, index: u32 },
}
/// Semantic value and its raw lane layout. Optional reconstruction is a source claim that
/// replay must prove at publication to validate the allocator's use of that recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedValue {
    pub id: ValueId,
    pub origin: ValueOrigin,
    pub layout: ValueLayout,
    /// Recoverable identity, distinct from the value's current physical availability.
    pub reconstruction: Option<Reconstruction>,
}
/// Pure, lane-ordered recipe for recovering a value without preserving a physical copy.
/// Alias dependencies must be acyclic and precede the value definition.
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// Source-memory permissions for an entire unit; individual instructions retain exact effects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SourceMemoryEffects {
    pub read: bool,
    pub write: bool,
}
impl SourceMemoryEffects {
    pub const READ: Self = Self {
        read: true,
        write: false,
    };
    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
    };
    pub const WRITE: Self = Self {
        read: false,
        write: true,
    };

    /// Check the requested access direction; this does not authorize compiler-frame accesses.
    pub fn permits(self, write: bool) -> bool {
        if write { self.write } else { self.read }
    }
}

/// Source-authorized effects of one unit. Replay checks actual instructions against these
/// permissions; a default unit has no source-memory effects, cannot trap, and returns normally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoweringEffects {
    pub memory: SourceMemoryEffects,
    pub may_trap: bool,
    /// Whether the unit has a normal continuation on which its outputs must be published.
    pub normal_return: bool,
    /// Optional invocation summary, checked against every registered call site's ABI effect.
    /// Target, arguments, and results remain source obligations in `UnitEvidence::calls`;
    /// this summary grants no authority for them. A replayed invocation requires `Some`.
    pub call: Option<CallEffect>,
}
impl Default for LoweringEffects {
    fn default() -> Self {
        Self {
            memory: SourceMemoryEffects::default(),
            may_trap: false,
            normal_return: true,
            call: None,
        }
    }
}
/// One allocation boundary whose internal instruction sequence is owned by a lowering definition.
/// Allocation arranges the declared stack prefix and stored inputs, then preserves all other
/// live values across the unit. Source checking seals the semantic contract before allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Required reads, writes, and borrows of compiler-owned scratch.
    pub scratch_effects: Vec<ScratchEffect>,
}
/// Straight-line sequence of units sharing one entry contract. Parameters are established
/// simultaneously by incoming edges, including separately named scratch contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedSegment {
    pub id: SegmentId,
    pub parameters: Vec<ValueId>,
    pub content_parameters: Vec<ContentId>,
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
/// Simultaneous renaming of scratch-content identity at a segment boundary; no bytes move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentBinding {
    pub destination: ContentId,
    pub source: ContentId,
}
/// Selected control transfer and its simultaneous value/content parameter bindings.
/// Physical edge actions arrange predecessor contents for the destination contract;
/// simultaneous semantic renaming then gives those contents their destination names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferEdge {
    pub id: EdgeId,
    pub source: SegmentId,
    /// Absent for terminal return or trap transfers.
    pub destination: Option<SegmentId>,
    pub kind: EdgeKind,
    pub bindings: Vec<ParameterBinding>,
    pub content_bindings: Vec<ContentBinding>,
}
/// Executable structured ordering of the selected graph. Segment and edge identifiers must
/// agree with graph coverage; this tree also constrains the expanded control skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlTree {
    /// Execute one straight-line segment.
    Segment(SegmentId),
    /// Execute children in order, subject to their terminal transfers.
    Sequence(Vec<ControlTree>),
    /// Consume the selected predicate and execute the matching edge and arm.
    If {
        id: ControlId,
        predicate: Atom,
        on_true: EdgeId,
        then_body: alloc::boxed::Box<ControlTree>,
        on_false: EdgeId,
        else_body: alloc::boxed::Box<ControlTree>,
    },
    Loop {
        id: ControlId,
        condition: ControlId,
        on_true: EdgeId,
        on_false: EdgeId,
        header: SegmentId,
        predicate: Atom,
        body: alloc::boxed::Box<ControlTree>,
        backedges: Vec<EdgeId>,
    },
    /// Execute one transfer, including any physical edge actions.
    Edge(EdgeId),
}
/// Semantic program proposed by selection, before any allocation decisions.
/// The first segment is the procedure entry. Source checking compares this entire description
/// to independently reconstructed evidence and returns an immutable checked wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// Allocator-proposed entry hypothesis, proved on every actual incoming transfer.
/// Stack entries may repeat an atom, but each entry still requires a physical occurrence.
/// Scratch and address facts crossing a segment boundary must refer to spanning objects.
#[derive(Debug, Clone, PartialEq, Eq)]
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
