//! Symbolic frame references, scratch obligations, and validated physical frame assignments.
use alloc::{collections::BTreeMap, vec::Vec};

use super::{
    Atom, ContentId, DiagnosticCategory, PlanError, ScratchId, SelectedStepId, SpillSlotId, ValueId,
};

/// Largest word-aligned frame extent representable by the target's local-frame operand.
pub(crate) const MAX_FRAME_ELEMENTS: u32 =
    (u16::MAX as u32 / miden_core::WORD_SIZE as u32) * miden_core::WORD_SIZE as u32;

/// Source-owned prefix of the local frame, preserved exactly by compiler storage assignment.
/// Offsets and extent count field elements, not bytes; offsets retain source-local index order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SourceLocalLayout {
    pub offsets: Vec<u32>,
    pub extent: u32,
}

/// Reservation scope controlling whether scratch ranges may share physical frame space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScratchLifetime {
    /// Reserved only while this unit executes; distinct units may share workspace.
    Unit(SelectedStepId),
    /// Reserved across units and selected segment boundaries.
    Spanning,
}

/// Storage domain requested by a scratch object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddressSpace {
    ProcedureLocal,
}
/// Fixed scratch descriptor declared by selection and repeated consistently by all uses.
/// Size, alignment, and object-relative offsets are measured in field elements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScratchRequirement {
    pub id: ScratchId,
    pub size: u32,
    pub alignment: u32,
    pub contiguous: bool,
    pub address_space: AddressSpace,
    pub lifetime: ScratchLifetime,
    /// Complete set of units declaring this object; source checking verifies exact coverage.
    pub uses: Vec<SelectedStepId>,
}

impl ScratchRequirement {
    /// Check nonzero size and power-of-two alignment. Whole-program use consistency
    /// and physical overlap are checked separately by selection and final frame validation.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.size == 0 || !self.alignment.is_power_of_two() {
            return Err(frame_error("scratch needs a nonzero size and power-of-two alignment"));
        }
        Ok(())
    }
}

/// Physical frame range assigned to a scratch object, retaining its reservation lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScratchAllocation {
    pub offset: u32,
    pub size: u32,
    pub alignment: u32,
    pub lifetime: ScratchLifetime,
}

/// Symbolic cell reference resolved only after frame assignment. Scratch offsets are object-relative;
/// source-local indices and spill identities are looked up in their respective domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameRef {
    SourceLocal(u32),
    Spill(SpillSlotId),
    Scratch { object: ScratchId, offset: u32 },
}
/// Strength of a scratch-cell obligation, from definite initialization to exact identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContentRequirement {
    /// Some value was definitely written, without requiring a particular identity.
    Initialized,
    /// Cell equals the current token of this semantic lane.
    Atom(Atom),
    /// Cell equals a separately defined scratch-content token.
    Content(ContentId),
}
/// One object-relative scratch cell and the contents required or established there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScratchCellFact {
    pub object: ScratchId,
    pub offset: u32,
    pub content: ContentRequirement,
}
/// Claimed exact association between a semantic pointer and a scratch range.
/// Replay proves its address and accessible extent independently of any cell initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddressFact {
    pub pointer: ValueId,
    pub object: ScratchId,
    pub offset: u32,
    pub accessible: u32,
}

/// Selected scratch permissions and content obligations for one unit. These do not authorize
/// source-memory effects or allow compiler addresses to escape the declared borrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScratchEffect {
    /// Require the cell fact on entry to the unit.
    Read(ScratchCellFact),
    /// Establish the cell fact on normal completion.
    Write(ScratchCellFact),
    /// Allow mutation of a range without proving that previously empty cells become initialized.
    PossibleWrite {
        object: ScratchId,
        offset: u32,
        size: u32,
    },
    /// Expose a bounded range to a call with these read/write permissions.
    /// A definite write establishes initialization; retaining a compiler pointer is rejected.
    Borrow {
        object: ScratchId,
        offset: u32,
        size: u32,
        read: bool,
        write: bool,
        definite_write: bool,
        retained: bool,
    },
}

/// Element contributions to an assigned frame, including alignment padding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrameResources {
    pub source_locals: u64,
    pub scalar_colors: u64,
    pub spanning_scratch: u64,
    pub workspace: u64,
    pub padding: u64,
}

/// Shared frame range reserved for unit-local scratch, with element-based size and alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameWorkspace {
    pub offset: u32,
    pub size: u32,
    pub alignment: u32,
}

/// Assigned source, spill, and scratch locations, validated before target operand narrowing.
/// Spills may share cells; scratch may overlap only for distinct unit lifetimes.
/// Resource accounting is attached separately and does not grant access to additional cells.
#[derive(Debug, Clone)]
pub(crate) struct FinalFrameLayout {
    source: SourceLocalLayout,
    spills: BTreeMap<SpillSlotId, u32>,
    scratch: BTreeMap<ScratchId, ScratchAllocation>,
    aligned_size: u32,
    resources: FrameResources,
    workspace: FrameWorkspace,
}

