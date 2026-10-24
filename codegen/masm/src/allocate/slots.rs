//! Assign physical frame offsets after all symbolic actions and contracts are frozen.
//!
//! Backward stored-copy demand permits scalar slots with disjoint lifetimes to share an
//! element. Stored inputs remain simultaneously reserved across an opaque lowering.
//! Source locals retain their prefix, spanning scratch gets separate aligned ranges,
//! and unit-local scratch shares a workspace across different units. All sizes and offsets
//! use field elements; wide checked arithmetic precedes target-capacity checks and narrowing.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use super::state::AllocationError;
use crate::plan::{
    actions::{PlanAction, ProcedurePlan},
    *,
};

/// Dense set of symbolic slot indices in one storage graph, not physical frame offsets.
#[derive(Clone, PartialEq, Eq)]
struct Slots(Vec<u64>);
impl Slots {
    fn empty(count: usize) -> Self {
        Self(vec![0; count.div_ceil(64)])
    }

    fn insert(&mut self, index: usize) {
        self.0[index / 64] |= 1 << (index % 64);
    }

    fn remove(&mut self, index: usize) {
        self.0[index / 64] &= !(1 << (index % 64));
    }

    fn union(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= b;
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(word, bits)| {
            let mut bits = *bits;
            core::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                Some(word * 64 + bit)
            })
        })
    }
}

/// One segment-action or edge-action block in the completed physical plan.
struct Block<'a> {
    actions: &'a [PlanAction],
    /// Entry-contract cells that must remain distinct even without a later explicit load.
    required: Slots,
    successors: Vec<usize>,
    predecessors: Vec<usize>,
}

/// Symbolic storage graph retaining edge repairs as distinct control-flow blocks.
struct StorageGraph<'a> {
    plan: &'a ProcedurePlan,
    /// Sorted unique slot IDs, giving stable indices and coloring tie breaks.
    slots: Vec<SpillSlotId>,
    index: BTreeMap<SpillSlotId, usize>,
    blocks: Vec<Block<'a>>,
}
impl<'a> StorageGraph<'a> {
    /// Index declared cells and segment/edge actions, rejecting incomplete graph contracts.
    fn new(plan: &'a ProcedurePlan) -> Result<Self, PlanError> {
        let slots: Vec<_> =
            plan.spill_slots.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
        if slots.len() != plan.spill_slots.len() {
            return Err(storage_error("duplicate scalar storage request"));
        }
        let index: BTreeMap<_, _> =
            slots.iter().copied().enumerate().map(|(i, s)| (s, i)).collect();
        let mut blocks = Vec::new();
        let mut segments = BTreeMap::new();
        for segment in &plan.selected.segments {
            segments.insert(segment.id, blocks.len());
            let contract = plan
                .contracts
                .get(&segment.id)
                .ok_or_else(|| storage_error("missing storage entry contract"))?;
            let mut required = Slots::empty(slots.len());
            for (slot, _) in &contract.required_spill_slots {
                required.insert(
                    *index
                        .get(slot)
                        .ok_or_else(|| storage_error("contract uses undeclared scalar slot"))?,
                );
            }
            blocks.push(Block {
                actions: plan
                    .segment_actions
                    .get(&segment.id)
                    .ok_or_else(|| storage_error("missing segment actions"))?,
                required,
                successors: Vec::new(),
                predecessors: Vec::new(),
            });
        }
        for edge in &plan.selected.edges {
            let id = blocks.len();
            let source = *segments
                .get(&edge.source)
                .ok_or_else(|| storage_error("unknown storage edge source"))?;
            blocks[source].successors.push(id);
            let successors = edge
                .destination
                .map(|destination| {
                    segments
                        .get(&destination)
                        .copied()
                        .ok_or_else(|| storage_error("unknown storage edge target"))
                })
                .transpose()?
                .into_iter()
                .collect();
            blocks.push(Block {
                actions: plan.edge_actions.get(&edge.id).map_or(&[], Vec::as_slice),
                required: Slots::empty(slots.len()),
                successors,
                predecessors: vec![],
            });
        }
        for id in 0..blocks.len() {
            for successor in blocks[id].successors.clone() {
                blocks[successor].predecessors.push(id);
            }
        }
        Ok(Self {
            plan,
            slots,
            index,
            blocks,
        })
    }

