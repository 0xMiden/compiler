//! Physical facts at one planning point and the ownership boundary for speculative work.
//!
//! A state describes owned stack occurrences and symbolic storage, before frame offsets exist.
//! Preparations operate on a clone and publish their actions, bindings, and resulting state
//! together. Equality and reconstruction come from checked selection and the current scope;
//! they are not inferred from the verifier's later simulation of emitted instructions.
use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    vec::Vec,
};

use super::{
    analysis::{AllocationAnalysis, NextUses},
    schedule::{AllocationOptions, SchedulingTier},
    storage::Trial,
};
use crate::{
    lower::units::LoweringBindings,
    plan::{actions::PlanAction, *},
    verify::CheckedSelectedProgram,
};

/// Allocation failures distinguish an invalid plan from exhausted symbolic identity space.
#[derive(Debug)]
pub(crate) enum AllocationError {
    /// A selected or physical contract could not be established; retains its diagnostic category.
    Invariant(PlanError),
    /// A fresh occurrence or temporary slot would overflow its `u32` namespace.
    IdentifierExhausted(&'static str),
}

impl From<PlanError> for AllocationError {
    fn from(error: PlanError) -> Self {
        Self::Invariant(error)
    }
}

/// Attach the failed physical or semantic contract to an allocator diagnostic.
pub(super) fn invariant(category: DiagnosticCategory, message: &'static str) -> AllocationError {
    AllocationError::Invariant(PlanError::new(category, message))
}

/// Known physical representations and semantic equalities at one point in a segment.
///
/// Only owned lanes are listed. The caller suffix and VM floor padding are never occurrences.
/// A slot's presence means its contents are known, while a request reserves its symbolic
/// identity even after those contents cease to be useful. Neither is a physical frame offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllocationState {
    /// Top first; equal atoms may have distinct occurrence identities.
    pub stack: VecDeque<Occurrence>,
    /// Known current contents, including noncanonical snapshots and cached copies.
    pub slots: BTreeMap<SpillSlotId, StoredValue>,
    /// Equalities valid in this dynamic scope, rather than across all loop iterations.
    pub aliases: LaneAliases,
    /// Established scratch initialization or exact contents, in element offsets.
    pub scratch: Vec<ScratchCellFact>,
    /// Pointer provenance; knowing an address does not establish its cell contents.
    pub addresses: Vec<AddressFact>,
    /// Protection of the unowned stack beneath `stack`.
    pub tail: TailPolicy,
    /// Definitions whose checked literal/alias reconstruction is valid in this dynamic scope.
    pub available_values: BTreeSet<ValueId>,
    /// All symbolic cells reserved so far, including cells no longer present in `slots`.
    pub slot_requests: BTreeSet<SpillSlotId>,
    /// Next unused occurrence identity in this state and its private descendants.
    pub next_occurrence: u32,
    /// Next temporary index per owner; unit and edge namespaces avoid cross-site collisions.
    pub next_temporary: BTreeMap<SlotOwner, u32>,
}

