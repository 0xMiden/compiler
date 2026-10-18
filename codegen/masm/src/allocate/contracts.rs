//! Choose one physical entry contract before planning a segment body.
//!
//! Completed forward predecessors supply candidate representations and shared facts.
//! Candidates are checked by constructing their edge repairs and preparing the first unit;
//! their estimated raw-action cost determines the winner. Loop headers use repeated-work
//! demand and exclude forward-only equalities that changing parameters cannot preserve.
//! The chosen contract stays fixed when later backedges are repaired against it.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use super::{
    analysis::{AllocationAnalysis, LoopDemand, NextUseDistance},
    edges::repair_edge,
    schedule::{action_cost, prepare_unit},
    state::*,
};
use crate::{plan::*, verify::CheckedSelectedProgram};

/// An immutable predecessor state after control-predicate consumption, with its transfer.
pub(super) struct Incoming<'a> {
    /// Maps destination parameter and content names back to this state's names.
    pub edge: &'a TransferEdge,
    /// Physical facts from the completed forward predecessor body.
    pub state: &'a AllocationState,
}
/// The winning contract and complete private repairs for its forward predecessors.
pub(super) struct Choice {
    /// Required entry facts used to plan the body exactly once.
    pub contract: EntryContract,
    /// Repairs already scored for this contract, keyed by incoming edge identity.
    pub repairs: BTreeMap<EdgeId, Preparation>,
    #[cfg(test)]
    pub costs: Vec<usize>,
}

/// Substitute a destination parameter once, preserving the raw lane index.
fn source(edge: &TransferEdge, atom: Atom) -> Atom {
    Atom::new(
        edge.bindings
            .iter()
            .find(|binding| binding.destination == atom.value)
            .map_or(atom.value, |binding| binding.source),
        atom.lane,
    )
}
/// Translate scratch content demand to old source identities without changing its cell.
fn content(edge: &TransferEdge, required: ContentRequirement) -> ContentRequirement {
    match required {
        ContentRequirement::Atom(atom) => ContentRequirement::Atom(source(edge, atom)),
        ContentRequirement::Content(id) => ContentRequirement::Content(
            edge.content_bindings
                .iter()
                .find(|binding| binding.destination == id)
                .map_or(id, |binding| binding.source),
        ),
        other => other,
    }
}
/// Expand semantic demand in stable value-ID order and ascending lane order.
fn atoms(program: &CheckedSelectedProgram, live: &BTreeSet<ValueId>) -> Vec<Atom> {
    live.iter()
        .flat_map(|value| {
            (0..program.values[value].layout.lanes()).map(move |lane| Atom::new(*value, lane))
        })
        .collect()
}
/// Prove a destination-side lane equality using one predecessor's old source identities.
fn equal(
    incoming: &Incoming<'_>,
    a: Atom,
    b: Atom,
    context: &AllocationContext<'_>,
) -> Result<bool, AllocationError> {
    context.same(
        incoming.state,
        StoredValue::Atom(source(incoming.edge, a)),
        StoredValue::Atom(source(incoming.edge, b)),
    )
}

