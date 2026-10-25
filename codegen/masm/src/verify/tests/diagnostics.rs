use super::*;

#[test]
fn failed_preparation_instruction_reports_pending_unit() {
    let fixture = Fixture::new(0, 0);
    let mut plan = located_plan(fixture.clone());
    plan.segment_actions.get_mut(&SegmentId(0)).unwrap().insert(0, PlanAction::Drop);
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let mut cursor = replay::Cursor {
        segment: Some(SegmentId(0)),
        ..Default::default()
    };
    let failure = verifier
        .execute(
            &ops(vec![inst(Instruction::Drop)]),
            stack::State::new(vec![], BTreeMap::new()),
            &mut cursor,
        )
        .unwrap_err();
    assert_eq!(failure.category, DiagnosticCategory::IllegalStackAccess);
    assert_eq!(failure.step, Some(SelectedStepId(0)));
    assert_eq!(failure.origin, location());
}

#[test]
fn failed_segment_cleanup_instruction_has_no_pending_unit() {
    let fixture = Fixture::new(0, 0);
    let mut plan = located_plan(fixture.clone());
    plan.segment_actions.get_mut(&SegmentId(0)).unwrap().push(PlanAction::Drop);
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let mut cursor = replay::Cursor {
        segment: Some(SegmentId(0)),
        phase: 1,
        action: 1,
        ..Default::default()
    };
    let failure = verifier
        .execute(
            &ops(vec![inst(Instruction::Drop)]),
            stack::State::new(vec![], BTreeMap::new()),
            &mut cursor,
        )
        .unwrap_err();
    assert_eq!(failure.category, DiagnosticCategory::IllegalStackAccess);
    assert_eq!(failure.step, None);
    assert_eq!(failure.origin, SourceLocation::default());
}

#[test]
fn failed_edge_instruction_does_not_borrow_destination_unit_context() {
    let fixture = Fixture::new(0, 0);
    let mut plan = located_plan(fixture.clone());
    plan.edge_actions.insert(EdgeId(0), vec![PlanAction::Drop]);
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let mut cursor = replay::Cursor {
        segment: Some(SegmentId(0)),
        edge: Some(EdgeId(0)),
        ..Default::default()
    };
    let failure = verifier
        .execute(
            &ops(vec![inst(Instruction::Drop)]),
            stack::State::new(vec![], BTreeMap::new()),
            &mut cursor,
        )
        .unwrap_err();
    assert_eq!(failure.category, DiagnosticCategory::IllegalStackAccess);
    assert_eq!(failure.step, None);
    assert_eq!(failure.origin, SourceLocation::default());
}

struct Located(Fixture);

impl GeneratedDefinition for Located {
    fn id(&self) -> GeneratedDefinitionId {
        self.0.id()
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = self.0.instantiate(parameters)?;
        evidence.program.source_map.get_mut(&SourceStepId(0)).unwrap().location = location();
        Ok(evidence)
    }
}

fn location() -> SourceLocation {
    let unknown = crate::masm::SourceSpan::UNKNOWN;
    SourceLocation {
        span: crate::masm::SourceSpan::new(unknown.source_id(), 10..20),
        inline_chain: vec![crate::masm::SourceSpan::new(unknown.source_id(), 30..40)],
    }
}

fn located_plan(fixture: Fixture) -> ProcedurePlan {
    let definition = Located(fixture.clone());
    let mut plan = fixture.plan();
    plan.selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    plan
}

#[test]
fn replay_input_mismatch_reports_selected_location_and_known_atoms() {
    let fixture = Fixture::new(2, 0);
    let plan = located_plan(fixture.clone());
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let first = verifier.tokens.fresh();
    let second = verifier.tokens.fresh();
    let state = stack::State::new(
        vec![second, first],
        BTreeMap::from([(Fixture::input(0), first), (Fixture::input(1), second)]),
    );
    let mut cursor = replay::Cursor {
        segment: Some(SegmentId(0)),
        ..Default::default()
    };
    let error = verifier
        .execute(
            &ops(vec![AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0)))]),
            state,
            &mut cursor,
        )
        .unwrap_err();
    assert_eq!(error.origin, location());
    assert_eq!(error.step, Some(SelectedStepId(0)));
    assert_eq!(error.depth, Some(0));
    assert_eq!(error.expected, Some(Fixture::input(0)));
    assert_eq!(error.actual, Some(Fixture::input(1)));
}

#[test]
fn publication_failure_retains_unit_context_without_fabricating_actual_atom() {
    let mut fixture = Fixture::new(1, 1);
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    let plan = located_plan(fixture.clone());
    let body = ops(vec![
        AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
        AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
        inst(Instruction::Drop),
        push(7),
        AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
        AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
    ]);
    let error = verify(&plan, &fixture.frame(), body).unwrap_err();
    assert_eq!(error.origin, location());
    assert_eq!(error.step, Some(SelectedStepId(0)));
    assert_eq!(error.depth, Some(0));
    assert_eq!(error.expected, Some(Fixture::output(0)));
    assert_eq!(error.actual, None);
}