impl AllocationState {
    /// Apply a unit's declared normal completion after its input prefix has been prepared.
    ///
    /// Consumes the top-first input lanes, publishes the output lanes, invalidates facts
    /// about redefined values, and updates scratch/provenance facts. Alias-only publication
    /// enables checked reconstructions without consuming or producing physical lanes. Unit-local scratch
    /// does not survive completion. A nonreturning unit has no completion state.
    ///
    /// This mutates in place; a failure invalidates this state. Speculative callers must
    /// use their private state, as they do for preparation and identifier allocation.
    pub(super) fn complete(&mut self, unit: &SelectedLowering) -> Result<(), AllocationError> {
        if !unit.effects.normal_return {
            return Err(invariant(
                DiagnosticCategory::InvalidSelectedLowering,
                "nonreturning unit cannot publish results",
            ));
        }
        let definitions: BTreeSet<_> = unit.outputs.iter().map(|atom| atom.value).collect();
        let contents: BTreeSet<_> = unit
            .scratch_effects
            .iter()
            .filter_map(|effect| match effect {
                ScratchEffect::Write(ScratchCellFact {
                    content: ContentRequirement::Content(id),
                    ..
                }) => Some(*id),
                _ => None,
            })
            .collect();
        self.slots.retain(|_, value| !matches!(value,StoredValue::Atom(atom) if definitions.contains(&atom.value)));
        self.aliases = LaneAliases::new(
            self.aliases
                .pairs()
                .iter()
                .copied()
                .filter(|(a, b)| !definitions.contains(&a.value) && !definitions.contains(&b.value))
                .collect(),
        );
        self.addresses.retain(|fact| !definitions.contains(&fact.pointer));
        self.scratch.retain_mut(|fact| {
            let old = match fact.content {
                ContentRequirement::Atom(atom) => definitions.contains(&atom.value),
                ContentRequirement::Content(id) => contents.contains(&id),
                ContentRequirement::Initialized => false,
            };
            if old {
                fact.content = ContentRequirement::Initialized;
            }
            for effect in &unit.scratch_effects {
                match effect {
                    ScratchEffect::Write(written)
                        if fact.object == written.object && fact.offset == written.offset =>
                    {
                        return false;
                    }
                    ScratchEffect::PossibleWrite {
                        object,
                        offset,
                        size,
                    }
                    | ScratchEffect::Borrow {
                        object,
                        offset,
                        size,
                        write: true,
                        ..
                    } if fact.object == *object
                        && fact.offset >= *offset
                        && fact.offset - offset < *size =>
                    {
                        fact.content = ContentRequirement::Initialized
                    }
                    _ => {}
                }
            }
            true
        });
        for effect in &unit.scratch_effects {
            match effect {
                ScratchEffect::Write(fact) => self.scratch.push(*fact),
                ScratchEffect::Borrow {
                    object,
                    offset,
                    size,
                    write: true,
                    definite_write: true,
                    ..
                } => {
                    for cell in *offset..offset.checked_add(*size).ok_or_else(|| {
                        invariant(
                            DiagnosticCategory::InvalidSelectedLowering,
                            "scratch range overflow",
                        )
                    })? {
                        self.scratch.push(ScratchCellFact {
                            object: *object,
                            offset: cell,
                            content: ContentRequirement::Initialized,
                        });
                    }
                }
                _ => {}
            }
        }
        self.scratch.retain(|fact| {
            unit.scratch
                .iter()
                .find(|object| object.id == fact.object)
                .is_none_or(|object| object.lifetime == ScratchLifetime::Spanning)
        });
        self.scratch.sort();
        self.scratch.dedup();
        if self.stack.len() < unit.stack_inputs.len() {
            return Err(invariant(
                DiagnosticCategory::UnavailableValue,
                "unit consumed unavailable owned lanes",
            ));
        }
        self.stack.drain(..unit.stack_inputs.len());
        self.available_values.retain(|value| !definitions.contains(value));
        match unit.publication {
            OutputPublication::Stack => {
                for atom in unit.outputs.iter().rev() {
                    self.push(*atom)?;
                }
            }
            OutputPublication::Aliases => {
                // Forget stale semantic facts above, but preserve occurrences and storage.
                // Checked reconstructions now make these complete output names available.
                self.available_values.extend(definitions);
            }
        }
        self.addresses.extend(unit.address_results.iter().copied());
        Ok(())
    }

    /// Start a segment from its frozen contract, independently of predecessor state.
    ///
    /// Adds only the reconstruction definitions valid at this entry. Fails for an unknown
    /// segment scope, conflicting slot contents, or exhausted occurrence identities.
    pub fn at_entry(
        contract: &EntryContract,
        segment: SegmentId,
        context: &AllocationContext<'_>,
    ) -> Result<Self, AllocationError> {
        let mut state = Self::from_contract(contract)?;
        state.available_values.extend(context.reconstructions_at_entry(segment)?);
        Ok(state)
    }

