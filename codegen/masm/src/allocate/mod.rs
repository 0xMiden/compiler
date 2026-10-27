//! Turn checked selected units into a physical stack and symbolic-storage plan.
//!
//! Demand analysis drives immutable entry contracts. Forward predecessors are planned
//! before their successors; each contract is frozen before its body is prepared once.
//! Loop headers exclude backedges from that ordering, then repair backedges against the
//! fixed contract after body planning. Lowerings stay opaque and publish only their
//! selected normal-completion effects; nonreturning units have no outgoing physical state.
//!
//! Preparations own speculative changes until committed. This phase requests symbolic
//! spill cells without assigning addresses; `slots::assign_frame` lays out the completed
//! plan later. Physical verification independently checks the resulting plan and expansion.
pub(crate) mod analysis;
pub(crate) mod contracts;
pub(crate) mod edges;
pub(crate) mod normalize;
pub(crate) mod schedule;
pub(crate) mod slots;
pub(crate) mod state;
pub(crate) mod storage;

#[cfg(test)]
mod tests;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use analysis::{UseDistances, analyze};
use contracts::Incoming;
use schedule::{AllocationOptions, prepare_control, prepare_unit};
use state::*;

use crate::{
    plan::{
        actions::{PlanAction, ProcedurePlan},
        *,
    },
    verify::CheckedSelectedProgram,
};

/// Test instrumentation for contract/body ordering and ready-order independence.
#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct PlanningStats {
    /// Planning milestones in the order they occurred.
    pub events: Vec<PlanningEvent>,
    /// Choose the greatest ready segment ID instead of the least during the test run.
    pub reverse_ready: bool,
}

/// Milestones used to prove headers freeze before bodies and cyclic repairs follow them.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PlanningEvent {
    Freeze(SegmentId),
    Body(SegmentId),
    Backedge(EdgeId),
}

/// Consume checked selection and construct contracts, preparation actions, and spill requests.
///
/// Entry and return lanes follow the checked ABI's top-first order. The caller's tail is
/// protected according to its calling convention. Allocation errors report an impossible
/// physical contract, inconsistent ABI, or exhausted symbolic identity namespace.
/// Expansion, frame assignment, and independent physical verification follow this phase.
pub(crate) fn allocate(
    program: CheckedSelectedProgram,
    options: &AllocationOptions,
) -> Result<ProcedurePlan, AllocationError> {
    allocate_inner(
        program,
        options,
        #[cfg(test)]
        &mut PlanningStats::default(),
    )
}

