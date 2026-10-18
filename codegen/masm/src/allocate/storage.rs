//! Safe symbolic storage shared by scheduling, normalization, and edge repair.
//!
//! Every write preserves the last needed copy of old contents before replacing them.
//! Canonical slots are preferences, while bound stored inputs are protected until the
//! opaque lowering runs. Actions and facts accumulate privately and are discarded on failure.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use super::state::*;
use crate::{
    lower::units::LoweringBindings,
    plan::{actions::PlanAction, *},
};

/// A private preparation workspace; cloning includes identity counters and storage requests.
#[derive(Clone)]
pub(super) struct Trial<'a> {
    /// Facts after the actions accumulated so far.
    pub state: AllocationState,
    /// Raw physical actions in execution order.
    pub actions: Vec<PlanAction>,
    /// Stored-input bindings to publish with the accepted proposal.
    pub bindings: BTreeMap<SelectedStepId, LoweringBindings>,
    /// Checked semantic identities and scheduling policy for this procedure.
    pub context: &'a AllocationContext<'a>,
    /// Namespace used for snapshots and other fresh temporary cells.
    pub owner: SlotOwner,
    /// Sources usable before the lowering, but unavailable after its input consumption.
    pub consumed_inputs: BTreeSet<OccurrenceId>,
    /// Cells promised to an opaque lowering; a different value cannot overwrite them.
    protected: BTreeSet<SpillSlotId>,
}

impl<'a> Trial<'a> {
    /// Fork caller facts without publishing actions, bindings, or fresh identities.
    pub fn new(
        state: &AllocationState,
        context: &'a AllocationContext<'a>,
        owner: SlotOwner,
    ) -> Self {
        Self {
            state: state.clone(),
            actions: Vec::new(),
            bindings: BTreeMap::new(),
            context,
            owner,
            consumed_inputs: BTreeSet::new(),
            protected: BTreeSet::new(),
        }
    }

    /// Find the least symbolic slot with equivalent current contents, if any.
    /// Equality failures propagate; a missing copy is an ordinary `None` result.
    pub fn slot_for(&self, value: StoredValue) -> Result<Option<SpillSlotId>, AllocationError> {
        for (slot, contents) in &self.state.slots {
            if self.context.same(&self.state, *contents, value)? {
                return Ok(Some(*slot));
            }
        }
        Ok(None)
    }

