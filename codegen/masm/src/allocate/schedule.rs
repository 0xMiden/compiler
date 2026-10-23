//! Prepare lowering inputs with increasingly general private scheduling strategies.
//!
//! Try resident stack movement, missing-input loads, and selective top-window eviction
//! before constructive normalization. Grouped search is optional: split layouts, exhausted
//! search fuel, or an unusable proposal fall through to a more general strategy. Every
//! accepted proposal preserves semantic inputs and values live through input consumption.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::cmp::Reverse;

use super::{
    analysis::NextUseDistance,
    normalize::{self, UnitRequirements},
    state::*,
    storage::Trial,
};
use crate::{
    Constraint,
    opt::operands::{
        Action, OperandMovementConstraintSolver, SolverError, SolverOptions, ValueOrAlias,
    },
    plan::{actions::PlanAction, *},
};

/// Strategy that produced a preparation, in the order tried by the scheduler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SchedulingTier {
    /// Use the current owned occurrences, with optional grouped movement or duplication.
    Stack,
    /// Load or reconstruct missing inputs before trying stack movement.
    Loads,
    /// Retain and evict individual raw lanes within the accessible sixteen-lane window.
    Selective,
    /// Drain the owned stack to safe backing storage and rebuild the input prefix.
    Normalization,
}

/// Limits for optional scheduling search; constructive fallback remains available.
#[derive(Debug, Clone)]
pub(crate) struct AllocationOptions {
    /// Fuel passed to each grouped operand-movement solver invocation.
    pub shuffle_fuel: usize,
    #[cfg(test)]
    /// Restrict optimized unit attempts to one tier; normalization is still the fallback.
    pub force_tier: Option<SchedulingTier>,
    #[cfg(test)]
    /// Make grouped search decline immediately to exercise constructive paths.
    pub refuse_solver: bool,
}
impl Default for AllocationOptions {
    fn default() -> Self {
        Self {
            shuffle_fuel: 40,
            #[cfg(test)]
            force_tier: None,
            #[cfg(test)]
            refuse_solver: false,
        }
    }
}

/// Establish the unit's top-first input prefix and protected stored-input bindings.
///
/// Each strategy owns its trial, including fresh identities and storage requests. The
/// first valid optimized result wins; refusal or failure falls through to normalization
/// from the original state. No actions are committed and the lowering itself is not run.
/// `live_after` is semantic demand after normal completion, including all lanes of values.
pub(crate) fn prepare_unit(
    state: &AllocationState,
    unit: &SelectedLowering,
    live_after: &BTreeSet<ValueId>,
    context: &AllocationContext<'_>,
) -> Result<Preparation, AllocationError> {
    let needs = normalize::requirements(unit, live_after, context)?;
    let mut base = Trial::new(state, context, SlotOwner::Unit(unit.id));
    if unit.publication == OutputPublication::Aliases {
        // Alias publication consumes nothing. Even forced fallback must not drain a buried
        // source simply to introduce another semantic name for its existing representation.
        if !valid(&base, unit, &needs)? {
            return Err(invariant(
                DiagnosticCategory::UnavailableValue,
                "alias publication source is not recoverable",
            ));
        }
        normalize::bind_inputs(&mut base, unit, &needs.needed)?;
        return Ok(Preparation::finish(base));
    }
    // A buried stored source can require a drain before it can be bound. A failed private
    // setup must not prevent that full construction from running on the original state.
    if normalize::bind_inputs(&mut base, unit, &needs.needed).is_ok() {
        for tier in [SchedulingTier::Stack, SchedulingTier::Loads, SchedulingTier::Selective] {
            #[cfg(test)]
            if context.options.force_tier.is_some_and(|forced| forced != tier) {
                continue;
            }
            let proposal = match tier {
                SchedulingTier::Stack => {
                    stack_candidate(base.clone(), unit, &needs).map(|p| p.map(|t| (t, Vec::new())))
                }
                SchedulingTier::Loads => {
                    load_candidates(&base, unit, &needs).map(|p| p.map(|t| (t, Vec::new())))
                }
                SchedulingTier::Selective => selective(base.clone(), unit, &needs),
                SchedulingTier::Normalization => unreachable!(),
            };
            if let Ok(Some((trial, victims))) = proposal {
                let mut preparation = Preparation::finish(trial);
                preparation.tier = tier;
                preparation.victims = victims;
                return Ok(preparation);
            }
        }
    }
    normalize::prepare_unit(state, unit, live_after, context)
}