    /// Establish the contract's physical facts with fresh local occurrence identities.
    ///
    /// Reconstruction availability comes only from represented values and aliases here;
    /// selected segment entries must use `at_entry` to add dominating reconstructions.
    /// Conflicting requirements for one slot are rejected.
    pub fn from_contract(contract: &EntryContract) -> Result<Self, AllocationError> {
        let mut state = Self {
            stack: VecDeque::new(),
            slots: BTreeMap::new(),
            aliases: contract.aliases.clone(),
            scratch: contract.required_scratch.clone(),
            addresses: contract.address_facts.clone(),
            tail: contract.tail,
            available_values: BTreeSet::new(),
            slot_requests: BTreeSet::new(),
            next_occurrence: 0,
            next_temporary: BTreeMap::new(),
        };
        for atom in contract.stack_top_first.iter().rev() {
            state.push(*atom)?;
        }
        for (slot, atom) in &contract.required_spill_slots {
            if state
                .slots
                .insert(*slot, StoredValue::Atom(*atom))
                .is_some_and(|old| old != StoredValue::Atom(*atom))
            {
                return Err(invariant(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "conflicting required spill contents",
                ));
            }
            state.available_values.insert(atom.value);
            state.slot_requests.insert(*slot);
        }
        for (a, b) in contract.aliases.pairs() {
            state.available_values.extend([a.value, b.value]);
        }
        Ok(state)
    }

    /// Iterate semantic lane names in physical top-first order, retaining duplicates.
    pub fn stack_atoms(&self) -> impl Iterator<Item = Atom> + '_ {
        self.stack.iter().map(|o| o.atom)
    }

    /// Publish a new top occurrence and make its value's checked reconstruction available.
    /// Identity exhaustion leaves the state unchanged.
    pub(super) fn push(&mut self, atom: Atom) -> Result<(), AllocationError> {
        let next = self
            .next_occurrence
            .checked_add(1)
            .ok_or(AllocationError::IdentifierExhausted("stack occurrence namespace exhausted"))?;
        self.stack.push_front(Occurrence {
            id: OccurrenceId(self.next_occurrence),
            atom,
        });
        self.next_occurrence = next;
        self.available_values.insert(atom.value);
        Ok(())
    }

    /// Reserve an unused symbolic cell in `owner`'s namespace without assigning contents.
    /// Existing requests are skipped even if they no longer have known contents.
    pub(super) fn fresh_slot(&mut self, owner: SlotOwner) -> Result<SpillSlotId, AllocationError> {
        let mut index = self.next_temporary.get(&owner).copied().unwrap_or(0);
        loop {
            let next = index.checked_add(1).ok_or(AllocationError::IdentifierExhausted(
                "spill temporary namespace exhausted",
            ))?;
            let slot = SpillSlotId { owner, index };
            if !self.slot_requests.contains(&slot) && !self.slots.contains_key(&slot) {
                self.next_temporary.insert(owner, next);
                self.slot_requests.insert(slot);
                return Ok(slot);
            }
            index = next;
        }
    }
}

/// Immutable selected identities and demand shared by all trials for one procedure.
///
/// The context controls which reconstructions may be used; physical availability remains
/// in each trial's state. Options bound optimization effort without changing feasibility.
pub(crate) struct AllocationContext<'a> {
    /// Only source-authorized alternatives; synthetic requests retain their canonical order.
    pub(super) input_symmetries: BTreeMap<SelectedStepId, InputSymmetry>,
    /// Layout and checked literal/alias reconstruction for each selected value.
    pub values: &'a BTreeMap<ValueId, SelectedValue>,
    /// Procedure-wide distances measured in selected unit steps.
    pub next_use: &'a NextUses,
    /// Scheduling policy copied into each allocation run.
    pub options: AllocationOptions,
    /// Reconstructible definitions that dominate an entry before its first unit executes.
    pub(super) entry_reconstructions: BTreeMap<SegmentId, BTreeSet<ValueId>>,
}

