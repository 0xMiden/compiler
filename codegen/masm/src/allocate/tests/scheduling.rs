use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use midenc_hir::Type;
use proptest::prelude::*;

use super::{
    edges::{atom, context, state, unit, values},
    normalization::{DiscardDefinition, fixture_body, fixture_frame, verify_assemble_execute},
};
use crate::{
    allocate::{
        schedule::{AllocationOptions, SchedulingTier, prepare_unit},
        state::{AllocationContext, AllocationState, Preparation},
    },
    plan::{
        actions::{PlanAction, ProcedurePlan},
        *,
    },
    verify::*,
};

/// A small semantic fixture oracle: it executes raw actions on distinct parameter lanes,
/// with no allocator equality, recoverability, transition or solver helper calls.
fn execute(stack: &mut Vec<Atom>, slots: &mut BTreeMap<SpillSlotId, Atom>, actions: &[PlanAction]) {
    for action in actions {
        match *action {
            PlanAction::Dup { depth } => {
                assert!(depth < 16);
                stack.insert(0, stack[usize::from(depth)]);
            }
            PlanAction::Swap { depth } => {
                assert!(depth < 16);
                stack.swap(0, usize::from(depth));
            }
            PlanAction::MoveUp { depth } => {
                assert!(depth < 16);
                let atom = stack.remove(usize::from(depth));
                stack.insert(0, atom);
            }
            PlanAction::MoveDown { depth } => {
                assert!(depth < 16);
                let atom = stack.remove(0);
                stack.insert(usize::from(depth), atom);
            }
            PlanAction::Drop => {
                stack.remove(0);
            }
            PlanAction::StoreLane(slot) => {
                slots.insert(slot, stack.remove(0));
            }
            PlanAction::LoadLane(slot) => {
                stack.insert(0, slots[&slot]);
            }
            _ => panic!("this fixture has parameter lanes and raw scheduling actions only"),
        }
    }
}

/// Build a checked source fixture, schedule a discard and a later use of each live value,
/// and independently verify the expanded body. Initial spills/copies are real prelude actions.
fn checked_case(
    types: Vec<Type>,
    prelude: Vec<PlanAction>,
    mut request: SelectedLowering,
    live: BTreeSet<ValueId>,
    options: AllocationOptions,
) -> (ProcedurePlan, Preparation, AllocationState) {
    let all: Vec<_> = types
        .iter()
        .enumerate()
        .flat_map(|(id, ty)| {
            (0..ValueLayout::new(ty.clone()).unwrap().lanes())
                .map(move |lane| Atom::new(ValueId(id as u32), lane))
        })
        .collect();
    let later_inputs: Vec<_> = live
        .iter()
        .flat_map(|id| {
            (0..ValueLayout::new(types[id.0 as usize].clone()).unwrap().lanes())
                .map(move |lane| Atom::new(*id, lane))
        })
        .collect();
    request.id = SelectedStepId(0);
    request.source = SourceStepId(0);
    let mut later = unit(later_inputs, vec![]);
    later.id = SelectedStepId(1);
    later.source = SourceStepId(1);
    let definition = DiscardDefinition {
        types,
        requests: vec![request.clone(), later.clone()],
    };
    let selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let analysis = crate::allocate::analysis::analyze(&selected);
    let mut context = AllocationContext::new(&selected, &analysis);
    context.options = options;
    let mut oracle_stack = all.clone();
    let mut oracle_slots = BTreeMap::new();
    execute(&mut oracle_stack, &mut oracle_slots, &prelude);
    let initial = state(
        oracle_stack.clone(),
        oracle_slots.iter().map(|(slot, atom)| (*slot, *atom)).collect(),
    );
    let preparation = prepare_unit(&initial, &request, &live, &context).unwrap();
    execute(&mut oracle_stack, &mut oracle_slots, &preparation.actions);
    assert_eq!(&oracle_stack[..request.stack_inputs.len()], request.stack_inputs);
    assert_eq!(oracle_stack, preparation.state.stack_atoms().collect::<Vec<_>>());
    oracle_stack.drain(..request.stack_inputs.len());
    for atom in &later.stack_inputs {
        assert!(oracle_stack.contains(atom) || oracle_slots.values().any(|stored| stored == atom));
    }
    let mut after = preparation.state.clone();
    after.stack.drain(..request.stack_inputs.len());
    // This is a second real lowering, so its needs are independently visible in the source.
    let second = prepare_unit(&after, &later, &BTreeSet::new(), &context).unwrap();
    execute(&mut oracle_stack, &mut oracle_slots, &second.actions);
    assert_eq!(&oracle_stack[..later.stack_inputs.len()], later.stack_inputs);
    let mut actions = prelude;
    actions.extend(preparation.actions.clone());
    actions.push(PlanAction::RunLowering { step: request.id });
    actions.extend(second.actions);
    actions.push(PlanAction::RunLowering { step: later.id });
    actions.extend(core::iter::repeat_n(
        PlanAction::Drop,
        second.state.stack.len() - later.stack_inputs.len(),
    ));
    let mut bindings = preparation.bindings.clone();
    bindings.extend(second.bindings);
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts: BTreeMap::from([(SegmentId(0), super::edges::contract(all, vec![]))]),
        segment_actions: BTreeMap::from([(SegmentId(0), actions)]),
        edge_actions: BTreeMap::new(),
        spill_slots: preparation.slot_requests.union(&second.slot_requests).copied().collect(),
        lowering_bindings: bindings,
    };
    let frame = fixture_frame(&plan);
    verify(&plan, &frame, fixture_body(&plan, &frame)).unwrap();
    (plan, preparation, initial)
}

