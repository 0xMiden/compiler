use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use super::edges::{atom, context, state, unit, values};
use crate::{
    allocate::{normalize::prepare_unit, state::AllocationError},
    lower::{expand::*, units::LoweringBindings},
    plan::{actions::*, *},
    verify::*,
};

/// The registered fixture discards declared parameter tuples. Physical preparation is the
/// allocator's job; its output is never fed back into this source definition.
pub(super) struct DiscardDefinition {
    pub types: Vec<midenc_hir::Type>,
    pub requests: Vec<SelectedLowering>,
}

impl GeneratedDefinition for DiscardDefinition {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(401)
    }

    fn instantiate(
        &self,
        parameters: &[miden_core::Felt],
    ) -> Result<DefinitionEvidence, VerificationError> {
        assert!(parameters.is_empty());
        let values: BTreeMap<_, _> = self
            .types
            .iter()
            .enumerate()
            .map(|(id, ty)| {
                let value = SelectedValue {
                    id: ValueId(id as u32),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(0),
                        index: id as u32,
                    },
                    layout: ValueLayout::new(ty.clone()).unwrap(),
                    reconstruction: None,
                };
                (value.id, value)
            })
            .collect();
        let lanes = values.values().map(|v| u32::from(v.layout.lanes())).sum();
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            units: self.requests.iter().map(|u| (u.id, UnitEvidence::default())).collect(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                source_map: self
                    .requests
                    .iter()
                    .map(|u| {
                        (
                            u.source,
                            SourceRecord {
                                origin: SourceOrigin::Generated {
                                    definition: self.id(),
                                    stage: u.source.0,
                                },
                                location: SourceLocation::default(),
                            },
                        )
                    })
                    .collect(),
                segments: vec![SelectedSegment {
                    id: SegmentId(0),
                    parameters: values.keys().copied().collect(),
                    content_parameters: vec![],
                    units: self.requests.clone(),
                }],
                values,
                edges: vec![TransferEdge {
                    id: EdgeId(0),
                    source: SegmentId(0),
                    destination: None,
                    kind: EdgeKind::Return,
                    bindings: vec![],
                    content_bindings: vec![],
                }],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, lanes, 0).unwrap(),
            },
        })
    }
}

pub(super) fn fixture_frame(plan: &ProcedurePlan) -> FinalFrameLayout {
    let offsets = plan.spill_slots.iter().enumerate().map(|(i, slot)| (*slot, i as u32)).collect();
    FinalFrameLayout::new(
        SourceLocalLayout::default(),
        offsets,
        BTreeMap::new(),
        (plan.spill_slots.len() as u32).next_multiple_of(4),
    )
    .unwrap()
}

pub(super) fn append_actions(
    body: &mut AnnotatedMasmBody,
    actions: &[PlanAction],
    plan: &ProcedurePlan,
    frame: &FinalFrameLayout,
) {
    for action in actions {
        if let PlanAction::RunLowering { step } = action {
            let unit = plan
                .selected
                .segments
                .iter()
                .flat_map(|s| &s.units)
                .find(|u| u.id == *step)
                .unwrap();
            body.push(AnnotatedOp::Boundary(Boundary::UnitStart(*step)));
            for _ in &unit.stack_inputs {
                body.push_instruction(crate::masm::Instruction::Drop, *step);
            }
            body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(*step)));
        } else if let Some(instruction) = expand_allocation_action(action, frame).unwrap() {
            body.push_instruction(instruction, SelectedStepId(u32::MAX));
        }
    }
}

pub(super) fn fixture_body(plan: &ProcedurePlan, frame: &FinalFrameLayout) -> AnnotatedMasmBody {
    let mut body = AnnotatedMasmBody::new();
    let ControlTree::Sequence(children) = &plan.selected.structure else {
        panic!("linear fixture")
    };
    for child in children {
        match child {
            ControlTree::Segment(id) => {
                body.push(AnnotatedOp::Boundary(Boundary::Segment(*id)));
                append_actions(&mut body, &plan.segment_actions[id], plan, frame);
            }
            ControlTree::Edge(id) => {
                body.push(AnnotatedOp::Boundary(Boundary::Edge(*id)));
                if let Some(actions) = plan.edge_actions.get(id) {
                    append_actions(&mut body, actions, plan, frame);
                }
            }
            _ => panic!("linear fixture"),
        }
    }
    body
}

