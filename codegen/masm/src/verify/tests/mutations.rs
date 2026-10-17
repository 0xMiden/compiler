use super::*;

#[test]
fn allocator_equality_and_slot_validity_claims_cannot_establish_facts() {
    let mut fixture = Fixture::new(2, 1);
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    let mut plan = fixture.plan();
    plan.contracts.get_mut(&SegmentId(0)).unwrap().aliases =
        LaneAliases::new(vec![(Fixture::input(0), Fixture::input(1))]);
    assert_eq!(
        verify(&plan, &fixture.frame(), body(vec![Instruction::Drop]))
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidEdgeTransfer
    );
    let mut plan = identity_plan();
    let slot = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
    plan.spill_slots = vec![slot];
    plan.contracts
        .get_mut(&SegmentId(0))
        .unwrap()
        .required_spill_slots
        .push((slot, Atom::new(ValueId(0), 0)));
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    assert_eq!(
        verify(&plan, &frame, body(vec![])).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn omitting_allocator_liveness_claims_does_not_make_lost_inputs_dead() {
    let mut plan = identity_plan();
    plan.contracts.get_mut(&SegmentId(0)).unwrap().required_spill_slots.clear();
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::Drop,
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
        ],
    );
    let mut code = AnnotatedMasmBody::new();
    code.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    code.push_instruction(Instruction::Drop, SelectedStepId(u32::MAX));
    code.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
    code.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    code.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    assert_eq!(
        verify(&plan, &frame(), code).unwrap_err().category,
        DiagnosticCategory::UnavailableValue
    );
}

#[test]
fn actual_instruction_mutations_and_unsupported_effects_fail_closed() {
    assert_eq!(
        verify(&identity_plan(), &frame(), body(vec![Instruction::U32Div]))
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
    let mut fixture = Fixture::new(17, 17);
    fixture.proof.aliases = LaneAliases::new(
        (0..17).map(|index| (Fixture::output(index), Fixture::input(index))).collect(),
    );
    fixture.run(vec![]).unwrap();
    // A legal raw index can still reach the protected tail when owned state is short.
    assert_eq!(
        verify(&identity_plan(), &frame(), body(vec![Instruction::Dup15]))
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
}