fn forced(tier: SchedulingTier) -> AllocationOptions {
    AllocationOptions {
        force_tier: Some(tier),
        ..Default::default()
    }
}

#[test]
fn gather_depth15_before_reload() {
    let values = values(22);
    let context = context(&values);
    for live in [BTreeSet::new(), BTreeSet::from([ValueId(15)])] {
        let initial =
            state((0..20).map(atom).collect(), vec![(SpillSlotId::canonical(atom(21)), atom(21))]);
        let prepared =
            prepare_unit(&initial, &unit(vec![atom(15), atom(21)], vec![]), &live, &context)
                .unwrap();
        assert_eq!(prepared.tier, SchedulingTier::Loads);
        assert_eq!(
            prepared.state.stack_atoms().take(2).collect::<Vec<_>>(),
            vec![atom(15), atom(21)]
        );
        assert!(
            !prepared
                .actions
                .iter()
                .any(|a| matches!(a, PlanAction::Drop | PlanAction::StoreLane(_)))
        );
        assert_eq!(
            prepared.state.stack.iter().rev().take(4).copied().collect::<Vec<_>>(),
            initial.stack.iter().rev().take(4).copied().collect::<Vec<_>>()
        );
    }
}

#[test]
fn one_eviction_stops_at_deficit() {
    let values = values(17);
    let mut context = context(&values);
    context.options.force_tier = Some(SchedulingTier::Selective);
    let initial =
        state((0..16).map(atom).collect(), vec![(SpillSlotId::canonical(atom(16)), atom(16))]);
    let prepared =
        prepare_unit(&initial, &unit(vec![atom(0), atom(16)], vec![]), &BTreeSet::new(), &context)
            .unwrap();
    assert_eq!(prepared.tier, SchedulingTier::Selective);
    assert_eq!(
        prepared
            .actions
            .iter()
            .filter(|a| matches!(a, PlanAction::Drop | PlanAction::StoreLane(_)))
            .count(),
        1
    );
    assert_eq!(prepared.state.stack.len(), 16);
}

#[test]
fn sixteen_live_after_inputs_get_backing_slots() {
    let values = values(16);
    let mut context = context(&values);
    context.options.force_tier = Some(SchedulingTier::Selective);
    let initial = state((0..16).map(atom).collect(), vec![]);
    let inputs = (0..16).map(atom).collect::<Vec<_>>();
    let live = (0..16).map(ValueId).collect();
    let prepared = prepare_unit(&initial, &unit(inputs.clone(), vec![]), &live, &context).unwrap();
    assert_eq!(prepared.tier, SchedulingTier::Selective);
    assert_eq!(prepared.state.stack_atoms().collect::<Vec<_>>(), inputs);
    assert_eq!(prepared.state.slots.len(), 16);
}