    /// Propagate stored-copy demand backward through one action.
    /// Loads, semantic boundary reads and opaque inputs require old contents; stores replace them.
    fn transfer(&self, action: &PlanAction, live: &mut Slots) -> Result<(), PlanError> {
        let index = |slot| {
            self.index
                .get(&slot)
                .copied()
                .ok_or_else(|| storage_error("action uses undeclared scalar slot"))
        };
        match *action {
            PlanAction::LoadLane(slot) => live.insert(index(slot)?),
            PlanAction::StoreLane(slot) => live.remove(index(slot)?),
            PlanAction::RunLowering { step } => {
                let bindings = self
                    .plan
                    .lowering_bindings
                    .get(&step)
                    .ok_or_else(|| storage_error("missing stored-input bindings"))?;
                for slot in self.plan.semantic_storage_reads.get(&step).into_iter().flatten() {
                    live.insert(index(*slot)?);
                }
                for slot in bindings.stored_inputs.values() {
                    live.insert(index(*slot)?);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Solve stored-copy demand, build simultaneous-use interference, and greedily color.
    ///
    /// Even a dead store interferes with live copies it could clobber. All stored inputs
    /// and live-through copies interfere across a complete opaque expansion. Coloring
    /// visits highest-degree slots first, then slot ID, choosing the least available color.
    fn color(&self) -> Result<Vec<usize>, PlanError> {
        let count = self.slots.len();
        let mut entries = vec![Slots::empty(count); self.blocks.len()];
        let mut pending: BTreeSet<_> = (0..self.blocks.len()).collect();
        while let Some(id) = pending.pop_first() {
            let block = &self.blocks[id];
            let mut live = Slots::empty(count);
            for successor in &block.successors {
                live.union(&entries[*successor]);
            }
            for action in block.actions.iter().rev() {
                self.transfer(action, &mut live)?;
            }
            live.union(&block.required);
            if live != entries[id] {
                entries[id] = live;
                pending.extend(block.predecessors.iter().copied());
            }
        }
        let mut interference = vec![Slots::empty(count); count];
        let mut clique = |live: &Slots| {
            for a in live.iter() {
                for b in live.iter() {
                    if a != b {
                        interference[a].insert(b);
                    }
                }
            }
        };
        for block in &self.blocks {
            let mut live = Slots::empty(count);
            for successor in &block.successors {
                live.union(&entries[*successor]);
            }
            clique(&live);
            for action in block.actions.iter().rev() {
                // Even a store whose new contents are dead can clobber a live copy.
                if let PlanAction::StoreLane(slot) = action {
                    let mut simultaneous = live.clone();
                    simultaneous.insert(self.index[slot]);
                    clique(&simultaneous);
                }
                self.transfer(action, &mut live)?;
                // RunLowering's incoming clique includes all bound cells and live-through
                // copies, reserving them for the complete opaque expansion.
                clique(&live);
            }
            live.union(&block.required);
            clique(&live);
        }
        let mut order: Vec<_> = (0..count).collect();
        order.sort_by_key(|id| {
            (core::cmp::Reverse(interference[*id].iter().count()), self.slots[*id])
        });
        let mut colors = vec![usize::MAX; count];
        for id in order {
            let forbidden: BTreeSet<_> =
                interference[id].iter().map(|other| colors[other]).collect();
            colors[id] = (0..).find(|color| !forbidden.contains(color)).unwrap();
        }
        Ok(colors)
    }
}

/// Assign element offsets to the completed plan's symbolic storage without revising actions.
///
/// Scalar colors follow the source-local extent. Spanning scratch follows in scratch-ID
/// order; unit-local objects are packed per unit and overlap only between different units.
/// Their shared workspace follows spanning scratch, and the whole frame is word-aligned.
///
/// Malformed storage references or missing bindings are rejected. Alignment, arithmetic,
/// and `MAX_FRAME_ELEMENTS` are checked before any address is narrowed; capacity failures
/// report source, scalar, spanning, workspace, and padding contributions when available.
pub(crate) fn assign_frame(
    plan: &ProcedurePlan,
    source: &SourceLocalLayout,
) -> Result<FinalFrameLayout, AllocationError> {
    let graph = StorageGraph::new(plan)?;
    let colors = graph.color()?;
    let scalar_colors = colors.iter().max().map_or(Ok(0), |color| {
        u64::try_from(*color)
            .ok()
            .and_then(|color| color.checked_add(1))
            .ok_or_else(|| error("scalar color count overflow"))
    })?;
    let mut layout = Layout {
        resources: FrameResources {
            source_locals: u64::from(source.extent),
            scalar_colors,
            ..Default::default()
        },
        cursor: 0,
    };
    layout.cursor = layout.add(u64::from(source.extent), scalar_colors)?;
    let requirements: BTreeMap<_, _> = plan
        .selected
        .segments
        .iter()
        .flat_map(|s| &s.units)
        .flat_map(|u| &u.scratch)
        .map(|r| (r.id, r))
        .collect();
    let mut assigned = Vec::new();
    let mut units = BTreeMap::<SelectedStepId, Vec<&ScratchRequirement>>::new();
    for requirement in requirements.values() {
        requirement.validate()?;
        match requirement.lifetime {
            ScratchLifetime::Spanning => {
                layout.pad(requirement.alignment)?;
                assigned.push((*requirement, layout.cursor));
                layout.resources.spanning_scratch =
                    layout.add(layout.resources.spanning_scratch, u64::from(requirement.size))?;
                layout.cursor = layout.add(layout.cursor, u64::from(requirement.size))?;
            }
            ScratchLifetime::Unit(owner) => units.entry(owner).or_default().push(requirement),
        }
    }
    let mut alignment = 1;
    let mut relative = Vec::new();
    for objects in units.values() {
        let mut size = 0;
        for requirement in objects {
            size = layout.align(size, requirement.alignment)?;
            alignment = alignment.max(requirement.alignment);
            relative.push((*requirement, size));
            size = layout.add(size, u64::from(requirement.size))?;
        }
        layout.resources.workspace = layout.resources.workspace.max(size);
    }
    layout.pad(alignment)?;
    let base = layout.cursor;
    for (requirement, offset) in relative {
        assigned.push((requirement, layout.add(base, offset)?));
    }
    layout.cursor = layout.add(base, layout.resources.workspace)?;
    layout.pad(miden_core::WORD_SIZE as u32)?;
    if layout.cursor > u64::from(MAX_FRAME_ELEMENTS) {
        return Err(capacity(&layout.resources).into());
    }
    // All arithmetic and the complete target bound are checked before narrowing any address.
    let narrow = |offset| u32::try_from(offset).map_err(|_| capacity(&layout.resources));
    let spills = graph
        .slots
        .iter()
        .zip(&colors)
        .map(|(id, color)| {
            let color = u64::try_from(*color).map_err(|_| capacity(&layout.resources))?;
            Ok((*id, narrow(layout.add(u64::from(source.extent), color)?)?))
        })
        .collect::<Result<BTreeMap<_, _>, PlanError>>()?;
    let scratch = assigned
        .into_iter()
        .map(|(requirement, offset)| {
            Ok((
                requirement.id,
                ScratchAllocation {
                    offset: narrow(offset)?,
                    size: requirement.size,
                    alignment: requirement.alignment,
                    lifetime: requirement.lifetime,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, PlanError>>()?;
    let workspace = FrameWorkspace {
        offset: narrow(base)?,
        size: narrow(layout.resources.workspace)?,
        alignment,
    };
    Ok(FinalFrameLayout::new(source.clone(), spills, scratch, narrow(layout.cursor)?)?
        .with_resources(layout.resources, workspace))
}

/// Checked frame cursor and capacity accounting, both measured in field elements.
struct Layout {
    cursor: u64,
    resources: FrameResources,
}
impl Layout {
    /// Add wide element counts, preserving the current resource breakdown on overflow.
    fn add(&self, a: u64, b: u64) -> Result<u64, PlanError> {
        a.checked_add(b).ok_or_else(|| capacity(&self.resources))
    }

    /// Round an element offset upward to a positive power-of-two element alignment.
    fn align(&self, value: u64, alignment: u32) -> Result<u64, PlanError> {
        if !alignment.is_power_of_two() {
            return Err(error("alignment must be a positive power of two"));
        }
        let alignment = u64::from(alignment);
        Ok(self.add(value, alignment - 1)? & !(alignment - 1))
    }

    /// Align the cursor while attributing the inserted gap to frame padding.
    fn pad(&mut self, alignment: u32) -> Result<(), PlanError> {
        let aligned = self.align(self.cursor, alignment)?;
        self.resources.padding = self.add(self.resources.padding, aligned - self.cursor)?;
        self.cursor = aligned;
        Ok(())
    }
}
/// Explain capacity failure with the resource contributions accumulated before failure.
fn capacity(resources: &FrameResources) -> PlanError {
    PlanError::new(
        DiagnosticCategory::FrameCapacityExceeded,
        alloc::format!(
            "frame exceeds {MAX_FRAME_ELEMENTS} elements: source={} scalar={} spanning={} \
             workspace={} padding={}",
            resources.source_locals,
            resources.scalar_colors,
            resources.spanning_scratch,
            resources.workspace,
            resources.padding
        ),
    )
}
fn error(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::FrameCapacityExceeded, message)
}

/// Storage graph errors describe invalid symbolic reads, rather than frame exhaustion.
fn storage_error(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::InvalidSpillSlotRead, message)
}