pub(super) fn verify_assemble_execute(
    plan: &ProcedurePlan,
    frame: &FinalFrameLayout,
    body: &AnnotatedMasmBody,
    input_lanes: usize,
) {
    use miden_core::Felt;
    use miden_processor::{DefaultHost, FastProcessor, StackInputs};
    verify(plan, frame, body.clone()).unwrap();
    let code: alloc::string::String = body
        .top_level_instructions()
        .map(|instruction| alloc::format!("{instruction} "))
        .collect();
    // Each argument and every suffix lane has a different full-width value. Independent
    // replay checks operand identity/order; execution checks the complete caller suffix.
    let base = 1u64 << 40;
    let arguments: alloc::string::String = (0..input_lanes)
        .rev()
        .map(|lane| alloc::format!("push.{} ", base + lane as u64))
        .collect();
    let checks: alloc::string::String = (0..16)
        .map(|lane| alloc::format!("push.{} assert_eq ", base + 1000 + lane))
        .collect();
    let source = alloc::format!(
        "@locals({}) proc test {code} end begin {arguments} exec.test {checks} end",
        frame.aligned_size()
    );
    let package = miden_assembly::Assembler::default()
        .assemble_program("allocator_transition", source)
        .unwrap();
    let program = package.try_into_program().unwrap();
    let suffix: Vec<_> = (0..16).map(|lane| Felt::new_unchecked(base + 1000 + lane)).collect();
    FastProcessor::new(StackInputs::new(&suffix).unwrap())
        .execute_sync(&program, &mut DefaultHost::default())
        .unwrap();
}

fn discard_plan(definition: &DiscardDefinition, live_after: &[BTreeSet<ValueId>]) -> ProcedurePlan {
    let selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(definition),
    )
    .unwrap();
    let initial: Vec<_> = selected
        .values
        .values()
        .flat_map(|v| (0..v.layout.lanes()).map(move |lane| Atom::new(v.id, lane)))
        .collect();
    let entry = super::edges::contract(initial.clone(), vec![]);
    let mut state = state(initial, vec![]);
    let mut actions = Vec::new();
    let mut bindings = BTreeMap::<SelectedStepId, LoweringBindings>::new();
    let mut slots = BTreeSet::new();
    for (request, live) in definition.requests.iter().zip(live_after) {
        let proposal = prepare_unit(&state, request, live, &context(&selected.values)).unwrap();
        assert!(
            proposal.actions.len()
                <= state.stack.len()
                    + 3 * selected.values.len()
                    + request.stack_inputs.len()
                    + 2 * request.stored_inputs.len()
        );
        proposal.commit(&mut state, &mut actions, &mut bindings, &mut slots);
        actions.push(PlanAction::RunLowering { step: request.id });
        state.stack.clear(); // The source fixture's declared discard effect, checked independently.
    }
    ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts: BTreeMap::from([(SegmentId(0), entry)]),
        segment_actions: BTreeMap::from([(SegmentId(0), actions)]),
        edge_actions: BTreeMap::new(),
        spill_slots: slots.into_iter().collect(),
        lowering_bindings: bindings,
    }
}