#[test]
fn forced_solver_refusal_uses_occurrence_permutation() {
    let values = values(20);
    let mut context = context(&values);
    context.options.force_tier = Some(SchedulingTier::Selective);
    context.options.refuse_solver = true;
    let initial = state((0..20).map(atom).collect(), vec![]);
    let prepared = prepare_unit(
        &initial,
        &unit(vec![atom(13), atom(1), atom(7)], vec![]),
        &BTreeSet::new(),
        &context,
    )
    .unwrap();
    assert_eq!(prepared.tier, SchedulingTier::Selective);
    assert_eq!(
        prepared.state.stack_atoms().take(3).collect::<Vec<_>>(),
        vec![atom(13), atom(1), atom(7)]
    );
    assert!(!prepared.actions.iter().any(|a| matches!(
        a,
        PlanAction::Drop | PlanAction::StoreLane(_) | PlanAction::LoadLane(_)
    )));
    assert_eq!(
        prepared.state.stack.iter().skip(16).copied().collect::<Vec<_>>(),
        initial.stack.iter().skip(16).copied().collect::<Vec<_>>()
    );
}

#[test]
fn gather_reload_live_survivor_and_existing_buried_copy_verify() {
    let b = SpillSlotId::canonical(atom(0));
    for live in [BTreeSet::new(), BTreeSet::from([ValueId(16)])] {
        for reloaded in [false, true] {
            let mut prelude = vec![PlanAction::StoreLane(b)];
            if reloaded {
                prelude.extend(
                    (1..17).map(|id| PlanAction::StoreLane(SpillSlotId::canonical(atom(id)))),
                );
                prelude.push(PlanAction::LoadLane(b));
                prelude.extend(
                    (1..17).rev().map(|id| PlanAction::LoadLane(SpillSlotId::canonical(atom(id)))),
                );
            }
            let (plan, preparation, initial) = checked_case(
                vec![Type::Felt; 21],
                prelude,
                unit(vec![atom(16), atom(0)], vec![]),
                live.clone(),
                Default::default(),
            );
            assert_eq!(preparation.tier, SchedulingTier::Loads);
            assert!(
                !preparation
                    .actions
                    .iter()
                    .any(|action| matches!(action, PlanAction::Drop | PlanAction::StoreLane(_)))
            );
            assert_eq!(
                preparation
                    .state
                    .stack
                    .iter()
                    .rev()
                    .take(initial.stack.len() - 16)
                    .copied()
                    .collect::<Vec<_>>(),
                initial.stack.iter().skip(16).rev().copied().collect::<Vec<_>>()
            );
            let frame = fixture_frame(&plan);
            verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), 21);
        }
    }
}

#[test]
fn pinned_input_is_not_a_survivor() {
    let b = SpillSlotId::canonical(atom(15));
    let prelude = vec![
        PlanAction::MoveUp { depth: 15 },
        PlanAction::StoreLane(b),
        PlanAction::Dup { depth: 0 },
    ];
    let (plan, preparation, _) = checked_case(
        vec![Type::Felt; 16],
        prelude,
        unit(vec![atom(0), atom(15)], vec![]),
        (0..15).map(ValueId).collect(),
        forced(SchedulingTier::Selective),
    );
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert!(
        preparation
            .state
            .slots
            .values()
            .any(|value| *value == StoredValue::Atom(atom(0)))
    );
    assert_eq!(
        preparation
            .actions
            .iter()
            .filter(|a| matches!(a, PlanAction::StoreLane(_)))
            .count(),
        1
    );
    let frame = fixture_frame(&plan);
    verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), 16);
}