impl<'a> AllocationContext<'a> {
    /// Reestablish source-checked reconstructions only where their definitions dominate entry.
    /// A parameter is defined at entry; a unit in that same segment has not executed yet.
    pub fn new(program: &'a CheckedSelectedProgram, analysis: &'a AllocationAnalysis) -> Self {
        use petgraph::{algo::dominators::simple_fast, graph::DiGraph};

        let mut graph = DiGraph::<SegmentId, ()>::new();
        let nodes: BTreeMap<_, _> = program
            .segments
            .iter()
            .map(|segment| (segment.id, graph.add_node(segment.id)))
            .collect();
        for edge in &program.edges {
            if let Some(destination) = edge.destination {
                graph.add_edge(nodes[&edge.source], nodes[&destination], ());
            }
        }
        let dominators = simple_fast(&graph, nodes[&program.segments[0].id]);
        let definitions: BTreeMap<_, _> = program
            .segments
            .iter()
            .flat_map(|segment| {
                segment.parameters.iter().map(|value| (*value, (segment.id, true))).chain(
                    segment.units.iter().flat_map(|unit| {
                        unit.outputs.iter().map(|atom| (atom.value, (segment.id, false)))
                    }),
                )
            })
            .collect();
        let entry_reconstructions = program
            .segments
            .iter()
            .map(|segment| {
                let dominating: BTreeSet<_> = dominators
                    .dominators(nodes[&segment.id])
                    .into_iter()
                    .flatten()
                    .map(|node| graph[node])
                    .collect();
                let eligible = program
                    .values
                    .values()
                    .filter(|value| value.reconstruction.is_some())
                    .filter(|value| {
                        let (owner, parameter) = definitions[&value.id];
                        dominating.contains(&owner) && (owner != segment.id || parameter)
                    })
                    .map(|value| value.id)
                    .collect();
                (segment.id, eligible)
            })
            .collect();
        Self {
            values: &program.values,
            next_use: &analysis.next_use,
            options: AllocationOptions::default(),
            input_symmetries: program
                .segments
                .iter()
                .flat_map(|segment| &segment.units)
                .filter_map(|unit| {
                    let symmetry =
                        program.input_symmetry(unit.id).expect("checked source covers every unit");
                    (symmetry != InputSymmetry::Ordered).then_some((unit.id, symmetry))
                })
                .collect(),
            entry_reconstructions,
        }
    }

    /// Look up entry-scope reconstruction eligibility; absence is an invalid graph scope.
    pub(super) fn reconstructions_at_entry(
        &self,
        segment: SegmentId,
    ) -> Result<&BTreeSet<ValueId>, AllocationError> {
        self.entry_reconstructions.get(&segment).ok_or_else(|| {
            invariant(DiagnosticCategory::InvalidEdgeTransfer, "unknown reconstruction entry scope")
        })
    }

    /// Follow available directed reconstructions, checking lanes and cycles on every hop.
    /// An unavailable definition stays an atom even when it has a reconstruction recipe.
    fn reconstruction(
        &self,
        state: &AllocationState,
        mut atom: Atom,
    ) -> Result<StoredValue, AllocationError> {
        let mut seen = BTreeSet::new();
        loop {
            let value = self.values.get(&atom.value).ok_or_else(|| {
                invariant(DiagnosticCategory::UnavailableValue, "unknown semantic value")
            })?;
            if atom.lane >= value.layout.lanes() || !seen.insert(atom) {
                return Err(invariant(
                    DiagnosticCategory::UnavailableValue,
                    "invalid lane or cyclic reconstruction",
                ));
            }
            if state.available_values.contains(&atom.value) {
                match &value.reconstruction {
                    Some(Reconstruction::Literal(lanes)) => {
                        return lanes
                            .get(usize::from(atom.lane))
                            .copied()
                            .map(StoredValue::Literal)
                            .ok_or_else(|| {
                                invariant(
                                    DiagnosticCategory::UnavailableValue,
                                    "missing literal lane",
                                )
                            });
                    }
                    Some(Reconstruction::Alias(lanes)) => {
                        atom = *lanes.get(usize::from(atom.lane)).ok_or_else(|| {
                            invariant(DiagnosticCategory::UnavailableValue, "missing alias lane")
                        })?;
                        continue;
                    }
                    None => {}
                }
            }
            return Ok(StoredValue::Atom(atom));
        }
    }