#[test]
fn wide_mixed_raw_layouts_repeated_lanes_and_live_after_verify_and_assemble() {
    use midenc_hir::Type;
    let types = vec![
        Type::U128,
        Type::U64,
        Type::Felt,
        Type::U128,
        Type::U64,
        Type::Felt,
        Type::U128,
        Type::U64,
        Type::Felt,
    ];
    let all: Vec<_> = types
        .iter()
        .enumerate()
        .flat_map(|(id, ty)| {
            (0..ValueLayout::new(ty.clone()).unwrap().lanes())
                .map(move |lane| Atom::new(ValueId(id as u32), lane))
        })
        .collect();
    let mut request = unit(
        all.iter().rev().copied().chain(all.iter().take(4).copied()).collect(),
        vec![
            StoredInput {
                id: StoredInputId(0),
                value: StoredValue::Atom(Atom::new(ValueId(0), 3)),
            },
            StoredInput {
                id: StoredInputId(1),
                value: StoredValue::Literal(miden_core::Felt::new_unchecked(73)),
            },
        ],
    );
    request.semantic_inputs = (0..types.len() as u32).map(ValueId).collect();
    let mut later = unit((0..4).map(|lane| Atom::new(ValueId(0), lane)).collect(), vec![]);
    later.id = SelectedStepId(1);
    later.source = SourceStepId(1);
    let definition = DiscardDefinition {
        types,
        requests: vec![request, later],
    };
    let plan = discard_plan(&definition, &[BTreeSet::from([ValueId(0)]), BTreeSet::new()]);
    let frame = fixture_frame(&plan);
    let body = fixture_body(&plan, &frame);
    verify_assemble_execute(&plan, &frame, &body, all.len());
    let mut corrupt = plan.clone();
    let actions = corrupt.segment_actions.get_mut(&SegmentId(0)).unwrap();
    let action = actions.iter_mut().find(|a| matches!(a, PlanAction::StoreLane(_))).unwrap();
    *action = PlanAction::Drop;
    let invalid = fixture_body(&corrupt, &frame);
    assert!(
        verify(&corrupt, &frame, invalid).is_err(),
        "source-loss mutation must fail independent replay"
    );
}

#[test]
fn buried_last_use_stored_input_also_on_stack_verifies_and_assembles() {
    let request = unit(
        vec![atom(19), atom(19)],
        vec![
            StoredInput {
                id: StoredInputId(0),
                value: StoredValue::Atom(atom(19)),
            },
            StoredInput {
                id: StoredInputId(1),
                value: StoredValue::Literal(miden_core::Felt::new_unchecked(99)),
            },
        ],
    );
    let plan = discard_plan(
        &DiscardDefinition {
            types: vec![midenc_hir::Type::Felt; 20],
            requests: vec![request],
        },
        &[BTreeSet::new()],
    );
    let frame = fixture_frame(&plan);
    let body = fixture_body(&plan, &frame);
    verify_assemble_execute(&plan, &frame, &body, 20);
    let mut corrupt = plan.clone();
    let actions = corrupt.segment_actions.get_mut(&SegmentId(0)).unwrap();
    let literal = actions.iter_mut().find(|a| matches!(a, PlanAction::PushLiteral(_))).unwrap();
    *literal = PlanAction::PushLiteral(miden_core::Felt::ZERO);
    assert!(verify(&corrupt, &frame, fixture_body(&corrupt, &frame)).is_err());
}

#[test]
fn canonical_slot_collision_preserves_both_inputs_through_verified_assembly() {
    let request = unit(
        vec![atom(0), atom(1)],
        vec![StoredInput {
            id: StoredInputId(0),
            value: StoredValue::Atom(atom(0)),
        }],
    );
    let definition = DiscardDefinition {
        types: vec![midenc_hir::Type::Felt; 2],
        requests: vec![request.clone()],
    };
    let selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let canonical = SpillSlotId::canonical(atom(0));
    let initial = state(vec![atom(0)], vec![(canonical, atom(1))]);
    let proposal =
        prepare_unit(&initial, &request, &BTreeSet::new(), &context(&selected.values)).unwrap();
    let mut actions = vec![PlanAction::Swap { depth: 1 }, PlanAction::StoreLane(canonical)];
    actions.extend(proposal.actions);
    actions.push(PlanAction::RunLowering { step: request.id });
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts: BTreeMap::from([(
            SegmentId(0),
            super::edges::contract(vec![atom(0), atom(1)], vec![]),
        )]),
        segment_actions: BTreeMap::from([(SegmentId(0), actions)]),
        edge_actions: BTreeMap::new(),
        spill_slots: proposal.slot_requests.into_iter().collect(),
        lowering_bindings: proposal.bindings,
    };
    let frame = fixture_frame(&plan);
    verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), 2);
    let mut corrupt = plan.clone();
    let actions = corrupt.segment_actions.get_mut(&SegmentId(0)).unwrap();
    let new_store = actions
        .iter_mut()
        .find(|action| matches!(action, PlanAction::StoreLane(slot) if *slot != canonical))
        .unwrap();
    *new_store = PlanAction::StoreLane(canonical);
    assert!(
        verify(&corrupt, &frame, fixture_body(&corrupt, &frame)).is_err(),
        "canonical preference cannot authorize a destructive overwrite"
    );
}