#[test]
fn two_redundant_copies_cannot_both_disappear() {
    let prelude = vec![
        PlanAction::MoveUp { depth: 15 },
        PlanAction::StoreLane(SpillSlotId::canonical(atom(15))),
        PlanAction::MoveUp { depth: 15 },
        PlanAction::StoreLane(SpillSlotId::canonical(atom(16))),
        PlanAction::Dup { depth: 0 },
    ];
    let (_, preparation, _) = checked_case(
        vec![Type::Felt; 17],
        prelude,
        unit(vec![atom(15), atom(16)], vec![]),
        (0..15).map(ValueId).collect(),
        forced(SchedulingTier::Selective),
    );
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert_eq!(preparation.actions.iter().filter(|a| matches!(a, PlanAction::Drop)).count(), 1);
    assert_eq!(
        preparation
            .actions
            .iter()
            .filter(|a| matches!(a, PlanAction::StoreLane(_)))
            .count(),
        1
    );
    assert!(
        preparation
            .state
            .slots
            .values()
            .any(|value| *value == StoredValue::Atom(atom(0)))
    );
}

#[test]
fn straddling_layout_keeps_fixed_suffix_and_buried_only_input_normalizes() {
    let types = core::iter::repeat_n(Type::Felt, 15)
        .chain([Type::U64, Type::Felt])
        .collect::<Vec<_>>();
    for (lane, tier) in [(0, SchedulingTier::Selective), (1, SchedulingTier::Normalization)] {
        let (_, preparation, initial) = checked_case(
            types.clone(),
            vec![],
            unit(vec![Atom::new(ValueId(15), lane), atom(0)], vec![]),
            BTreeSet::from([ValueId(15)]),
            forced(SchedulingTier::Selective),
        );
        assert_eq!(preparation.tier, tier);
        if lane == 0 {
            assert_eq!(
                preparation.state.stack.iter().skip(16).copied().collect::<Vec<_>>(),
                initial.stack.iter().skip(16).copied().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn sixteen_input_survivors_and_address_temporaries_verify_and_execute() {
    let (plan, preparation, _) = checked_case(
        vec![Type::Felt; 16],
        vec![],
        unit((0..16).map(atom).collect(), vec![]),
        (0..16).map(ValueId).collect(),
        forced(SchedulingTier::Selective),
    );
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert_eq!(preparation.state.slots.len(), 16);
    // At full active occupancy, Dup and the local-address push temporarily exceed sixteen.
    assert!(
        preparation
            .actions
            .windows(2)
            .any(|actions| matches!(actions, [PlanAction::Dup { .. }, PlanAction::StoreLane(_)]))
    );
    let frame = fixture_frame(&plan);
    verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), 16);
    let mut corrupt = plan.clone();
    let dup = corrupt
        .segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .iter_mut()
        .find(|action| matches!(action, PlanAction::Dup { .. }))
        .unwrap();
    *dup = PlanAction::Dup { depth: 16 };
    assert!(crate::lower::expand::expand_allocation_action(dup, &frame).is_err());
    assert!(verify(&corrupt, &frame, fixture_body(&plan, &frame)).is_err());
    let body = fixture_body(&plan, &frame);
    let mut buried = crate::lower::expand::AnnotatedMasmBody::new();
    for op in body.ops() {
        if matches!(
            op,
            crate::lower::expand::AnnotatedOp::Boundary(crate::lower::expand::Boundary::UnitEnd(
                SelectedStepId(0)
            ))
        ) {
            // Dup16 has no MASM encoding. Once all sixteen inputs have been consumed,
            // even encoded Dup15 reads the unknown caller suffix. Restore nominal height
            // with Drop so this mutation specifically requires independent raw checking.
            buried.push_instruction(crate::masm::Instruction::Dup15, SelectedStepId(0));
            buried.push_instruction(crate::masm::Instruction::Drop, SelectedStepId(0));
        }
        buried.push(op.clone());
    }
    assert_eq!(
        verify(&plan, &frame, buried).unwrap_err().category,
        DiagnosticCategory::IllegalStackAccess
    );
}

#[test]
fn victim_store_cannot_overwrite_the_only_post_input_backing() {
    let occupied = SpillSlotId::canonical(atom(1));
    let prelude = vec![PlanAction::Dup { depth: 0 }, PlanAction::StoreLane(occupied)];
    let (_, preparation, _) = checked_case(
        vec![Type::Felt; 16],
        prelude,
        unit((0..16).map(atom).collect(), vec![]),
        BTreeSet::from([ValueId(0), ValueId(1)]),
        forced(SchedulingTier::Selective),
    );
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert_eq!(preparation.state.slots[&occupied], StoredValue::Atom(atom(0)));
    assert!(
        preparation
            .state
            .slots
            .values()
            .any(|value| *value == StoredValue::Atom(atom(1)))
    );
}

#[test]
fn stored_setup_can_decline_and_semantic_only_uses_survive_eviction() {
    let request = unit(
        vec![atom(0)],
        vec![StoredInput {
            id: StoredInputId(0),
            value: StoredValue::Atom(atom(19)),
        }],
    );
    let (_, preparation, _) =
        checked_case(vec![Type::Felt; 20], vec![], request, BTreeSet::new(), Default::default());
    assert_eq!(preparation.tier, SchedulingTier::Normalization);
    let mut request = unit(vec![atom(16)], vec![]);
    request.semantic_inputs.push(ValueId(0));
    let prelude = vec![
        PlanAction::MoveUp { depth: 15 },
        PlanAction::StoreLane(SpillSlotId::canonical(atom(15))),
        PlanAction::MoveUp { depth: 15 },
        PlanAction::StoreLane(SpillSlotId::canonical(atom(16))),
        PlanAction::LoadLane(SpillSlotId::canonical(atom(15))),
    ];
    let (_, preparation, _) = checked_case(
        vec![Type::Felt; 17],
        prelude,
        request,
        BTreeSet::new(),
        forced(SchedulingTier::Selective),
    );
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert!(
        preparation.state.stack_atoms().any(|a| a == atom(0))
            || preparation
                .state
                .slots
                .values()
                .any(|value| *value == StoredValue::Atom(atom(0)))
    );
}

#[test]
fn grouped_actions_preserve_unequal_widths_and_deeper_lanes() {
    for types in [
        vec![Type::Felt, Type::U128, Type::U64, Type::Felt],
        vec![Type::U64, Type::Felt, Type::U128, Type::Felt],
    ] {
        let mut request = unit((0..4).map(|lane| Atom::new(ValueId(2), lane)).collect(), vec![]);
        let width = ValueLayout::new(types[2].clone()).unwrap().lanes();
        request.stack_inputs = (0..width)
            .map(|lane| Atom::new(ValueId(2), lane))
            .chain(core::iter::once(atom(0)))
            .chain(
                (0..ValueLayout::new(types[1].clone()).unwrap().lanes())
                    .map(|lane| Atom::new(ValueId(1), lane)),
            )
            .collect();
        // Include all lanes of the first operand when it is multi-lane.
        request.stack_inputs.splice(
            usize::from(width)..usize::from(width) + 1,
            (0..ValueLayout::new(types[0].clone()).unwrap().lanes())
                .map(|lane| Atom::new(ValueId(0), lane)),
        );
        request.semantic_inputs = vec![ValueId(0), ValueId(1), ValueId(2)];
        let lanes: usize = types
            .iter()
            .map(|ty| usize::from(ValueLayout::new(ty.clone()).unwrap().lanes()))
            .sum();
        let (plan, preparation, _) = checked_case(
            types,
            vec![],
            request,
            BTreeSet::from([ValueId(2)]),
            forced(SchedulingTier::Stack),
        );
        assert_eq!(preparation.tier, SchedulingTier::Stack);
        let frame = fixture_frame(&plan);
        verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), lanes);
    }
}