    /// Contract aliases are undirected equalities, including redundant/identity pairs.
    /// Checked reconstruction is directed and acyclic; normalize it before finding the
    /// alias component so every spelling of one current token gets the same source key.
    /// A component resolves to its literal, or its least atom if it has no literal;
    /// contradictory literals and invalid reconstruction chains are rejected.
    pub(super) fn source(
        &self,
        state: &AllocationState,
        atom: Atom,
    ) -> Result<StoredValue, AllocationError> {
        let initial = self.reconstruction(state, atom)?;
        if state.aliases.pairs().is_empty() {
            return Ok(initial);
        }
        let pairs: Vec<_> = state
            .aliases
            .pairs()
            .iter()
            .map(|(a, b)| Ok((self.reconstruction(state, *a)?, self.reconstruction(state, *b)?)))
            .collect::<Result<_, AllocationError>>()?;
        let mut component = alloc::vec![initial];
        let mut cursor = 0;
        while cursor < component.len() {
            let current = component[cursor];
            for (a, b) in &pairs {
                let other = if *a == current {
                    Some(*b)
                } else if *b == current {
                    Some(*a)
                } else {
                    None
                };
                if let Some(other) = other
                    && !component.contains(&other)
                {
                    component.push(other);
                }
            }
            cursor += 1;
        }
        let mut canonical: Option<Atom> = None;
        let mut literal = None;
        for value in component {
            match value {
                StoredValue::Literal(value) => {
                    if literal.is_some_and(|old| old != value) {
                        return Err(invariant(
                            DiagnosticCategory::UnavailableValue,
                            "alias joins distinct literal identities",
                        ));
                    }
                    literal = Some(value);
                }
                StoredValue::Atom(value) => {
                    canonical = Some(canonical.map_or(value, |old| old.min(value)))
                }
            }
        }
        Ok(match literal {
            Some(literal) => StoredValue::Literal(literal),
            None => StoredValue::Atom(canonical.expect("a component has at least one source")),
        })
    }

    /// Compare current semantic identities, including scoped reconstruction and aliases.
    /// This proves equality, not the existence of a stack or stored copy.
    pub(super) fn same(
        &self,
        state: &AllocationState,
        a: StoredValue,
        b: StoredValue,
    ) -> Result<bool, AllocationError> {
        let resolve = |value| match value {
            StoredValue::Atom(atom) => self.source(state, atom),
            literal => Ok(literal),
        };
        Ok(resolve(a)? == resolve(b)?)
    }
}

/// A complete proposal owning every speculative action, identity, request, and binding.
///
/// Dropping it rejects the proposal without changing the caller. The actions establish
/// the returned state; they do not include execution of the lowering being prepared.
#[derive(Debug)]
pub(crate) struct Preparation {
    /// Successful scheduling strategy, for inspection rather than execution.
    pub tier: SchedulingTier,
    /// Actual selective ranking outcomes; empty for preparations without evictions.
    pub victims: Vec<crate::plan::VictimDecision>,
    /// Preparation actions in execution order.
    pub actions: Vec<PlanAction>,
    /// Facts after these actions; unit preparations stop before lowering input consumption.
    pub state: AllocationState,
    /// Symbolic stored-input cells reserved for opaque lowering expansions.
    pub bindings: BTreeMap<SelectedStepId, LoweringBindings>,
    /// Complete cell requests carried by the resulting state.
    pub slot_requests: BTreeSet<SpillSlotId>,
}

impl Preparation {
    /// Seal a private trial; optimized callers may replace the default normalization tier.
    pub(super) fn finish(trial: Trial<'_>) -> Self {
        let slot_requests = trial.state.slot_requests.clone();
        Self {
            tier: SchedulingTier::Normalization,
            victims: Vec::new(),
            actions: trial.actions,
            state: trial.state,
            bindings: trial.bindings,
            slot_requests,
        }
    }

    /// Consume a successful proposal and publish all of its products together.
    ///
    /// This operation cannot fail. Callers commit each preparation once, into the action
    /// stream and binding table for which it was constructed.
    pub fn commit(
        self,
        state: &mut AllocationState,
        actions: &mut Vec<PlanAction>,
        bindings: &mut BTreeMap<SelectedStepId, LoweringBindings>,
        slots: &mut BTreeSet<SpillSlotId>,
    ) {
        actions.extend(self.actions);
        bindings.extend(self.bindings);
        slots.extend(self.slot_requests);
        *state = self.state;
    }
}