/// Put a selected predicate on top while preserving demand on the outgoing edges.
///
/// The edge identity owns temporary storage. This is a prefix request with no executable
/// lowering or stored-input bindings; the caller consumes the predicate after committing.
pub(super) fn prepare_control(
    state: &AllocationState,
    predicate: Atom,
    edge: EdgeId,
    live_after: &BTreeSet<ValueId>,
    context: &AllocationContext<'_>,
) -> Result<Preparation, AllocationError> {
    let request = SelectedLowering {
        id: SelectedStepId(u32::MAX),
        kind: LoweringKind(0),
        source: SourceStepId(0),
        semantic_inputs: alloc::vec![predicate.value],
        stack_inputs: alloc::vec![predicate],
        stored_inputs: Vec::new(),
        outputs: Vec::new(),
        publication: OutputPublication::Stack,
        address_results: Vec::new(),
        effects: LoweringEffects::default(),
        scratch: Vec::new(),
        scratch_effects: Vec::new(),
    };
    let needs = normalize::requirements(&request, live_after, context)?;
    let base = Trial::new(state, context, SlotOwner::Edge(edge));
    for tier in [SchedulingTier::Stack, SchedulingTier::Loads, SchedulingTier::Selective] {
        let candidate = match tier {
            SchedulingTier::Stack => {
                stack_candidate(base.clone(), &request, &needs).map(|p| p.map(|t| (t, Vec::new())))
            }
            SchedulingTier::Loads => {
                load_candidates(&base, &request, &needs).map(|p| p.map(|t| (t, Vec::new())))
            }
            SchedulingTier::Selective => selective(base.clone(), &request, &needs),
            SchedulingTier::Normalization => unreachable!(),
        };
        if let Ok(Some((trial, victims))) = candidate {
            let mut result = Preparation::finish(trial);
            result.tier = tier;
            result.victims = victims;
            return Ok(result);
        }
    }
    let mut trial = base;
    trial.drain(&needs.needed)?;
    trial.load(predicate)?;
    Ok(Preparation::finish(trial))
}

/// Compare lane identities under this trial's available reconstructions and aliases.
fn same(trial: &Trial<'_>, a: Atom, b: Atom) -> Result<bool, AllocationError> {
    trial.context.same(&trial.state, StoredValue::Atom(a), StoredValue::Atom(b))
}

/// Whether a lane can be recovered without an existing stack occurrence.
fn memory(trial: &Trial<'_>, atom: Atom) -> Result<bool, AllocationError> {
    Ok(trial.slot_for(StoredValue::Atom(atom))?.is_some()
        || matches!(trial.context.source(&trial.state, atom)?, StoredValue::Literal(_)))
}