/// A representation is renamed only through checked old-source equality. Duplicate
/// destination names get explicit aliases; they do not duplicate physical occurrences.
fn rename(
    incoming: &Incoming<'_>,
    atom: Atom,
    live_atoms: &[Atom],
    context: &AllocationContext<'_>,
) -> Result<Atom, AllocationError> {
    for candidate in live_atoms {
        if context.same(
            incoming.state,
            StoredValue::Atom(atom),
            StoredValue::Atom(source(incoming.edge, *candidate)),
        )? {
            return Ok(*candidate);
        }
    }
    Ok(atom)
}
/// Retain only equalities established on every forward edge.
/// Loop headers publish none: initial-entry equality alone does not prove cyclic equality.
fn aliases(
    incoming: &[Incoming<'_>],
    live_atoms: &[Atom],
    context: &AllocationContext<'_>,
    loop_header: bool,
) -> Result<LaneAliases, AllocationError> {
    if loop_header {
        return Ok(LaneAliases::default());
    }
    let mut pairs = vec![];
    for (index, b) in live_atoms.iter().enumerate() {
        for a in &live_atoms[..index] {
            if incoming
                .iter()
                .map(|edge| equal(edge, *a, *b, context))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .all(|same| same)
            {
                pairs.push((*a, *b));
                break;
            }
        }
    }
    Ok(LaneAliases::new(pairs))
}
/// Keep one common backing cell per eligible lane, preferring its canonical ID.
/// Every predecessor must establish the same source-substituted contents in that cell.
fn common_slots(
    incoming: &[Incoming<'_>],
    live_atoms: &[Atom],
    context: &AllocationContext<'_>,
) -> Result<BTreeMap<SpillSlotId, Atom>, AllocationError> {
    let mut slots = BTreeMap::new();
    for slot in incoming[0].state.slots.keys() {
        for atom in live_atoms {
            let mut common = true;
            for entry in incoming {
                common &= if let Some(contents) = entry.state.slots.get(slot) {
                    context.same(
                        entry.state,
                        *contents,
                        StoredValue::Atom(source(entry.edge, *atom)),
                    )?
                } else {
                    false
                };
            }
            if common {
                slots.insert(*slot, *atom);
                break;
            }
        }
    }
    let mut preferred = BTreeMap::<Atom, SpillSlotId>::new();
    for (slot, atom) in slots {
        preferred
            .entry(atom)
            .and_modify(|old| {
                if (slot != SpillSlotId::canonical(atom), slot)
                    < (*old != SpillSlotId::canonical(atom), *old)
                {
                    *old = slot;
                }
            })
            .or_insert(slot);
    }
    Ok(preferred.into_iter().map(|(atom, slot)| (slot, atom)).collect())
}
/// Preserve provenance only when every predecessor proves the same object, offset, and extent.
fn common_addresses(
    incoming: &[Incoming<'_>],
    live: &BTreeSet<ValueId>,
    context: &AllocationContext<'_>,
) -> Result<Vec<AddressFact>, AllocationError> {
    let mut result = vec![];
    for pointer in live {
        let original = source(incoming[0].edge, Atom::new(*pointer, 0));
        for fact in &incoming[0].state.addresses {
            if !context.same(
                incoming[0].state,
                StoredValue::Atom(original),
                StoredValue::Atom(Atom::new(fact.pointer, 0)),
            )? {
                continue;
            }
            let mut target = *fact;
            target.pointer = *pointer;
            let mut common = true;
            for entry in incoming {
                let required = source(entry.edge, Atom::new(*pointer, 0));
                let mut found = false;
                for actual in &entry.state.addresses {
                    if actual.object == target.object
                        && actual.offset == target.offset
                        && actual.accessible == target.accessible
                    {
                        found |= context.same(
                            entry.state,
                            StoredValue::Atom(required),
                            StoredValue::Atom(Atom::new(actual.pointer, 0)),
                        )?;
                    }
                }
                common &= found;
            }
            if common && !result.contains(&target) {
                result.push(target);
            }
        }
    }
    Ok(result)
}

/// Check that each demanded cell already has its required contents on every forward edge.
/// Unlike scalar spills, missing scratch contents cannot be manufactured by contract choice.
fn scratch(
    incoming: &[Incoming<'_>],
    demand: &[ScratchCellFact],
    context: &AllocationContext<'_>,
) -> Result<Vec<ScratchCellFact>, AllocationError> {
    for fact in demand {
        for entry in incoming {
            let required = content(entry.edge, fact.content);
            let mut found = false;
            for actual in &entry.state.scratch {
                if actual.object != fact.object || actual.offset != fact.offset {
                    continue;
                }
                found |= match (required, actual.content) {
                    (ContentRequirement::Initialized, _) => true,
                    (ContentRequirement::Atom(a), ContentRequirement::Atom(b)) => {
                        context.same(entry.state, StoredValue::Atom(a), StoredValue::Atom(b))?
                    }
                    (a, b) => a == b,
                };
            }
            if !found {
                return Err(invariant(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "incoming edge lacks required scratch contents",
                ));
            }
        }
    }
    Ok(demand.to_vec())
}
/// Find a value's complete contiguous raw layout within the top sixteen owned lanes.
/// Returns its top-first starting depth, or `None` for a split, buried, or absent layout.
fn complete_value_depth(
    incoming: &Incoming<'_>,
    value: ValueId,
    program: &CheckedSelectedProgram,
    context: &AllocationContext<'_>,
) -> Result<Option<usize>, AllocationError> {
    let width = usize::from(program.values[&value].layout.lanes());
    for start in 0..incoming.state.stack.len().min(16) {
        if start + width > 16 || start + width > incoming.state.stack.len() {
            continue;
        }
        let mut matches = true;
        for lane in 0..width {
            matches &= context.same(
                incoming.state,
                StoredValue::Atom(incoming.state.stack[start + lane].atom),
                StoredValue::Atom(source(incoming.edge, Atom::new(value, lane as u8))),
            )?;
        }
        if matches {
            return Ok(Some(start));
        }
    }
    Ok(None)
}
/// Pack whole values by rank into the top-sixteen entry window, skipping those that do not fit.
fn rank_layout(program: &CheckedSelectedProgram, ranked: &[ValueId]) -> Vec<Atom> {
    let mut stack = vec![];
    for value in ranked {
        let width = usize::from(program.values[value].layout.lanes());
        if stack.len() + width <= 16 {
            stack.extend((0..width).map(|lane| Atom::new(*value, lane as u8)));
        }
    }
    stack
}
/// Add backing requirements for live lanes absent from stack, aliases, or reconstruction.
/// Common cached cells are retained, but changing loop parameters use their own canonical
/// namespace so the header does not assume a previous dynamic iteration's cache exists.
fn required_slots(
    contract: &mut EntryContract,
    program: &CheckedSelectedProgram,
    segment: SegmentId,
    live_atoms: &[Atom],
    common: &BTreeMap<SpillSlotId, Atom>,
    context: &AllocationContext<'_>,
    header: bool,
) -> Result<(), AllocationError> {
    let mut state = AllocationState::at_entry(contract, segment, context)?;
    let parameters = &program.segments.iter().find(|s| s.id == segment).unwrap().parameters;
    contract
        .required_spill_slots
        .extend(common.iter().map(|(slot, atom)| (*slot, *atom)));
    state.slots = contract
        .required_spill_slots
        .iter()
        .map(|(slot, atom)| (*slot, StoredValue::Atom(*atom)))
        .collect();
    for atom in live_atoms {
        let value = StoredValue::Atom(*atom);
        let represented = state
            .stack
            .iter()
            .map(|o| StoredValue::Atom(o.atom))
            .chain(state.slots.values().copied())
            .map(|other| context.same(&state, value, other))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|same| same)
            || matches!(context.source(&state, *atom)?, StoredValue::Literal(_));
        if represented {
            continue;
        }
        let slot = if !header || !parameters.contains(&atom.value) {
            common
                .iter()
                .filter(|(_, a)| **a == *atom)
                .map(|(slot, _)| *slot)
                .min_by_key(|slot| (*slot != SpillSlotId::canonical(*atom), *slot))
                .unwrap_or_else(|| SpillSlotId::canonical(*atom))
        } else {
            // Unique selected definitions prevent this parameter's canonical namespace
            // from being requested by its forward predecessor bodies. Its incoming repair
            // creates the first request; enclosing fixed headers cannot bootstrap a cache
            // from an earlier dynamic instance absent on their initial forward entry.
            SpillSlotId::canonical(*atom)
        };
        contract.required_spill_slots.push((slot, *atom));
        state.slots.insert(slot, value);
    }
    contract.required_spill_slots.sort();
    contract.required_spill_slots.dedup();
    Ok(())
}
/// Check that a candidate represents every live lane through stack, slots, or literals.
fn recoverable(
    contract: &EntryContract,
    segment: SegmentId,
    live_atoms: &[Atom],
    context: &AllocationContext<'_>,
) -> Result<bool, AllocationError> {
    let state = AllocationState::at_entry(contract, segment, context)?;
    for atom in live_atoms {
        let mut found = matches!(context.source(&state, *atom)?, StoredValue::Literal(_));
        for value in state
            .stack
            .iter()
            .map(|o| StoredValue::Atom(o.atom))
            .chain(state.slots.values().copied())
        {
            found |= context.same(&state, StoredValue::Atom(*atom), value)?;
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Choose a deterministic, recoverable entry from completed forward predecessor states.
///
/// `incoming` must be nonempty and ordered by edge ID for stable inherited-layout ties.
/// Shared scratch and provenance facts must already hold; scalar repairs may materialize
/// missing representations. Ordinary joins compare demand-ranked, common-resident, and
/// inherited layouts unless predecessor agreement already provides a complete layout.
/// Loop headers use a single layout ranked by cyclic demand before ordinary next use.
///
/// Candidate scoring includes all forward repairs and the first unit's preparation,
/// weighted by incoming edge count. Equal costs retain the earlier candidate. Failed
/// private candidates are discarded; no viable contract produces `UnavailableValue`.
pub(super) fn choose(
    program: &CheckedSelectedProgram,
    analysis: &AllocationAnalysis,
    context: &AllocationContext<'_>,
    segment: SegmentId,
    incoming: &[Incoming<'_>],
    loop_demand: Option<&LoopDemand>,
) -> Result<Choice, AllocationError> {
    if incoming.is_empty() {
        return Err(invariant(
            DiagnosticCategory::InvalidEdgeTransfer,
            "internal entry has no forward states",
        ));
    }
    let live: BTreeSet<_> = analysis.live_in[&segment].iter().collect();
    let live_atoms = atoms(program, &live);
    // Select eligible invariant names before old-source equality chooses a representative.
    // A changing parameter may equal a capture on entry without preserving that equality.
    let parameters = &program.segments.iter().find(|s| s.id == segment).unwrap().parameters;
    let cached_atoms: Vec<_> = live_atoms
        .iter()
        .copied()
        .filter(|atom| loop_demand.is_none() || !parameters.contains(&atom.value))
        .collect();
    let common = common_slots(incoming, &cached_atoms, context)?;
    let aliases = aliases(incoming, &live_atoms, context, loop_demand.is_some())?;
    let tail = incoming[0].state.tail;
    if incoming.iter().any(|entry| entry.state.tail != tail) {
        return Err(invariant(
            DiagnosticCategory::InvalidEdgeTransfer,
            "incoming tail policies disagree",
        ));
    }
    let base = EntryContract {
        stack_top_first: vec![],
        required_spill_slots: vec![],
        required_scratch: scratch(incoming, &analysis.scratch.entry[&segment], context)?,
        address_facts: common_addresses(incoming, &live, context)?,
        aliases,
        tail,
    };
    let translated = incoming
        .iter()
        .map(|entry| {
            entry
                .state
                .stack
                .iter()
                .map(|o| rename(entry, o.atom, &live_atoms, context))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut candidates = vec![];
    if loop_demand.is_none()
        && (incoming.len() == 1 || translated.iter().all(|stack| *stack == translated[0]))
    {
        let mut agreed = base.clone();
        agreed.stack_top_first = translated[0].clone();
        agreed.required_spill_slots = common.iter().map(|(slot, atom)| (*slot, *atom)).collect();
        if recoverable(&agreed, segment, &live_atoms, context)? {
            candidates.push(agreed);
        }
    }
    if candidates.is_empty() {
        let ordinary = &analysis.next_use.entry[&segment];
        let mut ranked: Vec<_> = live.iter().copied().collect();
        if let Some(demand) = loop_demand {
            let cyclic = &demand.next_use.entry[&segment];
            let first = &translated[0];
            ranked.sort_by_key(|value| {
                let distance = cyclic.get(*value);
                let repeated = distance != NextUseDistance::Infinite;
                (
                    !repeated,
                    if repeated {
                        distance
                    } else {
                        ordinary.get(*value)
                    },
                    first.iter().position(|a| a.value == *value).unwrap_or(usize::MAX),
                    *value,
                )
            });
        } else {
            ranked.sort_by_key(|value| (ordinary.get(*value), *value));
        }
        let mut next = base.clone();
        next.stack_top_first = rank_layout(program, &ranked);
        candidates.push(next);
        if loop_demand.is_none() {
            let mut resident = vec![];
            for value in &ranked {
                let mut maximum = 0;
                let mut all = true;
                for entry in incoming {
                    if let Some(depth) = complete_value_depth(entry, *value, program, context)? {
                        maximum = maximum.max(depth);
                    } else {
                        all = false;
                        break;
                    }
                }
                if all {
                    resident.push((*value, maximum));
                }
            }
            resident.sort_by_key(|(value, depth)| (*depth, ordinary.get(*value), *value));
            let mut resident_rank: Vec<_> = resident.iter().map(|(value, _)| *value).collect();
            resident_rank
                .extend(ranked.iter().filter(|value| !resident.iter().any(|(v, _)| v == *value)));
            let mut common_resident = base.clone();
            common_resident.stack_top_first = rank_layout(program, &resident_rank);
            candidates.push(common_resident);
            let mut groups: BTreeMap<Vec<Atom>, (usize, EdgeId)> = BTreeMap::new();
            for (entry, stack) in incoming.iter().zip(&translated) {
                let mut order = vec![];
                let mut cursor = 0;
                while cursor < stack.len() {
                    let atom = stack[cursor];
                    let width = usize::from(program.values[&atom.value].layout.lanes());
                    if atom.lane == 0
                        && live.contains(&atom.value)
                        && !order.contains(&atom.value)
                        && (0..width).all(|lane| {
                            stack.get(cursor + lane) == Some(&Atom::new(atom.value, lane as u8))
                        })
                    {
                        order.push(atom.value);
                        cursor += width;
                    } else {
                        cursor += 1;
                    }
                }
                let layout = rank_layout(program, &order);
                let group = groups.entry(layout).or_insert((0, entry.edge.id));
                group.0 += 1;
                group.1 = group.1.min(entry.edge.id);
            }
            if let Some((layout, _)) = groups
                .into_iter()
                .min_by_key(|(_, (weight, edge))| (core::cmp::Reverse(*weight), *edge))
            {
                let mut inherited = base;
                inherited.stack_top_first = layout;
                candidates.push(inherited);
            }
        }
        for candidate in &mut candidates {
            required_slots(
                candidate,
                program,
                segment,
                &live_atoms,
                &common,
                context,
                loop_demand.is_some(),
            )?;
        }
    }
    let mut distinct = vec![];
    for candidate in candidates {
        if !distinct.contains(&candidate) {
            distinct.push(candidate);
        }
    }
    let successor = program.segments.iter().find(|s| s.id == segment).unwrap();
    let mut best: Option<(usize, EntryContract, BTreeMap<EdgeId, Preparation>)> = None;
    #[cfg(test)]
    let mut costs = vec![];
    for candidate in distinct {
        let trial = (|| {
            let mut repairs = BTreeMap::new();
            let mut score = 0usize;
            for entry in incoming {
                let repair = repair_edge(entry.state, entry.edge, &candidate, context)?;
                score = score.saturating_add(action_cost(&repair.actions));
                repairs.insert(entry.edge.id, repair);
            }
            if let Some(unit) = successor.units.first() {
                let state = AllocationState::at_entry(&candidate, segment, context)?;
                let live_after = analysis.after[&unit.id].iter().collect();
                let first = prepare_unit(&state, unit, &live_after, context)?;
                score = score
                    .saturating_add(incoming.len().saturating_mul(action_cost(&first.actions)));
            }
            Ok::<_, AllocationError>((score, repairs))
        })();
        if let Ok((score, repairs)) = trial {
            #[cfg(test)]
            costs.push(score);
            if best.as_ref().is_none_or(|(cost, ..)| score < *cost) {
                best = Some((score, candidate, repairs));
            }
        }
    }
    let (_, contract, repairs) = best.ok_or_else(|| {
        invariant(
            DiagnosticCategory::UnavailableValue,
            "no entry contract can recover all live atoms",
        )
    })?;
    Ok(Choice {
        contract,
        repairs,
        #[cfg(test)]
        costs,
    })
}