#[test]
fn solver_limits_do_not_limit_raw_depth_or_wide_tuples() {
    let (_, ready, _) = checked_case(
        vec![Type::Felt; 18],
        vec![],
        unit((0..18).map(atom).collect(), vec![]),
        BTreeSet::new(),
        Default::default(),
    );
    assert_eq!(ready.tier, SchedulingTier::Stack);
    assert!(ready.actions.is_empty());
    let (_, wider, _) = checked_case(
        vec![Type::Felt; 18],
        vec![],
        unit((0..18).rev().map(atom).collect(), vec![]),
        BTreeSet::new(),
        forced(SchedulingTier::Selective),
    );
    assert_eq!(wider.tier, SchedulingTier::Normalization);
    let (_, deep, initial) = checked_case(
        vec![Type::Felt; 260],
        vec![],
        unit(vec![atom(1), atom(0)], vec![]),
        BTreeSet::new(),
        Default::default(),
    );
    assert_eq!(deep.tier, SchedulingTier::Selective);
    assert_eq!(
        deep.state.stack.iter().skip(16).copied().collect::<Vec<_>>(),
        initial.stack.iter().skip(16).copied().collect::<Vec<_>>()
    );
}

#[test]
fn victim_ranking_uses_existing_next_use_distances() {
    let first = unit(vec![atom(16)], vec![]);
    let mut near = unit((0..15).map(atom).collect(), vec![]);
    near.id = SelectedStepId(1);
    near.source = SourceStepId(1);
    let mut far = unit(vec![atom(15)], vec![]);
    far.id = SelectedStepId(2);
    far.source = SourceStepId(2);
    let definition = DiscardDefinition {
        types: vec![Type::Felt; 17],
        requests: vec![first.clone(), near, far],
    };
    let selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let analysis = crate::allocate::analysis::analyze(&selected);
    let mut context = AllocationContext::new(&selected, &analysis);
    context.options = forced(SchedulingTier::Selective);
    let initial =
        state((0..16).map(atom).collect(), vec![(SpillSlotId::canonical(atom(16)), atom(16))]);
    let preparation =
        prepare_unit(&initial, &first, &(0..16).map(ValueId).collect(), &context).unwrap();
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert!(
        preparation
            .actions
            .contains(&PlanAction::StoreLane(SpillSlotId::canonical(atom(15))))
    );
    assert_eq!(
        preparation
            .actions
            .iter()
            .filter(|action| matches!(action, PlanAction::StoreLane(_)))
            .count(),
        1
    );
}