/// Check recoverability while excluding occurrences that a forthcoming consumption removes.
fn represented(
    trial: &Trial<'_>,
    atom: Atom,
    excluded: &BTreeSet<OccurrenceId>,
) -> Result<bool, AllocationError> {
    if memory(trial, atom)? {
        return Ok(true);
    }
    for occurrence in &trial.state.stack {
        if !excluded.contains(&occurrence.id) && same(trial, atom, occurrence.atom)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check the exact input prefix and that every survivor has a nonconsumed representation.
/// Semantic-only inputs need entry availability, but do not necessarily need a survivor copy.
fn valid(
    trial: &Trial<'_>,
    unit: &SelectedLowering,
    needs: &UnitRequirements,
) -> Result<bool, AllocationError> {
    if trial.state.stack.len() < unit.stack_inputs.len() {
        return Ok(false);
    }
    for (actual, expected) in trial.state.stack.iter().zip(&unit.stack_inputs) {
        if !same(trial, actual.atom, *expected)? {
            return Ok(false);
        }
    }
    for atom in &needs.needed {
        if !represented(trial, *atom, &BTreeSet::new())? {
            return Ok(false);
        }
    }
    let consumed = trial.state.stack.iter().take(unit.stack_inputs.len()).map(|o| o.id).collect();
    for atom in &needs.survivors {
        if !represented(trial, *atom, &consumed)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Apply an indexed raw shuffle to owned occurrences and append its action.
///
/// Scalar depth is zero-based; word indices name complete four-lane groups. Every accessed
/// lane must be owned and within the top sixteen.
/// Moves preserve occurrence identities; duplication creates a fresh identity. Nonshuffle
/// actions are a caller error. A rejected indexed access leaves the trial unchanged.
pub(super) fn shuffle(trial: &mut Trial<'_>, action: PlanAction) -> Result<(), AllocationError> {
    if let PlanAction::SwapWord { word } = action {
        let start = usize::from(word) * 4;
        if !(1..=3).contains(&word) || start + 4 > trial.state.stack.len() {
            return Err(invariant(
                DiagnosticCategory::IllegalStackAccess,
                "word shuffle requires complete owned addressable words",
            ));
        }
        for lane in 0..4 {
            trial.state.stack.swap(lane, start + lane);
        }
        trial.actions.push(action);
        return Ok(());
    }
    let depth = match action {
        PlanAction::Dup { depth }
        | PlanAction::Swap { depth }
        | PlanAction::MoveUp { depth }
        | PlanAction::MoveDown { depth } => usize::from(depth),
        _ => unreachable!("shuffle takes only indexed stack actions"),
    };
    if depth >= 16 || depth >= trial.state.stack.len() {
        return Err(invariant(
            DiagnosticCategory::IllegalStackAccess,
            "shuffle addresses a buried lane",
        ));
    }
    if depth == 0 && !matches!(action, PlanAction::Dup { .. }) {
        return Ok(());
    }
    match action {
        PlanAction::Dup { .. } => trial.state.push(trial.state.stack[depth].atom)?,
        PlanAction::Swap { .. } => trial.state.stack.swap(0, depth),
        PlanAction::MoveUp { .. } => {
            let occurrence = trial.state.stack.remove(depth).expect("checked depth");
            trial.state.stack.push_front(occurrence);
        }
        PlanAction::MoveDown { .. } => {
            let occurrence = trial.state.stack.pop_front().expect("checked depth");
            trial.state.stack.insert(depth, occurrence);
        }
        _ => unreachable!(),
    }
    trial.actions.push(action);
    Ok(())
}

/// Lift one owned raw lane by zero-based depth, rejecting inaccessible or overflowing depths.
pub(super) fn move_up(trial: &mut Trial<'_>, depth: usize) -> Result<(), AllocationError> {
    let depth = u8::try_from(depth)
        .map_err(|_| invariant(DiagnosticCategory::IllegalStackAccess, "shuffle depth overflow"))?;
    shuffle(trial, PlanAction::MoveUp { depth })
}

/// Complete contiguous layouts only. A partial or split representation bypasses the solver.
fn groups(atoms: &[Atom], context: &AllocationContext<'_>) -> Option<Vec<(usize, u8, ValueId)>> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < atoms.len() {
        let value = atoms[start].value;
        let width = context.values.get(&value)?.layout.lanes();
        let end = start.checked_add(usize::from(width))?;
        if end > atoms.len()
            || !(0..width)
                .zip(&atoms[start..end])
                .all(|(lane, atom)| *atom == Atom::new(value, lane))
        {
            return None;
        }
        result.push((start, width, value));
        start = end;
    }
    Some(result)
}

/// Expand grouped actions through raw moves. Unequal-width swaps rotate the top group
/// below the intervening groups, then lift the target group; no suffix lane participates.
fn replay(
    trial: &mut Trial<'_>,
    actions: &[Action],
    mut widths: Vec<usize>,
) -> Result<(), AllocationError> {
    for action in actions {
        let index = match *action {
            Action::Copy(i) | Action::Swap(i) | Action::MoveUp(i) | Action::MoveDown(i) => {
                usize::from(i)
            }
        };
        let Some(&width) = widths.get(index) else {
            return Err(invariant(DiagnosticCategory::IllegalStackAccess, "invalid grouped index"));
        };
        let end: usize = widths.iter().take(index + 1).sum();
        if end > 16 {
            return Err(invariant(
                DiagnosticCategory::IllegalStackAccess,
                "group includes a buried lane",
            ));
        }
        match *action {
            Action::Copy(_) => {
                for _ in 0..width {
                    shuffle(
                        trial,
                        PlanAction::Dup {
                            depth: (end - 1) as u8,
                        },
                    )?;
                }
                widths.insert(0, width);
            }
            Action::MoveUp(1) | Action::MoveDown(1) if width == 4 && widths[0] == 4 => {
                shuffle(trial, PlanAction::SwapWord { word: 1 })?;
                widths.swap(0, 1);
            }
            Action::MoveUp(_) => {
                for _ in 0..width {
                    move_up(trial, end - 1)?;
                }
                widths.remove(index);
                widths.insert(0, width);
            }
            Action::MoveDown(_) => {
                for _ in 0..widths[0] {
                    shuffle(
                        trial,
                        PlanAction::MoveDown {
                            depth: (end - 1) as u8,
                        },
                    )?;
                }
                let top = widths.remove(0);
                widths.insert(index, top);
            }
            Action::Swap(_)
                if index > 0 && width == 4 && widths[0] == 4 && (end - width).is_multiple_of(4) =>
            {
                shuffle(
                    trial,
                    PlanAction::SwapWord {
                        word: ((end - width) / 4) as u8,
                    },
                )?;
                widths.swap(0, index);
            }
            Action::Swap(_) if index > 0 && (width != 1 || widths[0] != 1) => {
                let top = widths[0];
                for _ in 0..top {
                    shuffle(
                        trial,
                        PlanAction::MoveDown {
                            depth: (end - 1) as u8,
                        },
                    )?;
                }
                for _ in 0..width {
                    move_up(trial, end - top - 1)?;
                }
                widths.swap(0, index);
            }
            Action::Swap(_) => {
                shuffle(
                    trial,
                    PlanAction::Swap {
                        depth: (end - 1) as u8,
                    },
                )?;
                widths.swap(0, index);
            }
        }
    }
    Ok(())
}

/// Adapt whole contiguous values to optional grouped search and replay privately.
///
/// `exact` requests occurrence identities rather than interchangeable semantic copies.
/// Unsupported layouts or solver refusal return `false`; successful replay replaces the
/// trial only after exact-target validation. Raw-access errors propagate to the caller.
fn solver(
    trial: &mut Trial<'_>,
    inputs: &[Atom],
    needs: &UnitRequirements,
    exact: Option<&[OccurrenceId]>,
) -> Result<bool, AllocationError> {
    #[cfg(test)]
    if trial.context.options.refuse_solver {
        return Ok(false);
    }
    let raw: Vec<_> = trial.state.stack_atoms().collect();
    let Some(stack_groups) = groups(&raw, trial.context) else {
        return Ok(false);
    };
    let Some(input_groups) = groups(inputs, trial.context) else {
        return Ok(false);
    };
    if stack_groups.len() > u8::MAX as usize || input_groups.len() > u8::MAX as usize {
        return Ok(false);
    }
    let ids: BTreeMap<_, _> = raw
        .iter()
        .map(|a| a.value)
        .chain(inputs.iter().map(|a| a.value))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(id, value)| (value, id as u32))
        .collect();
    let stack: Vec<_> = stack_groups
        .iter()
        .enumerate()
        .map(|(i, (_, width, value))| {
            ValueOrAlias::new(
                if exact.is_some() {
                    i as u32
                } else {
                    ids[value]
                },
                *width,
            )
        })
        .collect();
    let mut expected = Vec::new();
    let mut constraints = Vec::new();
    let mut seen = BTreeSet::new();
    for (start, width, value) in &input_groups {
        if let Some(target) = exact {
            let Some(index) = stack_groups.iter().position(|(source, source_width, _)| {
                source_width == width
                    && trial
                        .state
                        .stack
                        .iter()
                        .skip(*source)
                        .take(usize::from(*width))
                        .map(|o| o.id)
                        .eq(target.iter().skip(*start).take(usize::from(*width)).copied())
            }) else {
                return Ok(false);
            };
            expected.push(stack[index]);
            constraints.push(Constraint::Move);
        } else {
            let identity = ValueOrAlias::new(ids[value], *width);
            if !stack.contains(&identity) {
                return Ok(false);
            }
            let mut preserve = false;
            for atom in needs.survivors.iter().filter(|a| a.value == *value) {
                preserve |= !memory(trial, *atom)?;
            }
            constraints.push(if !seen.insert(*value) || preserve {
                Constraint::Copy
            } else {
                Constraint::Move
            });
            expected.push(identity);
        }
    }
    let options = SolverOptions {
        fuel: trial.context.options.shuffle_fuel,
        ..Default::default()
    };
    let actions = match OperandMovementConstraintSolver::from_layouts(
        &expected,
        &constraints,
        &stack,
        options,
    ) {
        Ok(solver) => match solver.solve() {
            Ok(actions) => actions,
            Err(_) => return Ok(false),
        },
        Err(SolverError::AlreadySolved) => Vec::new(),
        Err(SolverError::NoSolution) => return Ok(false),
    };
    let mut candidate = trial.clone();
    replay(
        &mut candidate,
        &actions,
        stack_groups.iter().map(|(_, width, _)| usize::from(*width)).collect(),
    )?;
    if let Some(target) = exact
        && !candidate
            .state
            .stack
            .iter()
            .take(target.len())
            .map(|o| o.id)
            .eq(target.iter().copied())
    {
        return Ok(false);
    }
    *trial = candidate;
    Ok(true)
}

/// Arrange distinct existing occurrence IDs in the requested top-first order.
///
/// Targets must already exist in the trial. Grouped search is optional and privately
/// replayed; otherwise reverse-order raw lifts construct the prefix. Every lift must
/// remain within the accessible sixteen lanes, or the caller must discard this trial.
pub(super) fn permute(
    trial: &mut Trial<'_>,
    target: &[OccurrenceId],
    needs: &UnitRequirements,
) -> Result<(), AllocationError> {
    if trial
        .state
        .stack
        .iter()
        .take(target.len())
        .map(|o| o.id)
        .eq(target.iter().copied())
    {
        return Ok(());
    }
    let atoms: Vec<_> = target
        .iter()
        .map(|id| trial.state.stack.iter().find(|o| o.id == *id).expect("target occurrence").atom)
        .collect();
    if !solver(trial, &atoms, needs, Some(target))? {
        for id in target.iter().rev() {
            let depth =
                trial.state.stack.iter().position(|o| o.id == *id).expect("target occurrence");
            move_up(trial, depth)?;
        }
    }
    Ok(())
}

/// Try grouped movement and each single word swap without loading or evicting lanes.
/// A bounded raw word candidate also works when split layouts exclude grouped search.
/// The grouped result wins equal-cost ties; every candidate proves the same full contract.
fn stack_candidate<'a>(
    trial: Trial<'a>,
    unit: &SelectedLowering,
    needs: &UnitRequirements,
) -> Result<Option<Trial<'a>>, AllocationError> {
    if valid(&trial, unit, needs)? {
        return Ok(Some(trial));
    }
    let mut grouped = trial.clone();
    let mut best = if solver(&mut grouped, &unit.stack_inputs, needs, None).unwrap_or(false)
        && valid(&grouped, unit, needs)?
    {
        Some(grouped)
    } else {
        None
    };
    for word in 1..=3 {
        let mut candidate = trial.clone();
        if shuffle(&mut candidate, PlanAction::SwapWord { word }).is_ok()
            && valid(&candidate, unit, needs)?
            && best.as_ref().is_none_or(|old| cost(&candidate) < cost(old))
        {
            best = Some(candidate);
        }
    }
    Ok(best)
}