/// Freeze forward entries and plan each reachable body once, then finalize cyclic/exit repairs.
fn allocate_inner(
    program: CheckedSelectedProgram,
    options: &AllocationOptions,
    #[cfg(test)] stats: &mut PlanningStats,
) -> Result<ProcedurePlan, AllocationError> {
    let analysis = analyze(&program);
    let mut context = AllocationContext::new(&program, &analysis);
    context.options = options.clone();
    let mut controls = BTreeMap::new();
    let mut headers = BTreeMap::new();
    let mut backedges = BTreeSet::new();
    /// Index control-predicate consumption, loop headers, and the edges excluded from
    /// forward readiness; checked structure guarantees every referenced edge exists.
    fn visit(
        tree: &ControlTree,
        edges: &[TransferEdge],
        controls: &mut BTreeMap<SegmentId, (Atom, EdgeId)>,
        headers: &mut BTreeMap<SegmentId, ControlId>,
        backedges: &mut BTreeSet<EdgeId>,
    ) {
        match tree {
            ControlTree::If {
                predicate,
                on_true,
                then_body,
                else_body,
                ..
            } => {
                let source = edges.iter().find(|edge| edge.id == *on_true).unwrap().source;
                controls.insert(source, (*predicate, *on_true));
                visit(then_body, edges, controls, headers, backedges);
                visit(else_body, edges, controls, headers, backedges);
            }
            ControlTree::Loop {
                id,
                header,
                predicate,
                on_true,
                before,
                body,
                backedges: cyclic,
                ..
            } => {
                let source = edges.iter().find(|edge| edge.id == *on_true).unwrap().source;
                controls.insert(source, (*predicate, *on_true));
                headers.insert(*header, *id);
                backedges.extend(cyclic);
                visit(before, edges, controls, headers, backedges);
                visit(body, edges, controls, headers, backedges);
            }
            ControlTree::Sequence(children) => {
                for child in children {
                    visit(child, edges, controls, headers, backedges);
                }
            }
            _ => {}
        }
    }
    visit(&program.structure, &program.edges, &mut controls, &mut headers, &mut backedges);
    let entry = program.segments[0].id;
    let mut contracts = BTreeMap::new();
    let mut segment_actions = BTreeMap::new();
    let mut edge_actions = BTreeMap::new();
    let mut slots = BTreeSet::new();
    let mut bindings = BTreeMap::new();
    let mut semantic_storage_reads = BTreeMap::new();
    let mut debug_locations = BTreeMap::new();
    let mut decisions = PlanDecisions::default();
    let mut outgoing = BTreeMap::<SegmentId, AllocationState>::new();
    let mut pending = analysis.reachable.clone();
    while !pending.is_empty() {
        let ready = if pending.contains(&entry) {
            Some(entry)
        } else {
            let mut ready = pending.iter().copied().filter(|id| {
                program
                    .edges
                    .iter()
                    .filter(|edge| {
                        edge.destination == Some(*id)
                            && !backedges.contains(&edge.id)
                            && analysis.reachable.contains(&edge.source)
                    })
                    .all(|edge| outgoing.contains_key(&edge.source))
            });
            #[cfg(test)]
            let next = if stats.reverse_ready {
                ready.next_back()
            } else {
                ready.next()
            };
            #[cfg(not(test))]
            let next = ready.next();
            next
        };
        let id = ready.ok_or_else(|| {
            invariant(
                DiagnosticCategory::InvalidEdgeTransfer,
                "selected forward graph cannot be planned once",
            )
        })?;
        pending.remove(&id);
        let segment = program.segments.iter().find(|s| s.id == id).unwrap();
        let contract = if id == entry {
            let stack: Vec<_> = segment
                .parameters
                .iter()
                .flat_map(|value| {
                    (0..program.values[value].layout.lanes())
                        .map(move |lane| Atom::new(*value, lane))
                })
                .collect();
            if stack.len() != program.abi.arguments() as usize {
                return Err(invariant(
                    DiagnosticCategory::AbiMismatch,
                    "entry parameters disagree with ABI",
                ));
            }
            if program.abi.convention() == CallingConvention::Root
                && (program.abi.arguments() != 16 || program.abi.results() != 16)
            {
                return Err(invariant(
                    DiagnosticCategory::AbiMismatch,
                    "root must model all sixteen lanes",
                ));
            }
            decisions.entries.insert(
                id,
                EntryDecision {
                    origin: match program.abi.convention() {
                        CallingConvention::Root => "root",
                        _ => "procedure-entry",
                    },
                    costs: vec![],
                },
            );
            EntryContract {
                stack_top_first: stack,
                required_spill_slots: vec![],
                required_scratch: vec![],
                address_facts: vec![],
                aliases: LaneAliases::default(),
                tail: match program.abi.convention() {
                    CallingConvention::SameContext => TailPolicy::PreserveSuffix,
                    CallingConvention::Root => TailPolicy::RootWindow,
                    _ => TailPolicy::ContextWindow,
                },
            }
        } else if program.return_cleanup(id) {
            decisions.entries.insert(
                id,
                EntryDecision {
                    origin: "context-return",
                    costs: vec![],
                },
            );
            EntryContract {
                stack_top_first: segment.units[0].stack_inputs.clone(),
                required_spill_slots: vec![],
                required_scratch: vec![],
                address_facts: vec![],
                aliases: LaneAliases::default(),
                tail: TailPolicy::ContextWindow,
            }
        } else {
            let mut incoming: Vec<_> = program
                .edges
                .iter()
                .filter(|edge| {
                    edge.destination == Some(id)
                        && !backedges.contains(&edge.id)
                        && analysis.reachable.contains(&edge.source)
                })
                .map(|edge| Incoming {
                    edge,
                    state: &outgoing[&edge.source],
                })
                .collect();
            incoming.sort_by_key(|entry| entry.edge.id);
            let choice = contracts::choose(
                &program,
                &analysis,
                &context,
                id,
                &incoming,
                headers.get(&id).map(|control| &analysis.loops[control]),
            )?;
            decisions.entries.insert(
                id,
                EntryDecision {
                    origin: choice.origin,
                    costs: choice.costs,
                },
            );
            for (edge, repair) in choice.repairs {
                slots.extend(repair.slot_requests);
                edge_actions.insert(edge, repair.actions);
            }
            choice.contract
        };
        #[cfg(test)]
        stats.events.push(PlanningEvent::Freeze(id));
        let mut state = AllocationState::at_entry(&contract, id, &context)?;
        slots.extend(state.slot_requests.iter().copied());
        contracts.insert(id, contract);
        let mut actions = vec![];
        if id == entry
            && matches!(
                program.abi.convention(),
                CallingConvention::Context | CallingConvention::Kernel
            )
        {
            actions.extend(context_entry_cleanup(program.abi.arguments() as u8)?);
        }
        #[cfg(test)]
        stats.events.push(PlanningEvent::Body(id));
        let mut normal = true;
        for (position, unit) in segment.units.iter().enumerate() {
            let at_unit = |error: AllocationError| {
                error.at_step(
                    unit.id,
                    program.source_map.get(&unit.source).map(|source| &source.location),
                )
            };
            observe(&program, id, position, &state, &context, &mut actions, &mut debug_locations)
                .map_err(at_unit)?;
            let live = analysis.after[&unit.id].iter().collect();
            let preparation = prepare_unit(&state, unit, &live, &context).map_err(at_unit)?;
            decisions.preparations.insert(
                unit.id,
                PreparationDecision::new(
                    preparation.tier,
                    preparation.victims.clone(),
                    &preparation.actions,
                ),
            );
            preparation.commit(&mut state, &mut actions, &mut bindings, &mut slots);
            let reads = context.semantic_storage_reads(&state, unit).map_err(at_unit)?;
            if !reads.is_empty() {
                semantic_storage_reads.insert(unit.id, reads);
            }
            actions.push(PlanAction::RunLowering { step: unit.id });
            if !unit.effects.normal_return {
                normal = false;
                break;
            }
            state.complete(unit).map_err(at_unit)?;
        }
        if normal {
            observe(
                &program,
                id,
                segment.units.len(),
                &state,
                &context,
                &mut actions,
                &mut debug_locations,
            )?;
            if let Some((predicate, edge)) = controls.get(&id) {
                let mut live = BTreeSet::new();
                let mut next_use = UseDistances::default();
                for outgoing in program.edges.iter().filter(|edge| edge.source == id) {
                    live.extend(analysis.edge_live[&outgoing.id].iter());
                    if let Some(uses) = context.next_use.edge.get(&outgoing.id) {
                        next_use.merge(uses);
                    }
                }
                let preparation =
                    prepare_control(&state, *predicate, *edge, &live, &next_use, &context)
                        .map_err(|error| {
                            error.context(alloc::format!(
                                "control predicate {predicate} in b{} for e{}",
                                id.0,
                                edge.0
                            ))
                        })?;
                decisions.controls.insert(
                    id,
                    (
                        actions.len(),
                        *edge,
                        PreparationDecision::new(
                            preparation.tier,
                            preparation.victims.clone(),
                            &preparation.actions,
                        ),
                    ),
                );
                preparation.commit(&mut state, &mut actions, &mut bindings, &mut slots);
                state.stack.pop_front().ok_or_else(|| {
                    invariant(DiagnosticCategory::UnavailableValue, "missing selected predicate")
                })?;
            }
            outgoing.insert(id, state);
        }
        segment_actions.insert(id, actions);
    }
    let mut edges: Vec<_> = program.edges.iter().collect();
    edges.sort_by_key(|edge| edge.id);
    for edge in edges {
        if edge_actions.contains_key(&edge.id) {
            continue;
        }
        let Some(state) = outgoing.get(&edge.source) else {
            edge_actions.insert(edge.id, vec![]);
            continue;
        };
        let repair = if let Some(destination) = edge.destination {
            #[cfg(test)]
            stats.events.push(PlanningEvent::Backedge(edge.id));
            Some(edges::repair_edge(state, edge, &contracts[&destination], &context)?)
        } else if edge.kind == EdgeKind::Return {
            let stack: Vec<_> = edge
                .bindings
                .iter()
                .flat_map(|binding| {
                    (0..program.values[&binding.source].layout.lanes())
                        .map(move |lane| Atom::new(binding.source, lane))
                })
                .collect();
            if stack.len() != program.abi.results() as usize {
                return Err(invariant(
                    DiagnosticCategory::AbiMismatch,
                    "return width disagrees with ABI",
                ));
            }
            let target = EntryContract {
                stack_top_first: stack,
                required_spill_slots: vec![],
                required_scratch: vec![],
                address_facts: vec![],
                aliases: LaneAliases::default(),
                tail: state.tail,
            };
            let mut transfer = edge.clone();
            transfer.bindings.clear();
            transfer.content_bindings.clear();
            Some(edges::repair_edge(state, &transfer, &target, &context)?)
        } else {
            None
        };
        if let Some(repair) = repair {
            slots.extend(repair.slot_requests);
            edge_actions.insert(edge.id, repair.actions);
        } else {
            edge_actions.insert(edge.id, vec![]);
        }
    }
    Ok(ProcedurePlan {
        decisions,
        debug_locations,
        selected: program,
        contracts,
        segment_actions,
        edge_actions,
        spill_slots: slots.into_iter().collect(),
        lowering_bindings: bindings,
        semantic_storage_reads,
    })
}