#[test]
fn buried_only_input_and_dead_occurrences_are_drained_without_indexing_the_tail() {
    let values = values(24);
    let context = context(&values);
    let initial = state((0..24).map(atom).collect(), vec![]);
    let proposal =
        prepare_unit(&initial, &unit(vec![atom(23), atom(23)], vec![]), &BTreeSet::new(), &context)
            .unwrap();
    assert_eq!(
        proposal.state.stack_atoms().collect::<alloc::vec::Vec<_>>(),
        vec![atom(23), atom(23)]
    );
    assert!(proposal.actions.len() < 32, "finite drain plus two reloads");
    assert_eq!(proposal.state.tail, TailPolicy::PreserveSuffix);
    assert_eq!(initial.stack.len(), 24);
}

#[test]
fn semantic_inputs_without_physical_operands_remain_recoverable_at_unit_entry() {
    let values = values(1);
    let initial = state(vec![atom(0)], vec![]);
    let mut request = unit(vec![], vec![]);
    request.semantic_inputs = vec![ValueId(0)];
    let proposal = prepare_unit(&initial, &request, &BTreeSet::new(), &context(&values)).unwrap();
    assert!(proposal.state.slots.values().any(|value| *value == StoredValue::Atom(atom(0))));
    assert!(proposal.state.stack.is_empty());
}

#[test]
fn alias_equality_is_symmetric_and_identity_cycles_do_not_lose_sources() {
    let values = values(2);
    for aliases in [
        vec![(atom(0), atom(1))],
        vec![(atom(1), atom(0))],
        vec![(atom(0), atom(1)), (atom(1), atom(0)), (atom(0), atom(0))],
    ] {
        let mut entry = super::edges::contract(vec![atom(0)], vec![]);
        entry.aliases = LaneAliases::new(aliases);
        let initial = crate::allocate::state::AllocationState::from_contract(&entry).unwrap();
        let proposal = prepare_unit(
            &initial,
            &unit(vec![atom(1), atom(0)], vec![]),
            &BTreeSet::new(),
            &context(&values),
        )
        .unwrap();
        assert_eq!(proposal.state.stack_atoms().collect::<Vec<_>>(), vec![atom(1), atom(0)]);
    }
}

#[test]
fn canonical_slot_holds_other_needed_atom_during_backing_and_stored_preparation() {
    use crate::allocate::storage::Trial;
    let values = values(2);
    let context = context(&values);
    let old = SpillSlotId::canonical(atom(0));
    let initial = state(vec![atom(0)], vec![(old, atom(1))]);
    let needed = BTreeSet::from([atom(0), atom(1)]);
    let mut backing = Trial::new(&initial, &context, SlotOwner::Unit(SelectedStepId(0)));
    let slot = backing.back_up_top(&needed).unwrap();
    assert_ne!(slot, old);
    assert_eq!(backing.state.slots[&old], StoredValue::Atom(atom(1)));
    let mut stored = Trial::new(&initial, &context, SlotOwner::Unit(SelectedStepId(0)));
    let slot = stored.bind_stored(StoredValue::Atom(atom(0)), &needed).unwrap();
    assert_ne!(slot, old);
    assert_eq!(stored.state.slots[&old], StoredValue::Atom(atom(1)));
    assert_eq!(
        stored.state.stack_atoms().collect::<Vec<_>>(),
        vec![atom(0)],
        "binding copies rather than consumes a pending operand"
    );
}