impl FinalFrameLayout {
    /// Validate aligned target capacity and storage separation. Reject scratch overlap
    /// with source locals, spills, or simultaneously reserved scratch objects.
    pub fn new(
        source: SourceLocalLayout,
        spills: BTreeMap<SpillSlotId, u32>,
        scratch: BTreeMap<ScratchId, ScratchAllocation>,
        aligned_size: u32,
    ) -> Result<Self, PlanError> {
        if aligned_size > MAX_FRAME_ELEMENTS
            || !aligned_size.is_multiple_of(miden_core::WORD_SIZE as u32)
            || source.extent > aligned_size
        {
            return Err(frame_error("frame exceeds the aligned target capacity"));
        }
        if source.offsets.iter().any(|offset| *offset >= source.extent) {
            return Err(frame_error("source local lies outside its declared extent"));
        }
        for offset in spills.values() {
            if *offset < source.extent || *offset >= aligned_size {
                return Err(frame_error("spill cell outside compiler frame range"));
            }
        }
        for (id, allocation) in &scratch {
            if allocation.size == 0
                || !allocation.alignment.is_power_of_two()
                || !allocation.offset.is_multiple_of(allocation.alignment)
            {
                return Err(frame_error("invalid scratch alignment or size"));
            }
            let end = allocation
                .offset
                .checked_add(allocation.size)
                .ok_or_else(|| frame_error("scratch range overflow"))?;
            if allocation.offset < source.extent
                || end > aligned_size
                || spills.values().any(|offset| (allocation.offset..end).contains(offset))
            {
                return Err(frame_error(
                    "scratch overlaps source locals, spill cells, or frame boundary",
                ));
            }
            for (other_id, other) in &scratch {
                if other_id >= id {
                    continue;
                }
                let shared_workspace = matches!((allocation.lifetime, other.lifetime), (ScratchLifetime::Unit(a), ScratchLifetime::Unit(b)) if a != b);
                let other_end = other
                    .offset
                    .checked_add(other.size)
                    .ok_or_else(|| frame_error("scratch range overflow"))?;
                if !shared_workspace && allocation.offset < other_end && other.offset < end {
                    return Err(frame_error("simultaneously reserved scratch ranges overlap"));
                }
            }
        }
        Ok(Self {
            source,
            spills,
            scratch,
            aligned_size,
            resources: FrameResources::default(),
            workspace: FrameWorkspace {
                offset: aligned_size,
                size: 0,
                alignment: 1,
            },
        })
    }

    /// Accounting supplied by frame assignment, including its alignment padding.
    pub fn resources(&self) -> &FrameResources {
        &self.resources
    }

    /// Assignment's shared unit-local workspace descriptor.
    pub fn workspace(&self) -> &FrameWorkspace {
        &self.workspace
    }

    /// Attach assignment accounting without changing or revalidating any cell locations.
    pub fn with_resources(mut self, resources: FrameResources, workspace: FrameWorkspace) -> Self {
        self.resources = resources;
        self.workspace = workspace;
        self
    }

    /// Original source-owned prefix, used to check agreement with sealed source evidence.
    pub fn source(&self) -> &SourceLocalLayout {
        &self.source
    }

    /// Total frame element count, including trailing alignment padding.
    pub fn aligned_size(&self) -> u32 {
        self.aligned_size
    }

    /// Symbolic spill-to-cell assignment; multiple slots may use the same physical cell.
    pub fn spills(&self) -> &BTreeMap<SpillSlotId, u32> {
        &self.spills
    }

    /// Assigned object ranges used for provenance and bounds checks.
    pub fn scratch(&self) -> &BTreeMap<ScratchId, ScratchAllocation> {
        &self.scratch
    }

    /// Resolve one symbolic cell to a target local operand. Reject unknown objects,
    /// out-of-object scratch offsets, and operands that cannot fit the target encoding.
    pub fn resolve(&self, reference: FrameRef) -> Result<u16, PlanError> {
        let offset = match reference {
            FrameRef::SourceLocal(index) => *self
                .source
                .offsets
                .get(index as usize)
                .ok_or_else(|| frame_error("unknown source local"))?,
            FrameRef::Spill(slot) => {
                *self.spills.get(&slot).ok_or_else(|| frame_error("unassigned spill slot"))?
            }
            FrameRef::Scratch { object, offset } => {
                let allocation = self
                    .scratch
                    .get(&object)
                    .ok_or_else(|| frame_error("unassigned scratch object"))?;
                if offset >= allocation.size {
                    return Err(frame_error("scratch offset outside object"));
                }
                allocation
                    .offset
                    .checked_add(offset)
                    .ok_or_else(|| frame_error("scratch offset overflow"))?
            }
        };
        u16::try_from(offset).map_err(|_| frame_error("frame operand overflow"))
    }
}

fn frame_error(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::FrameCapacityExceeded, message)
}