/// Keep the first occurrence of each exact input name, preserving request order.
fn distinct(inputs: &[Atom]) -> Vec<Atom> {
    let mut seen = BTreeSet::new();
    inputs.iter().copied().filter(|atom| seen.insert(*atom)).collect()
}

/// Load distinct missing inputs from backing cells or literals, then try stack scheduling.
///
/// With `gather`, lift resident input occurrences before loading to keep them accessible.
/// Returns no candidate when all inputs are resident or an absent input has no backing
/// source. Each invocation owns its trial, so a declined candidate publishes no changes.
pub(super) fn load_candidate<'a>(
    mut trial: Trial<'a>,
    unit: &SelectedLowering,
    needs: &UnitRequirements,
    gather: bool,
) -> Result<Option<Trial<'a>>, AllocationError> {
    let mut resident = Vec::new();
    let mut absent = Vec::new();
    for atom in distinct(&unit.stack_inputs) {
        let mut found = None;
        for occurrence in trial.state.stack.iter().take(16) {
            if same(&trial, atom, occurrence.atom)? {
                found = Some(occurrence.id);
                break;
            }
        }
        if let Some(id) = found {
            resident.push(id);
        } else if memory(&trial, atom)? {
            absent.push(atom);
        } else {
            return Ok(None);
        }
    }
    if absent.is_empty() {
        return Ok(None);
    }
    if gather {
        for id in resident.into_iter().rev() {
            let depth =
                trial.state.stack.iter().position(|o| o.id == id).expect("resident occurrence");
            move_up(&mut trial, depth)?;
        }
    }
    for atom in absent.into_iter().rev() {
        trial.load(atom)?;
    }
    stack_candidate(trial, unit, needs)
}

