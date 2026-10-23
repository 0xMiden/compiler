use super::*;

// These fixtures catch token allocation per location, one-arm initialization, and
// unchecked loop parameter renaming. All execute actual annotated instructions.
#[test]
fn branch_merge_interns_equal_vectors_across_stack_and_memory() {
    let mut fixture = Fixture::new(1, 2).scratch(1);
    fixture.proof.equal_outputs.push((0, 1));
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    let arm = |value| {
        ops(vec![
            push(value),
            inst(Instruction::Dup0),
            inst(Instruction::LocStore(0.into())),
            inst(Instruction::Dup0),
        ])
    };
    fixture
        .run(vec![AnnotatedOp::If {
            id: ControlId(0),
            then_body: arm(11),
            else_body: arm(29),
        }])
        .unwrap();
}

#[test]
fn one_arm_uninitialized_is_unavailable() {
    let fixture = Fixture::new(1, 1).scratch(1);
    let arm = ops(vec![push(11), inst(Instruction::LocStore(0.into()))]);
    let error = fixture
        .run(vec![
            AnnotatedOp::If {
                id: ControlId(0),
                then_body: arm,
                else_body: ops(vec![]),
            },
            inst(Instruction::LocLoad(0.into())),
        ])
        .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
}

#[test]
fn sole_normal_arm_initialization_survives_trapping_arm() {
    let mut fixture = Fixture::new(1, 1).scratch(1);
    fixture.effects.may_trap = true;
    fixture
        .run(vec![
            AnnotatedOp::If {
                id: ControlId(0),
                then_body: ops(vec![push(11), inst(Instruction::LocStore(0.into()))]),
                else_body: ops(vec![push(0), inst(Instruction::Assert)]),
            },
            inst(Instruction::LocLoad(0.into())),
        ])
        .unwrap();
}

#[test]
fn join_does_not_equal_an_incoming_token() {
    let mut fixture = Fixture::new(2, 1);
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(1))]);
    let error = fixture
        .run(vec![AnnotatedOp::If {
            id: ControlId(0),
            then_body: ops(vec![]),
            else_body: ops(vec![inst(Instruction::Drop), push(29)]),
        }])
        .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::UnavailableValue);
}

fn loop_fixture(equal: bool) -> Fixture {
    let mut fixture = Fixture::new(1, 2).scratch(1);
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0), LoopPosition::Carry(if equal { 0 } else { 1 })],
            cells: vec![],
        },
    );
    fixture
}

#[test]
fn equal_initial_parameters_diverge() {
    let code = vec![
        inst(Instruction::Dup0),
        push(1),
        AnnotatedOp::While {
            id: ControlId(0),
            body: ops(vec![push(1), inst(Instruction::Add), push(0)]),
        },
    ];
    loop_fixture(false).run(code.clone()).unwrap();
    assert_eq!(
        loop_fixture(true).run(code).unwrap_err().category,
        DiagnosticCategory::InvalidEdgeTransfer
    );
}

#[test]
fn carried_stack_advances_but_cell_is_stale() {
    let mut fixture = Fixture::new(1, 1).scratch(1);
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![(
                FrameRef::Scratch {
                    object: ScratchId(0),
                    offset: 0,
                },
                LoopPosition::Carry(0),
            )],
        },
    );
    let code = vec![
        inst(Instruction::Dup0),
        inst(Instruction::LocStore(0.into())),
        push(1),
        AnnotatedOp::While {
            id: ControlId(0),
            body: ops(vec![push(1), inst(Instruction::Add), push(0)]),
        },
    ];
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::InvalidEdgeTransfer);
}

#[test]
fn zero_trip_does_not_initialize_scratch() {
    let mut fixture = Fixture::new(1, 1).scratch(1);
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Invariant],
            cells: vec![],
        },
    );
    let code = vec![
        inst(Instruction::Dup0),
        AnnotatedOp::While {
            id: ControlId(0),
            body: ops(vec![push(11), inst(Instruction::LocStore(0.into())), push(0)]),
        },
        inst(Instruction::Drop),
        inst(Instruction::LocLoad(0.into())),
    ];
    assert_eq!(
        fixture.run(code).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn nested_loop_tokens_do_not_escape_scope() {
    let mut fixture = Fixture::new(1, 1);
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    fixture.proof.loops.insert(
        ControlId(1),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    fixture
        .run(vec![
            inst(Instruction::Dup0),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(vec![
                    push(1),
                    AnnotatedOp::While {
                        id: ControlId(1),
                        body: ops(vec![push(1), inst(Instruction::Add), push(0)]),
                    },
                    push(0),
                ]),
            },
        ])
        .unwrap();
    // An inner loop with the same parameter number cannot establish an alias to the input.
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    let error = fixture
        .run(vec![
            inst(Instruction::Dup0),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(vec![
                    push(1),
                    AnnotatedOp::While {
                        id: ControlId(1),
                        body: ops(vec![push(1), inst(Instruction::Add), push(0)]),
                    },
                    push(0),
                ]),
            },
        ])
        .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::UnavailableValue);
}

#[test]
fn internal_loop_preserves_allocator_owned_suffix() {
    let mut fixture = Fixture::new(2, 2);
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(1), Fixture::input(1))]);
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    let code = vec![
        push(1),
        AnnotatedOp::While {
            id: ControlId(0),
            body: ops(vec![push(1), inst(Instruction::Add), push(0)]),
        },
    ];
    fixture.run(code).unwrap();
}

