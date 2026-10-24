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