#[test]
fn literal_and_alias_reconstruction_still_emit_real_stored_writes() {
    use miden_core::Felt;
    let mut values = values(2);
    values.get_mut(&ValueId(0)).unwrap().reconstruction =
        Some(Reconstruction::Literal(vec![Felt::new_unchecked(123)]));
    values.get_mut(&ValueId(1)).unwrap().reconstruction =
        Some(Reconstruction::Alias(vec![atom(0)]));
    let initial = state(vec![atom(0), atom(1)], vec![]);
    let request = unit(
        vec![atom(1), atom(0)],
        vec![StoredInput {
            id: StoredInputId(0),
            value: StoredValue::Atom(atom(1)),
        }],
    );
    let proposal = prepare_unit(&initial, &request, &BTreeSet::new(), &context(&values)).unwrap();
    assert!(proposal.actions.iter().any(|action| matches!(action, PlanAction::StoreLane(_))));
    assert_eq!(
        proposal
            .actions
            .iter()
            .filter(|action| matches!(action, PlanAction::PushLiteral(_)))
            .count(),
        3,
        "one physical store source and two reconstructed occurrences"
    );
    assert_eq!(proposal.state.stack_atoms().collect::<Vec<_>>(), vec![atom(1), atom(0)]);
}

#[test]
fn canonical_slot_holds_other_needed_atom() {
    let values = values(2);
    let initial = state(vec![atom(0)], vec![(SpillSlotId::canonical(atom(0)), atom(1))]);
    let proposal = prepare_unit(
        &initial,
        &unit(vec![atom(0), atom(1)], vec![]),
        &BTreeSet::new(),
        &context(&values),
    )
    .unwrap();
    assert_eq!(
        proposal.state.stack_atoms().collect::<alloc::vec::Vec<_>>(),
        vec![atom(0), atom(1)]
    );
    assert_eq!(
        proposal.state.slots[&SpillSlotId::canonical(atom(0))],
        StoredValue::Atom(atom(1))
    );
}

#[test]
fn failed_preparation_is_atomic() {
    let values = values(2);
    let initial = state(vec![atom(0)], vec![]);
    let snapshot = initial.clone();
    let result = prepare_unit(
        &initial,
        &unit(vec![atom(0), atom(1)], vec![]),
        &BTreeSet::new(),
        &context(&values),
    );
    assert!(
        matches!(result, Err(AllocationError::Invariant(error)) if error.category == DiagnosticCategory::UnavailableValue)
    );
    assert_eq!(initial, snapshot);
    let request = unit(vec![atom(0)], vec![]);
    let first = prepare_unit(&initial, &request, &BTreeSet::new(), &context(&values)).unwrap();
    let second = prepare_unit(&initial, &request, &BTreeSet::new(), &context(&values)).unwrap();
    assert_eq!(first.actions, second.actions);
    assert_eq!(first.state, second.state);
}