/// A callee discards its unspecified argument-window lanes just as a caller discards
/// unspecified result-window lanes. Reuse the same bounded cost comparison, translating
/// its closed stack vocabulary into independently replayable allocation actions.
fn context_entry_cleanup(arguments: u8) -> Result<Vec<PlanAction>, PlanError> {
    use crate::masm::Instruction::*;

    let mut actions = Vec::new();
    for instruction in context_result_cleanup(arguments)? {
        let action = match instruction {
            Drop => PlanAction::Drop,
            DropW => {
                actions.extend(core::iter::repeat_n(PlanAction::Drop, 4));
                continue;
            }
            Swap1 => PlanAction::Swap { depth: 1 },
            MovUp2 => PlanAction::MoveUp { depth: 2 },
            MovUp3 => PlanAction::MoveUp { depth: 3 },
            MovUp4 => PlanAction::MoveUp { depth: 4 },
            MovUp5 => PlanAction::MoveUp { depth: 5 },
            MovUp6 => PlanAction::MoveUp { depth: 6 },
            MovUp7 => PlanAction::MoveUp { depth: 7 },
            MovUp8 => PlanAction::MoveUp { depth: 8 },
            MovUp9 => PlanAction::MoveUp { depth: 9 },
            MovUp10 => PlanAction::MoveUp { depth: 10 },
            MovUp11 => PlanAction::MoveUp { depth: 11 },
            MovUp12 => PlanAction::MoveUp { depth: 12 },
            MovUp13 => PlanAction::MoveUp { depth: 13 },
            MovUp14 => PlanAction::MoveUp { depth: 14 },
            MovUp15 => PlanAction::MoveUp { depth: 15 },
            MovDn15 => PlanAction::MoveDown { depth: 15 },
            SwapW1 => PlanAction::SwapWord { word: 1 },
            SwapW2 => PlanAction::SwapWord { word: 2 },
            SwapW3 => PlanAction::SwapWord { word: 3 },
            _ => unreachable!("context cleanup only rearranges and drops window lanes"),
        };
        actions.push(action);
    }
    Ok(actions)
}