#[test]
fn victim_classes_prefer_dead_then_backed_then_new_stores() {
    let values = values(17);
    let mut context = context(&values);
    context.options = forced(SchedulingTier::Selective);
    let backed = SpillSlotId::canonical(atom(12));
    let initial = state(
        (0..16).map(atom).collect(),
        vec![(SpillSlotId::canonical(atom(16)), atom(16)), (backed, atom(12))],
    );
    for (dead, victim) in [(Some(7), 7), (None, 12)] {
        let live = (0..16).filter(|id| Some(*id) != dead).map(ValueId).collect();
        let preparation =
            prepare_unit(&initial, &unit(vec![atom(16)], vec![]), &live, &context).unwrap();
        assert_eq!(preparation.tier, SchedulingTier::Selective);
        assert!(!preparation.state.stack_atoms().any(|a| a == atom(victim)));
        assert_eq!(preparation.actions.iter().filter(|a| matches!(a, PlanAction::Drop)).count(), 1);
        assert!(!preparation.actions.iter().any(|a| matches!(a, PlanAction::StoreLane(_))));
        assert_eq!(preparation.victims.len(), 1);
        assert_eq!(preparation.victims[0].atom, atom(victim));
        assert_eq!(
            preparation.victims[0].reason,
            if dead.is_some() {
                VictimReason::Redundant
            } else {
                VictimReason::Backed
            }
        );
        let repeated =
            prepare_unit(&initial, &unit(vec![atom(16)], vec![]), &live, &context).unwrap();
        assert_eq!(
            alloc::format!("{:?}", preparation.victims),
            alloc::format!("{:?}", repeated.victims)
        );
    }
}