/// Estimate before frame assignment: address generation takes two primitives, reads
/// one, and writes two (including the discarded result). Price indexed instructions by their
/// assembler expansion rather than treating each MASM mnemonic as one VM operation.
pub(super) fn cost(trial: &Trial<'_>) -> usize {
    action_cost(&trial.actions)
}

/// Estimate emitted primitive count for raw preparation actions before frame assignment.
/// Slot loads cost three and stores four. Stack instructions use the pinned assembler's costs;
/// packing noops and control-flow overhead remain outside this local ranking heuristic.
pub(super) fn action_cost(actions: &[PlanAction]) -> usize {
    actions
        .iter()
        .map(|action| match action {
            PlanAction::LoadLane(_) => 3,
            PlanAction::StoreLane(_) => 4,
            PlanAction::Dup { depth } => stack_cost(Primitive::Dup(*depth)),
            PlanAction::Swap { depth } => stack_cost(Primitive::Swap(*depth)),
            PlanAction::SwapWord { word } => stack_cost(Primitive::SwapWord(*word)),
            PlanAction::MoveUp { depth } => stack_cost(Primitive::MoveUp(*depth)),
            PlanAction::MoveDown { depth } => stack_cost(Primitive::MoveDown(*depth)),
            PlanAction::PushLiteral(value) => stack_cost(Primitive::Push(*value)),
            PlanAction::Drop | PlanAction::RunLowering { .. } | PlanAction::DebugLocation(_) => 1,
        })
        .sum()
}

