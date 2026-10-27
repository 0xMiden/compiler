//! Repair a transfer to an already frozen destination contract.
//!
//! Destination requirements are first translated to old source names and validated there.
//! Storage writes preserve parallel-assignment sources before clobbering any destination.
//! Only after physical transfer succeeds are destination names and exactly the target facts
//! published together; cached copies from an earlier loop iteration cannot leak through.
use alloc::collections::{BTreeMap, BTreeSet};

use super::{
    normalize::UnitRequirements,
    schedule::{move_up, permute},
    state::*,
    storage::Trial,
};
use crate::plan::*;

/// Prepare a transfer to `target`, retaining useful occurrences when legal.
///
/// The input state is immutable. A failed incremental trial falls back to a complete
/// owned-stack drain and rebuild. Both paths require source-established aliases, scratch
/// contents, provenance, and unchanged tail protection; repairs cannot invent those facts.
/// The returned preparation contains edge actions only, with no lowering execution.
pub(crate) fn repair_edge(
    state: &AllocationState,
    edge: &TransferEdge,
    target: &EntryContract,
    context: &AllocationContext<'_>,
) -> Result<Preparation, AllocationError> {
    transfer(state, edge, target, context, true).map_err(|error| error.on_edge(edge))
}

/// Construct a transfer by draining and rebuilding, bypassing incremental optimization.
/// Shares validation and publication rules with `repair_edge` for equivalence testing.
#[cfg(test)]
pub(super) fn normalize_edge(
    state: &AllocationState,
    edge: &TransferEdge,
    target: &EntryContract,
    context: &AllocationContext<'_>,
) -> Result<Preparation, AllocationError> {
    transfer(state, edge, target, context, false).map_err(|error| error.on_edge(edge))
}

/// Validate in the source namespace, move physical representations, then rename once.
/// Each speculative path starts from the same state and reserves mandatory slot IDs
/// before creating snapshots, preserving simultaneous-assignment semantics.
fn transfer(
    state: &AllocationState,
    edge: &TransferEdge,
    target: &EntryContract,
    context: &AllocationContext<'_>,
    incremental: bool,
) -> Result<Preparation, AllocationError> {
    let mut values = BTreeMap::new();
    for binding in &edge.bindings {
        if values.insert(binding.destination, binding.source).is_some() {
            return Err(invariant(
                DiagnosticCategory::InvalidEdgeTransfer,
                "duplicate edge destination",
            ));
        }
    }
    let contents: BTreeMap<_, _> = edge
        .content_bindings
        .iter()
        .map(|binding| (binding.destination, binding.source))
        .collect();
    if contents.len() != edge.content_bindings.len() {
        return Err(invariant(
            DiagnosticCategory::InvalidEdgeTransfer,
            "duplicate content destination",
        ));
    }
    let source =
        |atom: Atom| Atom::new(values.get(&atom.value).copied().unwrap_or(atom.value), atom.lane);
    if state.tail != target.tail {
        return Err(invariant(
            DiagnosticCategory::InvalidEdgeTransfer,
            "edge cannot change tail protection",
        ));
    }
    for binding in &edge.bindings {
        let value = context.values.get(&binding.destination).ok_or_else(|| {
            invariant(DiagnosticCategory::InvalidEdgeTransfer, "unknown destination parameter")
        })?;
        if let Some(reconstruction) = &value.reconstruction {
            for lane in 0..value.layout.lanes() {
                let expected = match reconstruction {
                    Reconstruction::Literal(lanes) => {
                        StoredValue::Literal(*lanes.get(usize::from(lane)).ok_or_else(|| {
                            invariant(
                                DiagnosticCategory::InvalidEdgeTransfer,
                                "missing reconstructed parameter lane",
                            )
                        })?)
                    }
                    Reconstruction::Alias(lanes) => StoredValue::Atom(source(
                        *lanes.get(usize::from(lane)).ok_or_else(|| {
                            invariant(
                                DiagnosticCategory::InvalidEdgeTransfer,
                                "missing reconstructed parameter lane",
                            )
                        })?,
                    )),
                };
                if !context.same(
                    state,
                    StoredValue::Atom(Atom::new(binding.source, lane)),
                    expected,
                )? {
                    return Err(invariant(
                        DiagnosticCategory::InvalidEdgeTransfer,
                        "incoming parameter violates its reconstruction identity",
                    ));
                }
            }
        }
    }
    for (a, b) in target.aliases.pairs() {
        if !context.same(state, StoredValue::Atom(source(*a)), StoredValue::Atom(source(*b)))? {
            return Err(invariant(
                DiagnosticCategory::InvalidEdgeTransfer,
                "edge alias is not established by old sources",
            ));
        }
    }
    for fact in &target.required_scratch {
        let content = match fact.content {
            ContentRequirement::Atom(atom) => ContentRequirement::Atom(source(atom)),
            ContentRequirement::Content(id) => {
                ContentRequirement::Content(contents.get(&id).copied().unwrap_or(id))
            }
            ContentRequirement::Initialized => ContentRequirement::Initialized,
        };
        let mut established = false;
        for actual in &state.scratch {
            if actual.object != fact.object || actual.offset != fact.offset {
                continue;
            }
            established = match (content, actual.content) {
                (ContentRequirement::Initialized, _) => true,
                (ContentRequirement::Atom(a), ContentRequirement::Atom(b)) => {
                    context.same(state, StoredValue::Atom(a), StoredValue::Atom(b))?
                }
                (a, b) => a == b,
            };
            if established {
                break;
            }
        }
        if !established {
            return Err(invariant(
                DiagnosticCategory::InvalidEdgeTransfer,
                "edge cannot invent scratch contents",
            ));
        }
    }
    for fact in &target.address_facts {
        let mut required = *fact;
        required.pointer = values.get(&fact.pointer).copied().unwrap_or(fact.pointer);
        let mut established = false;
        for actual in &state.addresses {
            if actual.object == required.object
                && actual.offset == required.offset
                && actual.accessible == required.accessible
            {
                established |= context.same(
                    state,
                    StoredValue::Atom(Atom::new(actual.pointer, 0)),
                    StoredValue::Atom(Atom::new(required.pointer, 0)),
                )?;
            }
        }
        if !established {
            return Err(invariant(
                DiagnosticCategory::InvalidEdgeTransfer,
                "edge cannot invent address provenance",
            ));
        }
    }
    let desired: alloc::vec::Vec<_> = target.stack_top_first.iter().copied().map(source).collect();
    let mut assignments = BTreeMap::new();
    for (slot, atom) in &target.required_spill_slots {
        let atom = source(*atom);
        if let Some(previous) = assignments.insert(*slot, atom)
            && !context.same(state, StoredValue::Atom(previous), StoredValue::Atom(atom))?
        {
            return Err(invariant(
                DiagnosticCategory::InvalidEdgeTransfer,
                "conflicting destination slot assignments",
            ));
        }
    }
    let needed = desired
        .iter()
        .copied()
        .chain(assignments.values().copied())
        .collect::<BTreeSet<_>>();
    let mut trial = Trial::new(state, context, SlotOwner::Edge(edge.id));
    // Reserve mandatory identities before generating snapshots in the same edge namespace.
    trial.state.slot_requests.extend(assignments.keys().copied());
    if !incremental || incremental_transfer(&mut trial, &desired, &assignments, &needed).is_err() {
        trial = Trial::new(state, context, SlotOwner::Edge(edge.id));
        trial.state.slot_requests.extend(assignments.keys().copied());
        trial.drain(&needed)?;
        for (slot, atom) in assignments {
            trial.write(slot, StoredValue::Atom(atom), &needed)?;
        }
        for atom in desired.iter().rev() {
            trial.load(*atom)?;
        }
    }
    // All physical transfers still use the original names. Publish the target names together,
    // retaining exactly its declared facts rather than stale previous-iteration copies.
    for (occurrence, atom) in trial.state.stack.iter_mut().zip(&target.stack_top_first) {
        occurrence.atom = *atom;
    }
    trial.state.slots = target
        .required_spill_slots
        .iter()
        .map(|(slot, atom)| (*slot, StoredValue::Atom(*atom)))
        .collect();
    trial.state.aliases = target.aliases.clone();
    trial.state.scratch = target.required_scratch.clone();
    trial.state.addresses = target.address_facts.clone();
    trial.state.available_values = target
        .stack_top_first
        .iter()
        .chain(target.required_spill_slots.iter().map(|(_, atom)| atom))
        .map(|atom| atom.value)
        .chain(target.aliases.pairs().iter().flat_map(|(a, b)| [a.value, b.value]))
        .chain(edge.bindings.iter().map(|binding| binding.destination))
        .collect();
    if let Some(destination) = edge.destination {
        trial
            .state
            .available_values
            .extend(context.reconstructions_at_entry(destination)?);
    }
    Ok(Preparation::finish(trial))
}