/// The loop consumes its condition while a separate procedure argument remains live.
/// Allocation may keep that argument only in a spill cell, outside the unit's schema.
struct SpilledLoop {
    proof: UnitEvidence,
}

impl GeneratedDefinition for SpilledLoop {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(482)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut definition = Fixture::new(2, 0).instantiate(parameters)?;
        definition.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        definition.program.source_map.get_mut(&SourceStepId(0)).unwrap().origin =
            SourceOrigin::Generated {
                definition: self.id(),
                stage: 0,
            };
        let unit = &mut definition.program.segments[0].units[0];
        unit.semantic_inputs = vec![ValueId(1)];
        unit.stack_inputs = vec![Fixture::input(1)];
        definition.program.edges[0].bindings = vec![ParameterBinding {
            destination: ValueId(0),
            source: ValueId(0),
        }];
        definition.program.abi = ProcedureAbi::new(CallingConvention::SameContext, 2, 1)?;
        definition.units.insert(SelectedStepId(0), self.proof.clone());
        Ok(definition)
    }
}

/// Exercise physical stores and reloads, including two logical slots sharing one cell.
fn spilled_loop(
    code: Vec<AnnotatedOp>,
    cached: bool,
    explicit: Option<LoopPosition>,
    reload_unstored_alias: bool,
) -> Result<VerifiedProcedure, VerificationError> {
    use crate::lower::expand::expand_allocation_action;

    let slot = SpillSlotId::canonical(Fixture::input(0));
    let alias = SpillSlotId::temporary(SelectedStepId(0), 0);
    let mut definition = SpilledLoop {
        proof: UnitEvidence::default(),
    };
    for id in 0..2 {
        definition.proof.loops.insert(
            ControlId(id),
            LoopSchema {
                stack: vec![],
                cells: explicit.into_iter().map(|kind| (FrameRef::Spill(slot), kind)).collect(),
            },
        );
    }
    let selected = check_selection(
        definition.instantiate(&[])?.program,
        &SourceContext::Generated(&definition),
    )?;
    let mut plan = Fixture::new(2, 0).plan();
    plan.selected = selected;
    plan.spill_slots = vec![slot, alias];
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0), (alias, 0)]),
        BTreeMap::new(),
        4,
    )?;
    let mut actions = vec![];
    if cached {
        actions.push(PlanAction::Dup { depth: 0 });
    }
    actions.push(PlanAction::StoreLane(slot));
    if cached {
        actions.push(PlanAction::Swap { depth: 1 });
    }
    actions.push(PlanAction::RunLowering {
        step: SelectedStepId(0),
    });
    actions.push(PlanAction::LoadLane(if reload_unstored_alias { alias } else { slot }));
    if cached {
        actions.push(PlanAction::Drop);
    }
    plan.segment_actions.insert(SegmentId(0), actions.clone());
    let mut body = ops(vec![AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0)))]);
    for action in actions {
        if matches!(action, PlanAction::RunLowering { .. }) {
            body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
            for op in &code {
                body.push(op.clone());
            }
            body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
        } else if let Some(instruction) = expand_allocation_action(&action, &frame)? {
            body.push_instruction(instruction, SelectedStepId(0));
        }
    }
    body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    verify(&plan, &frame, body)
}

fn internal_while(body: Vec<AnnotatedOp>) -> AnnotatedOp {
    AnnotatedOp::While {
        id: ControlId(0),
        body: ops(body),
    }
}

#[test]
fn internal_loop_preserves_spilled_live_value_and_cached_copy() {
    for cached in [false, true] {
        for explicit in [None, Some(LoopPosition::Invariant)] {
            for condition in [None, Some(0), Some(1)] {
                let mut code = vec![];
                if let Some(condition) = condition {
                    code.extend([inst(Instruction::Drop), push(condition)]);
                }
                code.push(internal_while(vec![push(0)]));
                spilled_loop(code, cached, explicit, false).unwrap();
            }
        }
    }
}

#[test]
fn nested_internal_loops_preserve_spill_facts() {
    spilled_loop(
        vec![internal_while(vec![
            push(1),
            AnnotatedOp::While {
                id: ControlId(1),
                body: ops(vec![push(0)]),
            },
            push(0),
        ])],
        false,
        None,
        false,
    )
    .unwrap();
}

#[test]
fn internal_loop_cannot_write_an_implicit_spill_invariant() {
    let error = spilled_loop(
        vec![internal_while(vec![push(17), inst(Instruction::LocStore(0.into())), push(0)])],
        false,
        None,
        false,
    )
    .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert_eq!(error.message, "undeclared compiler-storage access inside lowering");
}

#[test]
fn internal_loop_does_not_initialize_an_unstored_logical_alias() {
    let error = spilled_loop(vec![internal_while(vec![push(0)])], true, None, true).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert_eq!(error.message, "abstract slot never stored on this path");
}

#[test]
fn internal_loop_explicit_carry_cannot_hide_a_required_spill_identity() {
    let error = spilled_loop(
        vec![internal_while(vec![push(0)])],
        false,
        Some(LoopPosition::Carry(0)),
        false,
    )
    .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidEdgeTransfer);
    assert_eq!(error.message, "loop generalized a required external identity");
}
