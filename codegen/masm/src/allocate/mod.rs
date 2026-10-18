//! Allocation owns its analyses independently of verification.
pub(crate) mod analysis;
pub(crate) mod contracts;
pub(crate) mod edges;
pub(crate) mod normalize;
pub(crate) mod schedule;
pub(crate) mod state;
pub(crate) mod storage;

#[cfg(test)]
mod tests;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use analysis::analyze;
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
                body,
                backedges: cyclic,
                ..
            } => {
                controls.insert(*header, (*predicate, *on_true));
                headers.insert(*header, *id);
                backedges.extend(cyclic);
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
            let arguments = program.abi.arguments() as u8;
            for _ in arguments..16 {
                if arguments == 1 {
                    actions.push(PlanAction::Swap { depth: 1 });
                } else if arguments > 1 {
                    actions.push(PlanAction::MoveUp { depth: arguments });
                }
                actions.push(PlanAction::Drop);
            }
        }
        #[cfg(test)]
        stats.events.push(PlanningEvent::Body(id));
        let mut normal = true;
        for unit in &segment.units {
            let live = analysis.after[&unit.id].iter().collect();
            prepare_unit(&state, unit, &live, &context)?.commit(
                &mut state,
                &mut actions,
                &mut bindings,
                &mut slots,
            );
            actions.push(PlanAction::RunLowering { step: unit.id });
            if !unit.effects.normal_return {
                normal = false;
                break;
            }
            state.complete(unit)?;
        }
        if normal {
            if let Some((predicate, edge)) = controls.get(&id) {
                let live = program
                    .edges
                    .iter()
                    .filter(|edge| edge.source == id)
                    .flat_map(|edge| analysis.edge_live[&edge.id].iter())
                    .collect();
                prepare_control(&state, *predicate, *edge, &live, &context)?.commit(
                    &mut state,
                    &mut actions,
                    &mut bindings,
                    &mut slots,
                );
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
        selected: program,
        contracts,
        segment_actions,
        edge_actions,
        spill_slots: slots.into_iter().collect(),
        lowering_bindings: bindings,
    })
}