/// Write mandatory slots, retain distinct useful occurrences, and fill only missing lanes.
/// Exact occurrence permutation enforces multiplicity even when several atoms are equal.
/// A failure leaves this private trial unusable; the caller retries from the original state.
fn incremental_transfer(
    trial: &mut Trial<'_>,
    desired: &[Atom],
    assignments: &BTreeMap<SpillSlotId, Atom>,
    needed: &BTreeSet<Atom>,
) -> Result<(), AllocationError> {
    for (slot, atom) in assignments {
        trial.write(*slot, StoredValue::Atom(*atom), needed)?;
    }
    let mut retained = BTreeSet::new();
    let mut missing = alloc::vec::Vec::new();
    for atom in desired {
        let mut found = None;
        for occurrence in &trial.state.stack {
            if !retained.contains(&occurrence.id)
                && trial.context.same(
                    &trial.state,
                    StoredValue::Atom(*atom),
                    StoredValue::Atom(occurrence.atom),
                )?
            {
                found = Some(occurrence.id);
                break;
            }
        }
        if let Some(id) = found {
            retained.insert(id);
        } else {
            missing.push(*atom);
        }
    }
    while let Some(depth) = trial
        .state
        .stack
        .iter()
        .position(|occurrence| !retained.contains(&occurrence.id))
    {
        move_up(trial, depth)?;
        trial.state.stack.pop_front();
        trial.actions.push(actions::PlanAction::Drop);
    }
    for atom in missing {
        trial.load(atom)?;
    }
    let mut placed = BTreeSet::new();
    let mut target = alloc::vec::Vec::new();
    for atom in desired {
        let mut found = None;
        for (depth, occurrence) in trial.state.stack.iter().enumerate() {
            if !placed.contains(&occurrence.id)
                && trial.context.same(
                    &trial.state,
                    StoredValue::Atom(*atom),
                    StoredValue::Atom(occurrence.atom),
                )?
            {
                found = Some(depth);
                break;
            }
        }
        let depth = found.ok_or_else(|| {
            invariant(DiagnosticCategory::UnavailableValue, "incremental edge lost a target source")
        })?;
        let id = trial.state.stack[depth].id;
        target.push(id);
        placed.insert(id);
    }
    permute(
        trial,
        &target,
        &UnitRequirements {
            needed: needed.clone(),
            survivors: BTreeSet::new(),
        },
    )
}