fn stack_cost(primitive: Primitive) -> usize {
    let instruction = primitive.instruction().expect("prepared stack action has a legal depth");
    crate::opt::cost::stack_instruction_cost(&instruction)
        .expect("prepared stack instruction has a known expansion cost")
}

/// Compare direct loads with gather-then-loads, preferring direct loads on an equal cost.
/// Failed private attempts count as absent candidates, leaving constructive fallback open.
pub(super) fn load_candidates<'a>(
    base: &Trial<'a>,
    unit: &SelectedLowering,
    needs: &UnitRequirements,
) -> Result<Option<Trial<'a>>, AllocationError> {
    let first = load_candidate(base.clone(), unit, needs, false).ok().flatten();
    let second = load_candidate(base.clone(), unit, needs, true).ok().flatten();
    Ok(match (first, second) {
        (Some(first), Some(second)) => Some(if cost(&first) <= cost(&second) {
            first
        } else {
            second
        }),
        (first, second) => first.or(second),
    })
}

/// A lane to retain after the input prefix, possibly awaiting a new physical copy.
#[derive(Clone)]
struct Retained {
    atom: Atom,
    /// `None` reserves capacity for a survivor currently represented only by consumed pins.
    occurrence: Option<OccurrenceId>,
}

/// Classify redundancy after consuming input pins, while also keeping semantic-only
/// inputs available at unit entry. A pin is a source, never a live-after survivor.
/// Classes prefer redundant lanes (0), backed lanes (1), then lanes requiring a store (2).
fn victim_class(
    trial: &Trial<'_>,
    victim: usize,
    retained: &[Retained],
    pins: &BTreeSet<OccurrenceId>,
    needs: &UnitRequirements,
) -> Result<u8, AllocationError> {
    let candidate = &retained[victim];
    let mut entry_required = false;
    let mut after_required = false;
    for atom in &needs.needed {
        entry_required |= same(trial, candidate.atom, *atom)?;
    }
    for atom in &needs.survivors {
        after_required |= same(trial, candidate.atom, *atom)?;
    }
    let mut entry_stack = false;
    let mut after_stack = false;
    for occurrence in &trial.state.stack {
        if Some(occurrence.id) != candidate.occurrence
            && same(trial, candidate.atom, occurrence.atom)?
        {
            entry_stack = true;
            after_stack |= !pins.contains(&occurrence.id);
        }
    }
    for (index, other) in retained.iter().enumerate() {
        if index != victim && other.occurrence.is_none() && same(trial, candidate.atom, other.atom)?
        {
            entry_stack = true;
            after_stack = true;
        }
    }
    if (!entry_required || entry_stack) && (!after_required || after_stack) {
        Ok(0)
    } else if memory(trial, candidate.atom)? {
        Ok(1)
    } else {
        Ok(2)
    }
}

