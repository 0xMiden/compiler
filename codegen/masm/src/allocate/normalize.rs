//! Constructive fallback for every feasible lowering preparation.
//!
//! Drain only owned lanes into safe symbolic storage, bind stored inputs, and rebuild the
//! requested top-first prefix. Live-through values remain recoverable in storage or as
//! literals. This path does not depend on grouped stack search or its optimization budget.
use alloc::collections::{BTreeMap, BTreeSet};

use super::{state::*, storage::Trial};
use crate::{lower::units::LoweringBindings, plan::*};

/// Prepare a unit by storing needed identities and rebuilding its exact input prefix.
///
/// `live_after` names semantic values after normal completion; newly defined outputs
/// need no old copy. The proposal leaves the caller untouched and does not run the unit.
/// Missing nonliteral sources or unsafe writes fail the private construction.
pub(crate) fn prepare_unit(
    state: &AllocationState,
    unit: &SelectedLowering,
    live_after: &BTreeSet<ValueId>,
    context: &AllocationContext<'_>,
) -> Result<Preparation, AllocationError> {
    let mut trial = Trial::new(state, context, SlotOwner::Unit(unit.id));
    let needs = requirements(unit, live_after, context)?;
    trial.drain(&needs.needed)?;
    // Live-through values stay in memory. A reconstructible literal needs no backing store.
    for atom in &needs.needed {
        if trial.slot_for(StoredValue::Atom(*atom))?.is_none()
            && !matches!(context.source(&trial.state, *atom)?, StoredValue::Literal(_))
        {
            return Err(invariant(
                DiagnosticCategory::UnavailableValue,
                "normalization lost a required semantic value",
            ));
        }
    }
    bind_inputs(&mut trial, unit, &needs.needed)?;
    for atom in unit.stack_inputs.iter().rev() {
        trial.load(*atom)?;
    }
    Ok(Preparation::finish(trial))
}

/// Raw-lane demand separating entry availability from preservation through consumption.
pub(super) struct UnitRequirements {
    /// Inputs, alias-reconstruction dependencies, and live-through lanes required at entry.
    pub needed: BTreeSet<Atom>,
    /// Old values demanded after normal completion, excluding this unit's definitions.
    pub survivors: BTreeSet<Atom>,
}

/// Expand complete semantic input demand, including alias recipes, into raw lanes.
/// Live-through survivors are deliberately excluded: their cached cells need not be read.
pub(super) fn input_demand(
    unit: &SelectedLowering,
    context: &AllocationContext<'_>,
) -> Result<BTreeSet<Atom>, AllocationError> {
    let mut needed: BTreeSet<_> = unit.stack_inputs.iter().copied().collect();
    let mut semantic_uses: BTreeSet<_> = unit.semantic_inputs.iter().copied().collect();
    for output in &unit.outputs {
        if let Some(Reconstruction::Alias(sources)) = context
            .values
            .get(&output.value)
            .and_then(|value| value.reconstruction.as_ref())
        {
            semantic_uses.extend(sources.iter().map(|source| source.value));
        }
    }
    for value in semantic_uses {
        let layout = &context
            .values
            .get(&value)
            .ok_or_else(|| {
                invariant(DiagnosticCategory::UnavailableValue, "semantic input has no layout")
            })?
            .layout;
        needed.extend((0..layout.lanes()).map(|lane| Atom::new(value, lane)));
    }
    for input in &unit.stored_inputs {
        if let StoredValue::Atom(atom) = input.value {
            needed.insert(atom);
        }
    }
    Ok(needed)
}

/// Add live-through demand to the complete semantic inputs needed for preparation.
pub(super) fn requirements(
    unit: &SelectedLowering,
    live_after: &BTreeSet<ValueId>,
    context: &AllocationContext<'_>,
) -> Result<UnitRequirements, AllocationError> {
    let mut needed = input_demand(unit, context)?;
    let definitions: BTreeSet<_> = unit.outputs.iter().map(|atom| atom.value).collect();
    let mut survivors = BTreeSet::new();
    for value in live_after.difference(&definitions) {
        let layout = &context
            .values
            .get(value)
            .ok_or_else(|| {
                invariant(DiagnosticCategory::UnavailableValue, "live value has no layout")
            })?
            .layout;
        survivors.extend((0..layout.lanes()).map(|lane| Atom::new(*value, lane)));
    }
    needed.extend(&survivors);
    Ok(UnitRequirements { needed, survivors })
}

/// Bind every stored input to protected contents before opaque lowering execution.
/// Replaces this trial's binding table with the unit's complete bindings; duplicate input
/// IDs or unavailable sources invalidate the trial, which callers must discard on failure.
pub(super) fn bind_inputs(
    trial: &mut Trial<'_>,
    unit: &SelectedLowering,
    needed: &BTreeSet<Atom>,
) -> Result<(), AllocationError> {
    let mut bindings = LoweringBindings::default();
    for input in &unit.stored_inputs {
        let slot = trial.bind_stored(input.value, needed)?;
        if bindings.stored_inputs.insert(input.id, slot).is_some() {
            return Err(invariant(
                DiagnosticCategory::InvalidSelectedLowering,
                "duplicate stored input ID",
            ));
        }
    }
    trial.bindings = BTreeMap::from([(unit.id, bindings)]);
    Ok(())
}