    /// Test demand by semantic identity so aliases protect one another's stored copies.
    fn needed(&self, value: StoredValue, needed: &BTreeSet<Atom>) -> Result<bool, AllocationError> {
        for atom in needed {
            if self.context.same(&self.state, value, StoredValue::Atom(*atom))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Check for another copy that survives input consumption, or a literal recipe.
    fn elsewhere(&self, value: StoredValue, except: SpillSlotId) -> Result<bool, AllocationError> {
        if matches!(value, StoredValue::Literal(_)) {
            return Ok(true);
        }
        if let StoredValue::Atom(atom) = value
            && matches!(self.context.source(&self.state, atom)?, StoredValue::Literal(_))
        {
            return Ok(true);
        }
        for occurrence in &self.state.stack {
            if !self.consumed_inputs.contains(&occurrence.id)
                && self.context.same(&self.state, value, StoredValue::Atom(occurrence.atom))?
            {
                return Ok(true);
            }
        }
        for (slot, contents) in &self.state.slots {
            if *slot != except && self.context.same(&self.state, value, *contents)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Choose storage without emitting a write: reuse an equal copy, prefer a safe
    /// canonical atom cell, otherwise reserve a fresh temporary in this trial's namespace.
    /// Protected cells and the only surviving copy of needed old contents cannot be reused.
    pub fn safe_spill_slot(
        &mut self,
        value: StoredValue,
        needed: &BTreeSet<Atom>,
    ) -> Result<SpillSlotId, AllocationError> {
        if let Some(slot) = self.slot_for(value)? {
            return Ok(slot);
        }
        if let StoredValue::Atom(atom) = value {
            let slot = SpillSlotId::canonical(atom);
            if !self.protected.contains(&slot)
                && match self.state.slots.get(&slot).copied() {
                    None => true,
                    Some(old) => !self.needed(old, needed)? || self.elsewhere(old, slot)?,
                }
            {
                self.state.slot_requests.insert(slot);
                return Ok(slot);
            }
        }
        self.state.fresh_slot(self.owner)
    }

    /// Snapshot the only needed old copy before a mandatory write to its cell.
    /// A protected stored-input binding cannot be changed, even when another copy exists.
    fn protect_write(
        &mut self,
        slot: SpillSlotId,
        value: StoredValue,
        needed: &BTreeSet<Atom>,
    ) -> Result<(), AllocationError> {
        if let Some(old) = self.state.slots.get(&slot).copied() {
            if self.context.same(&self.state, old, value)? {
                return Ok(());
            }
            if self.protected.contains(&slot) {
                return Err(invariant(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "write would clobber a bound stored input",
                ));
            }
            if self.needed(old, needed)? && !self.elsewhere(old, slot)? {
                let snapshot = self.state.fresh_slot(self.owner)?;
                self.actions
                    .extend([PlanAction::LoadLane(slot), PlanAction::StoreLane(snapshot)]);
                self.state.slots.insert(snapshot, old);
            }
        }
        self.state.slot_requests.insert(slot);
        Ok(())
    }

    /// Consume the top owned lane into a safe symbolic cell and record its new contents.
    /// Fails if the owned stack is empty or safe storage cannot be established.
    pub fn back_up_top(&mut self, needed: &BTreeSet<Atom>) -> Result<SpillSlotId, AllocationError> {
        let atom = self
            .state
            .stack
            .front()
            .ok_or_else(|| {
                invariant(DiagnosticCategory::IllegalStackAccess, "no owned lane to store")
            })?
            .atom;
        let value = StoredValue::Atom(atom);
        let slot = self.safe_spill_slot(value, needed)?;
        self.protect_write(slot, value, needed)?;
        self.actions.push(PlanAction::StoreLane(slot));
        self.state.stack.pop_front();
        self.state.slots.insert(slot, value);
        Ok(slot)
    }

    /// Emit a literal push, slot load, or legal top-sixteen duplicate for a raw source.
    /// The caller accounts for the resulting occurrence; existing occurrences are unchanged.
    fn materialize(&mut self, value: StoredValue) -> Result<(), AllocationError> {
        let source = match value {
            StoredValue::Atom(atom) => self.context.source(&self.state, atom)?,
            literal => literal,
        };
        if let StoredValue::Literal(literal) = source {
            self.actions.push(PlanAction::PushLiteral(literal));
            return Ok(());
        }
        if let Some(slot) = self.slot_for(value)? {
            self.actions.push(PlanAction::LoadLane(slot));
            return Ok(());
        }
        for (depth, occurrence) in self.state.stack.iter().take(16).enumerate() {
            if self.context.same(&self.state, value, StoredValue::Atom(occurrence.atom))? {
                self.actions.push(PlanAction::Dup { depth: depth as u8 });
                return Ok(());
            }
        }
        Err(invariant(
            DiagnosticCategory::UnavailableValue,
            "required atom has no legal preserved source",
        ))
    }

    /// Establish `value` in a mandatory cell, preserving needed old contents first.
    /// An already equal cell needs no action. On failure, discard this private trial.
    pub fn write(
        &mut self,
        slot: SpillSlotId,
        value: StoredValue,
        needed: &BTreeSet<Atom>,
    ) -> Result<(), AllocationError> {
        if let Some(old) = self.state.slots.get(&slot)
            && self.context.same(&self.state, *old, value)?
        {
            return Ok(());
        }
        self.protect_write(slot, value, needed)?;
        self.materialize(value)?;
        self.actions.push(PlanAction::StoreLane(slot));
        self.state.slots.insert(slot, value);
        Ok(())
    }

    /// Establish a stored input and protect its cell from later writes in this trial.
    /// Equivalent inputs may share a cell; protection applies to its semantic contents.
    pub fn bind_stored(
        &mut self,
        value: StoredValue,
        needed: &BTreeSet<Atom>,
    ) -> Result<SpillSlotId, AllocationError> {
        let slot = self.safe_spill_slot(value, needed)?;
        self.write(slot, value, needed)?;
        self.protected.insert(slot);
        Ok(slot)
    }

    /// Materialize another copy at the top and assign it a fresh occurrence identity.
    pub fn load(&mut self, atom: Atom) -> Result<(), AllocationError> {
        self.materialize(StoredValue::Atom(atom))?;
        self.state.push(atom)
    }

    /// Empty only the owned stack, storing needed nonliteral identities lacking a slot.
    /// The protected caller suffix and VM padding are never inspected or consumed.
    pub fn drain(&mut self, needed: &BTreeSet<Atom>) -> Result<(), AllocationError> {
        while let Some(occurrence) = self.state.stack.front() {
            let value = StoredValue::Atom(occurrence.atom);
            let literal = matches!(
                self.context.source(&self.state, occurrence.atom)?,
                StoredValue::Literal(_)
            );
            if self.needed(value, needed)? && self.slot_for(value)?.is_none() && !literal {
                self.back_up_top(needed)?;
            } else {
                self.actions.push(PlanAction::Drop);
                self.state.stack.pop_front();
            }
        }
        Ok(())
    }
}