/// Snapshot only lanes mentioned by source markers at this exact completed program point.
/// Existing aliases and valid cells may describe a value, but observations never request storage.
fn observe(
    program: &CheckedSelectedProgram,
    segment: SegmentId,
    position: usize,
    state: &AllocationState,
    context: &AllocationContext<'_>,
    actions: &mut Vec<PlanAction>,
    records: &mut BTreeMap<DebugLocationId, crate::lower::debug::AllocatedObservation>,
) -> Result<(), AllocationError> {
    use crate::lower::debug::{AllocatedObservation, Location};
    for observation in program
        .observations()
        .iter()
        .filter(|o| o.segment == segment && o.position == position)
    {
        let mut locations = vec![];
        for atom in observation.lanes() {
            let required = StoredValue::Atom(*atom);
            let mut found = Location::Unavailable;
            for (depth, occurrence) in state.stack.iter().take(16).enumerate() {
                if context.same(state, required, StoredValue::Atom(occurrence.atom))? {
                    found = Location::Stack(depth as u8);
                    break;
                }
            }
            if found == Location::Unavailable {
                for (slot, value) in &state.slots {
                    if context.same(state, required, *value)? {
                        found = Location::Cell(*slot);
                        break;
                    }
                }
            }
            locations.push(found);
        }
        let id = DebugLocationId(
            u32::try_from(records.len())
                .map_err(|_| AllocationError::exhausted("debug location"))?,
        );
        records.insert(
            id,
            AllocatedObservation {
                observation: observation.clone(),
                locations,
            },
        );
        actions.push(PlanAction::DebugLocation(id));
    }
    Ok(())
}