/// Fit the input prefix and survivor copies into sixteen accessible raw lanes.
///
/// Pin distinct resident inputs, rank retention victims by redundancy then farthest next
/// use, and back up only copies that would otherwise be lost. Buried owned occurrences
/// remain an exact suffix. Duplicate input requests receive distinct occurrence IDs.
/// Successful selective trial with only its actual victim decisions retained.
type SelectiveTrial<'a> = (Trial<'a>, Vec<crate::plan::VictimDecision>);
fn selective<'a>(
    mut trial: Trial<'a>,
    unit: &SelectedLowering,
    needs: &UnitRequirements,
) -> Result<Option<SelectiveTrial<'a>>, AllocationError> {
    if unit.stack_inputs.len() > 16 {
        return Ok(None);
    }
    let active: Vec<_> = trial.state.stack.iter().take(16).copied().collect();
    let suffix: Vec<_> = trial.state.stack.iter().skip(active.len()).copied().collect();
    let mut victims = Vec::new();
    let mut pins = BTreeSet::new();
    let mut assigned = Vec::new();
    for atom in &unit.stack_inputs {
        let mut source = None;
        let mut active_source = false;
        for occurrence in &active {
            if same(&trial, *atom, occurrence.atom)? {
                active_source = true;
                if !pins.contains(&occurrence.id) {
                    source = Some(occurrence.id);
                    break;
                }
            }
        }
        if !active_source && !memory(&trial, *atom)? {
            return Ok(None);
        }
        if let Some(id) = source {
            pins.insert(id);
        }
        assigned.push(source);
    }
    trial.consumed_inputs = pins.clone();
    let mut retained: Vec<_> = active
        .iter()
        .filter(|o| !pins.contains(&o.id))
        .map(|o| Retained {
            atom: o.atom,
            occurrence: Some(o.id),
        })
        .collect();
    for atom in &needs.survivors {
        if !represented(&trial, *atom, &pins)? {
            let mut reserved = false;
            for other in &retained {
                reserved |= same(&trial, *atom, other.atom)?;
            }
            if !reserved {
                retained.push(Retained {
                    atom: *atom,
                    occurrence: None,
                });
            }
        }
    }
    while unit.stack_inputs.len() + retained.len() > 16 {
        let mut ranked = Vec::new();
        for (index, candidate) in retained.iter().enumerate() {
            let uses = match trial.owner {
                SlotOwner::Edge(edge) => trial.context.next_use.edge.get(&edge),
                _ => trial.context.next_use.after.get(&unit.id),
            };
            let distance =
                uses.map_or(NextUseDistance::Infinite, |uses| uses.get(candidate.atom.value));
            ranked.push((
                victim_class(&trial, index, &retained, &pins, needs)?,
                Reverse(distance),
                candidate.atom,
                candidate.occurrence,
                index,
            ));
        }
        let (class, Reverse(distance), _, _, victim) =
            ranked.into_iter().min().expect("a capacity deficit has a retention victim");
        let candidate = retained.remove(victim);
        use crate::plan::{VictimDecision, VictimReason};
        victims.push(VictimDecision {
            atom: candidate.atom,
            next_use: distance,
            reason: match class {
                0 => VictimReason::Redundant,
                1 => VictimReason::Backed,
                _ => VictimReason::NeedsStore,
            },
        });
        if let Some(id) = candidate.occurrence {
            let depth =
                trial.state.stack.iter().position(|o| o.id == id).expect("retained occurrence");
            move_up(&mut trial, depth)?;
            if class == 2 {
                trial.back_up_top(&needs.needed)?;
            } else {
                trial.actions.push(PlanAction::Drop);
                trial.state.stack.pop_front();
            }
        } else if class == 2 {
            let value = StoredValue::Atom(candidate.atom);
            let slot = trial.safe_spill_slot(value, &needs.needed)?;
            trial.write(slot, value, &needs.needed)?;
        }
        // A new backing slot can make other survivor reservations unnecessary.
        let mut index = 0;
        while index < retained.len() {
            if retained[index].occurrence.is_none()
                && represented(&trial, retained[index].atom, &pins)?
            {
                retained.remove(index);
            } else {
                index += 1;
            }
        }
    }
    for (atom, assignment) in unit.stack_inputs.iter().zip(&mut assigned) {
        if assignment.is_none() {
            materialize_active(&mut trial, *atom, suffix.len())?;
            *assignment = Some(trial.state.stack[0].id);
        }
    }
    for survivor in &mut retained {
        if survivor.occurrence.is_none() {
            materialize_active(&mut trial, survivor.atom, suffix.len())?;
            survivor.occurrence = Some(trial.state.stack[0].id);
        }
    }
    let target: Vec<_> = assigned
        .into_iter()
        .map(Option::unwrap)
        .chain(retained.iter().map(|r| r.occurrence.unwrap()))
        .collect();
    permute(&mut trial, &target, needs)?;
    if !trial
        .state
        .stack
        .iter()
        .take(target.len())
        .map(|o| o.id)
        .eq(target.iter().copied())
        || !trial.state.stack.iter().skip(target.len()).copied().eq(suffix)
        || !valid(&trial, unit, needs)?
    {
        return Ok(None);
    }
    Ok(Some((trial, victims)))
}

/// Copy from the active prefix or backing storage, preserving the buried owned suffix.
/// `suffix_len` counts owned raw lanes excluded from active duplicate-source search.
fn materialize_active(
    trial: &mut Trial<'_>,
    atom: Atom,
    suffix_len: usize,
) -> Result<(), AllocationError> {
    let active_len = trial.state.stack.len() - suffix_len;
    for (depth, occurrence) in trial.state.stack.iter().take(active_len).enumerate() {
        if same(trial, atom, occurrence.atom)? {
            return shuffle(trial, PlanAction::Dup { depth: depth as u8 });
        }
    }
    if memory(trial, atom)? {
        trial.load(atom)
    } else {
        Err(invariant(
            DiagnosticCategory::UnavailableValue,
            "no active materialization source",
        ))
    }
}