#[test]
fn selected_unit_rejection_reports_its_source_context() {
    struct Invalid;
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(1)
        }

        fn instantiate(
            &self,
            parameters: &[Felt],
        ) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Located(Fixture::new(0, 1)).instantiate(parameters)?;
            evidence.program.segments[0].units[0].outputs[0].lane = 1;
            Ok(evidence)
        }
    }
    let error = check_selection(
        Invalid.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&Invalid),
    )
    .unwrap_err();
    assert_eq!(error.origin, location());
    assert_eq!(error.step, Some(SelectedStepId(0)));
    assert_eq!(error.depth, None);
    assert_eq!(error.expected, Some(Atom::new(ValueId(100), 1)));
    assert_eq!(error.actual, None);
}

#[test]
fn replay_unbound_failure_has_no_fabricated_source_or_state() {
    let fixture = Fixture::new(0, 0);
    let plan = located_plan(fixture.clone());
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let error = verifier
        .execute(
            &ops(vec![AnnotatedOp::Boundary(Boundary::Control(ControlId(9)))]),
            stack::State::new(vec![], BTreeMap::new()),
            &mut replay::Cursor::default(),
        )
        .unwrap_err();
    assert_eq!(error.origin, SourceLocation::default());
    assert_eq!(error.step, None);
    assert_eq!(error.depth, None);
    assert_eq!(error.expected, None);
    assert_eq!(error.actual, None);
}

#[test]
fn selection_disagreement_reports_the_changed_units_trusted_location() {
    let definition = Located(Fixture::new(1, 1));
    let mut candidate = definition.instantiate(&[]).unwrap().program;
    candidate.segments[0].units[0].stack_inputs.clear();
    let error = check_selection(candidate, &SourceContext::Generated(&definition)).unwrap_err();
    assert_eq!(error.origin, location());
    assert_eq!(error.step, Some(SelectedStepId(0)));
    assert_eq!(error.depth, None);
    assert_eq!(error.actual, None);
}

#[test]
fn unavailable_semantic_lane_reports_expected_atom() {
    let fixture = Fixture::new(1, 0);
    let plan = fixture.plan();
    let frame = fixture.frame();
    let verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let error = verifier
        .available(
            &stack::State::new(vec![], BTreeMap::new()),
            &alloc::collections::BTreeSet::from([Fixture::input(0)]),
        )
        .unwrap_err();
    assert_eq!(error.expected, Some(Fixture::input(0)));
    assert_eq!(error.actual, None);
    assert_eq!(error.depth, None);
}

#[test]
fn call_coverage_failure_reports_the_registered_unit_location() {
    let fixture = Fixture::new(0, 0);
    let plan = located_plan(fixture.clone());
    let body = ops(vec![
        AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
        AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
        inst(Instruction::DynExec),
        AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
        AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
    ]);
    let error = verify(&plan, &fixture.frame(), body).unwrap_err();
    assert_eq!(error.origin, location());
    assert_eq!(error.step, Some(SelectedStepId(0)));
    assert_eq!(error.depth, None);
    assert_eq!(error.expected, None);
}

#[test]
fn matching_step_enrichment_fills_unknown_origin_and_preserves_nested_context() {
    let mut error = error("missing physical identity");
    error.step = Some(SelectedStepId(0));
    let error = error.at_step(SelectedStepId(0), Some(&location()));
    assert_eq!(error.origin, location());

    let mut nested = error.clone();
    nested.step = Some(SelectedStepId(2));
    let enclosing = SourceLocation::default();
    let nested = nested.at_step(SelectedStepId(0), Some(&enclosing));
    assert_eq!(nested.step, Some(SelectedStepId(2)));
    assert_eq!(nested.origin, location());
}

#[test]
fn destination_contract_mismatch_reports_known_position_and_atoms_without_step() {
    let fixture = Fixture::new(2, 0);
    let plan = fixture.plan();
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let first = verifier.tokens.fresh();
    let second = verifier.tokens.fresh();
    let state = stack::State::new(
        vec![second, first],
        BTreeMap::from([(Fixture::input(0), first), (Fixture::input(1), second)]),
    );
    let error = verifier.check_contract(SegmentId(0), &state).unwrap_err();
    assert_eq!(error.depth, Some(0));
    assert_eq!(error.expected, Some(Fixture::input(0)));
    assert_eq!(error.actual, Some(Fixture::input(1)));
    assert_eq!(error.step, None);
    assert_eq!(error.origin, SourceLocation::default());
}