#[test]
fn fixed_candidates_compare_expanded_costs_and_tie_in_load_first_order() {
    use crate::allocate::{
        normalize,
        schedule::{cost, load_candidate, load_candidates},
        storage::Trial,
    };
    let mut distinct_tie = false;
    let mut compared = 0;
    for size in 2..=7 {
        let values = values(size + 1);
        let context = context(&values);
        let initial = state(
            (0..size).map(atom).collect(),
            vec![(SpillSlotId::canonical(atom(size)), atom(size))],
        );
        for a in 0..size {
            for b in 0..size {
                for input in [
                    vec![atom(a), atom(size), atom(b)],
                    vec![atom(size), atom(a), atom(b)],
                    vec![atom(a), atom(b), atom(size)],
                ] {
                    let request = unit(input, vec![]);
                    let needs =
                        normalize::requirements(&request, &BTreeSet::new(), &context).unwrap();
                    let base = Trial::new(&initial, &context, SlotOwner::Unit(request.id));
                    let first = load_candidate(base.clone(), &request, &needs, false).unwrap();
                    let second = load_candidate(base.clone(), &request, &needs, true).unwrap();
                    if let (Some(first), Some(second)) = (first, second) {
                        let chosen = load_candidates(&base, &request, &needs).unwrap().unwrap();
                        let expected = if cost(&first) <= cost(&second) {
                            &first
                        } else {
                            &second
                        };
                        assert_eq!(chosen.actions, expected.actions);
                        assert_eq!(chosen.state, expected.state);
                        distinct_tie |=
                            cost(&first) == cost(&second) && first.actions != second.actions;
                        compared += 1;
                    }
                }
            }
        }
    }
    assert!(compared > 100);
    assert!(distinct_tie, "exercise an observable tie between different fixed candidates");
}

#[test]
fn failed_candidates_leave_bindings_ids_slots_and_actions_private() {
    use crate::allocate::{
        normalize,
        schedule::{load_candidate, load_candidates},
        storage::Trial,
    };
    let values = values(22);
    let context = context(&values);
    let initial =
        state((0..20).map(atom).collect(), vec![(SpillSlotId::canonical(atom(21)), atom(21))]);
    let request = unit(
        vec![atom(15), atom(21)],
        vec![StoredInput {
            id: StoredInputId(0),
            value: StoredValue::Literal(miden_core::Felt::new_unchecked(73)),
        }],
    );
    let live = BTreeSet::from([ValueId(15)]);
    let needs = normalize::requirements(&request, &live, &context).unwrap();
    let mut base = Trial::new(&initial, &context, SlotOwner::Unit(request.id));
    normalize::bind_inputs(&mut base, &request, &needs.needed).unwrap();
    let snapshot = base.clone();
    assert!(load_candidate(base.clone(), &request, &needs, false).unwrap().is_none());
    let second = load_candidate(base.clone(), &request, &needs, true).unwrap().unwrap();
    let chosen = load_candidates(&base, &request, &needs).unwrap().unwrap();
    assert_eq!(chosen.actions, second.actions);
    assert_eq!(chosen.state, second.state);
    assert_eq!(
        chosen.bindings[&request.id].stored_inputs,
        second.bindings[&request.id].stored_inputs
    );
    assert_eq!(base.state, snapshot.state);
    assert_eq!(base.actions, snapshot.actions);
    assert_eq!(
        base.bindings[&request.id].stored_inputs,
        snapshot.bindings[&request.id].stored_inputs
    );
    let first_retry = prepare_unit(&initial, &request, &live, &context).unwrap();
    let second_retry = prepare_unit(&initial, &request, &live, &context).unwrap();
    assert_eq!(first_retry.state, second_retry.state);
    assert_eq!(first_retry.actions, second_retry.actions);
    let mut exhausted = initial.clone();
    exhausted.next_occurrence = u32::MAX;
    let snapshot = exhausted.clone();
    assert!(prepare_unit(&exhausted, &request, &live, &context).is_err());
    assert_eq!(exhausted, snapshot);
}

