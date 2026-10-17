//! Separate identities for semantic values, lowering/control sites, and physical storage copies.
use core::fmt;

macro_rules! ids {
    ($($name:ident),* $(,)?) => {$(
        /// Identity within a selected procedure; the numeric value is not a physical location.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) struct $name(pub u32);
    )*};
}

// Separate identity domains prevent confusing semantic values, control nodes, lowering stages,
// storage objects, and physical occurrences even when their numeric indices coincide.
ids!(
    ValueId,
    SegmentId,
    EdgeId,
    SelectedStepId,
    SourceStepId,
    LoweringKind,
    StoredInputId,
    ScratchId,
    OccurrenceId,
    DebugLocationId,
    GeneratedDefinitionId,
    ContentId,
    ControlId
);

/// One raw lane of a semantic value, independent of its physical copies or storage.
/// Lane zero is nearest stack top, following `Type::to_raw_parts()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Atom {
    pub value: ValueId,
    pub lane: u8,
}

impl Atom {
    /// Name a lane; selection checking separately verifies that it exists in the value layout.
    pub const fn new(value: ValueId, lane: u8) -> Self {
        Self { value, lane }
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}", self.value.0, self.lane)
    }
}

/// Namespace for a spill slot: a lasting value home or a temporary for one unit or edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SlotOwner {
    Value(ValueId),
    Unit(SelectedStepId),
    Edge(EdgeId),
}

/// A semantic key, rather than an allocation counter: speculative trials cannot perturb IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SpillSlotId {
    /// Semantic scope that makes the local index unique.
    pub owner: SlotOwner,
    /// Lane for value homes; owner-local temporary index otherwise.
    pub index: u32,
}

impl SpillSlotId {
    /// Stable home for this lane, shared by all of its spill/reload actions.
    pub const fn canonical(atom: Atom) -> Self {
        Self {
            owner: SlotOwner::Value(atom.value),
            index: atom.lane as u32,
        }
    }

    /// Temporary used while arranging or executing one lowering unit.
    pub const fn temporary(step: SelectedStepId, index: u32) -> Self {
        Self {
            owner: SlotOwner::Unit(step),
            index,
        }
    }

    /// Temporary used while transferring values along one selected edge.
    pub const fn edge_temporary(edge: EdgeId, index: u32) -> Self {
        Self {
            owner: SlotOwner::Edge(edge),
            index,
        }
    }
}

impl fmt::Display for SpillSlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.owner {
            SlotOwner::Value(value) => write!(f, "v{}.{}", value.0, self.index),
            SlotOwner::Unit(step) => write!(f, "s{}.tmp{}", step.0, self.index),
            SlotOwner::Edge(edge) => write!(f, "e{}.tmp{}", edge.0, self.index),
        }
    }
}

/// A particular physical copy of an atom; duplicating a lane creates another occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Occurrence {
    pub id: OccurrenceId,
    pub atom: Atom,
}