#[test]
fn stored_literals_and_stack_inputs_are_preserved_during_the_same_drain() {
    let values = values(20);
    let initial = state((0..20).map(atom).collect(), vec![]);
    let request = unit(
        vec![atom(19), atom(18), atom(19)],
        vec![
            StoredInput {
                id: StoredInputId(0),
                value: StoredValue::Literal(miden_core::Felt::new_unchecked(77)),
            },
            StoredInput {
                id: StoredInputId(1),
                value: StoredValue::Atom(atom(19)),
            },
            StoredInput {
                id: StoredInputId(2),
                value: StoredValue::Atom(atom(19)),
            },
        ],
    );
    let proposal =
        prepare_unit(&initial, &request, &BTreeSet::from([ValueId(17)]), &context(&values))
            .unwrap();
    let bound = &proposal.bindings[&request.id].stored_inputs;
    assert_eq!(bound[&StoredInputId(1)], bound[&StoredInputId(2)]);
    assert_eq!(
        proposal.state.slots[&bound[&StoredInputId(0)]],
        StoredValue::Literal(miden_core::Felt::new_unchecked(77))
    );
    assert!(proposal.state.slots.values().any(|v| *v == StoredValue::Atom(atom(17))));
    assert_eq!(
        proposal.state.stack_atoms().collect::<alloc::vec::Vec<_>>(),
        vec![atom(19), atom(18), atom(19)]
    );
    assert!(proposal.actions.windows(2).any(|a| a
        == [
            crate::plan::actions::PlanAction::PushLiteral(miden_core::Felt::new_unchecked(77)),
            crate::plan::actions::PlanAction::StoreLane(bound[&StoredInputId(0)])
        ]));
}

#[test]
fn identifier_failure_discards_partially_built_actions_slots_and_bindings() {
    let values = values(2);
    let mut initial = state(vec![atom(0)], vec![(SpillSlotId::canonical(atom(0)), atom(1))]);
    initial.next_temporary.insert(SlotOwner::Unit(SelectedStepId(0)), u32::MAX);
    let snapshot = initial.clone();
    assert!(matches!(
        prepare_unit(
            &initial,
            &unit(vec![atom(0), atom(1)], vec![]),
            &BTreeSet::new(),
            &context(&values)
        ),
        Err(AllocationError::IdentifierExhausted(_))
    ));
    assert_eq!(initial, snapshot);
    initial.next_temporary.clear();
    initial.next_occurrence = u32::MAX;
    let snapshot = initial.clone();
    assert!(matches!(
        prepare_unit(
            &initial,
            &unit(vec![atom(0), atom(1)], vec![]),
            &BTreeSet::new(),
            &context(&values)
        ),
        Err(AllocationError::IdentifierExhausted(_))
    ));
    assert_eq!(initial, snapshot);
}

#[test]
fn physical_frame_capacity_is_checked_after_symbolic_storage() {
    use alloc::collections::BTreeMap;
    assert!(
        FinalFrameLayout::new(
            SourceLocalLayout::default(),
            BTreeMap::new(),
            BTreeMap::new(),
            65_532
        )
        .is_ok()
    );
    assert_eq!(
        FinalFrameLayout::new(
            SourceLocalLayout::default(),
            BTreeMap::new(),
            BTreeMap::new(),
            65_536
        )
        .unwrap_err()
        .category,
        DiagnosticCategory::FrameCapacityExceeded
    );
}

#[test]
fn raw_storage_expansion_resolves_symbolic_cells_and_rejects_buried_accesses() {
    use alloc::collections::BTreeMap;

    use crate::{
        lower::expand::expand_allocation_action, masm::Instruction, plan::actions::PlanAction,
    };
    let slot = SpillSlotId::canonical(atom(0));
    let frame = FinalFrameLayout::new(
        SourceLocalLayout {
            offsets: vec![0],
            extent: 3,
        },
        BTreeMap::from([(slot, 3)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    assert_eq!(
        expand_allocation_action(&PlanAction::StoreLane(slot), &frame).unwrap(),
        Some(Instruction::LocStore(3.into()))
    );
    assert_eq!(
        expand_allocation_action(&PlanAction::LoadLane(slot), &frame).unwrap(),
        Some(Instruction::LocLoad(3.into()))
    );
    assert_eq!(
        expand_allocation_action(&PlanAction::Dup { depth: 16 }, &frame)
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
    assert_eq!(
        expand_allocation_action(&PlanAction::LoadLane(SpillSlotId::canonical(atom(1))), &frame)
            .unwrap_err()
            .category,
        DiagnosticCategory::FrameCapacityExceeded
    );
}