#[test]
fn solver_refusal_completion_is_independently_verified() {
    let mut options = forced(SchedulingTier::Selective);
    options.refuse_solver = true;
    let (plan, preparation, initial) = checked_case(
        vec![Type::Felt; 20],
        vec![],
        unit(vec![atom(13), atom(1), atom(7)], vec![]),
        BTreeSet::from([ValueId(13)]),
        options,
    );
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert_eq!(
        preparation.state.stack.iter().skip(16).copied().collect::<Vec<_>>(),
        initial.stack.iter().skip(16).copied().collect::<Vec<_>>()
    );
    assert_eq!(
        preparation
            .actions
            .iter()
            .filter(|a| matches!(a, PlanAction::Drop | PlanAction::StoreLane(_)))
            .count(),
        1
    );
    let frame = fixture_frame(&plan);
    verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), 20);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, max_shrink_iters: 512, ..ProptestConfig::default() })]
    #[test]
    fn selective_and_normalization_have_identical_observations(
        size in 2usize..=22, inputs in prop::collection::vec(0usize..44, 0..=18), mask in any::<u32>(), spill in 0usize..=4, duplicate in any::<bool>(), refuse in any::<bool>(),
    ) {
        let spill = spill.min(size - 1);
        let mut prelude: Vec<_> = (0..spill as u32).map(|id| PlanAction::StoreLane(SpillSlotId::canonical(atom(id)))).collect();
        if duplicate { prelude.push(PlanAction::Dup { depth: 0 }); }
        let inputs: Vec<_> = inputs.into_iter().map(|id| atom((id % size) as u32)).collect();
        let live: BTreeSet<_> = (0..size).filter(|id| mask & (1 << id) != 0).map(|id| ValueId(id as u32)).collect();
        let mut options = forced(SchedulingTier::Selective);
        options.refuse_solver = refuse;
        let (_, selective, _) = checked_case(vec![Type::Felt; size], prelude.clone(), unit(inputs.clone(), vec![]), live.clone(), options);
        let (_, normalized, _) = checked_case(vec![Type::Felt; size], prelude, unit(inputs.clone(), vec![]), live, forced(SchedulingTier::Normalization));
        prop_assert_eq!(selective.state.stack_atoms().take(inputs.len()).collect::<Vec<_>>(), normalized.state.stack_atoms().take(inputs.len()).collect::<Vec<_>>());
        if inputs.len() <= 16 && size - spill <= 16 && !duplicate { prop_assert_eq!(selective.tier, SchedulingTier::Selective); }
    }

    #[test]
    fn grouped_copy_schedules_match_distinct_lane_observations(
        kinds in prop::collection::vec(0u8..3, 1..=5), requests in prop::collection::vec(0usize..20, 0..=4), mask in any::<u8>(),
    ) {
        let types: Vec<_> = kinds.iter().map(|kind| match kind { 0 => Type::Felt, 1 => Type::U64, _ => Type::U128 }).collect();
        let inputs: Vec<_> = requests.into_iter().flat_map(|id| {
            let id = id % types.len();
            (0..ValueLayout::new(types[id].clone()).unwrap().lanes()).map(move |lane| Atom::new(ValueId(id as u32), lane))
        }).collect();
        let live: BTreeSet<_> = (0..types.len()).filter(|id| mask & (1 << id) != 0).map(|id| ValueId(id as u32)).collect();
        let (_, fast, _) = checked_case(types.clone(), vec![], unit(inputs.clone(), vec![]), live.clone(), Default::default());
        let (_, full, _) = checked_case(types, vec![], unit(inputs.clone(), vec![]), live, forced(SchedulingTier::Normalization));
        prop_assert_eq!(fast.state.stack_atoms().take(inputs.len()).collect::<Vec<_>>(), full.state.stack_atoms().take(inputs.len()).collect::<Vec<_>>());
    }
}

#[test]
fn selective_trace_records_actual_store_victims() {
    let values = values(16);
    let mut context = context(&values);
    context.options.force_tier = Some(SchedulingTier::Selective);
    let initial = state((0..16).map(atom).collect(), vec![]);
    let request = unit((0..16).map(atom).collect(), vec![]);
    let preparation =
        prepare_unit(&initial, &request, &(0..16).map(ValueId).collect(), &context).unwrap();
    assert_eq!(preparation.tier, SchedulingTier::Selective);
    assert_eq!(preparation.victims.len(), 16);
    assert!(
        preparation
            .victims
            .iter()
            .all(|decision| decision.reason == VictimReason::NeedsStore)
    );
    assert_eq!(preparation.state.slots.len(), 16);
}
