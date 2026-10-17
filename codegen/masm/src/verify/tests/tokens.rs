use super::*;

#[test]
fn padding_is_not_caller_tail() {
    assert_eq!(
        Fixture::new(0, 1).run(vec![inst(Instruction::Dup0)]).unwrap_err().category,
        DiagnosticCategory::IllegalStackAccess
    );
    assert_eq!(
        Fixture::new(1, 1)
            .run(vec![inst(Instruction::Drop), inst(Instruction::Dup0)])
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
}

#[test]
fn floor_drain_on_one_branch() {
    let mut fixture = Fixture::new(3, 2).scratch(2);
    fixture.proof.aliases = LaneAliases::new(vec![
        (Fixture::output(0), Fixture::input(1)),
        (Fixture::output(1), Fixture::input(2)),
    ]);
    let arm = ops(vec![
        inst(Instruction::LocStore(0.into())),
        inst(Instruction::LocStore(1.into())),
        inst(Instruction::LocLoad(1.into())),
        inst(Instruction::LocLoad(0.into())),
    ]);
    fixture
        .run(vec![AnnotatedOp::If {
            id: ControlId(0),
            then_body: arm,
            else_body: ops(vec![]),
        }])
        .unwrap();
    let mut drained = stack::Padding::new(2);
    drained.floor(0);
    let mut merged = stack::Padding::new(2);
    merged.union(&drained);
    assert_eq!(merged.0[0], (1 << 14) | (1 << 16));
    assert_eq!(merged.0[15], (1 << 0) | (1 << 1));
}

#[test]
fn repeated_floor_loop_is_bounded() {
    let mut fixture = Fixture::new(2, 2).scratch(2);
    fixture.proof.aliases = LaneAliases::new(vec![
        (Fixture::output(0), Fixture::input(0)),
        (Fixture::output(1), Fixture::input(1)),
    ]);
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Invariant, LoopPosition::Invariant],
            cells: vec![],
        },
    );
    fixture
        .run(vec![
            inst(Instruction::Dup0),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(vec![
                    inst(Instruction::LocStore(0.into())),
                    inst(Instruction::LocStore(1.into())),
                    inst(Instruction::LocLoad(1.into())),
                    inst(Instruction::LocLoad(0.into())),
                    push(0),
                ]),
            },
        ])
        .unwrap();
    let mut padding = stack::Padding::new(2);
    for _ in 0..1000 {
        padding.floor(0);
        padding.floor(2);
    }
    assert_eq!(padding.0[0], 1 << 16);
}

#[test]
fn publication_preserves_token_equalities() {
    let mut fixture = Fixture::new(1, 2).scratch(1);
    fixture.proof.equal_outputs.push((0, 1));
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    fixture
        .run(vec![
            inst(Instruction::Dup0),
            inst(Instruction::LocStore(0.into())),
            inst(Instruction::Dup0),
        ])
        .unwrap();
    let plan = fixture.plan();
    let frame = fixture.frame();
    let mut verifier = replay::Verifier::new(&plan, &frame).unwrap();
    let token = verifier.tokens.intern(super::super::tokens::Identity::Input(0));
    let state = stack::State::new(vec![token], BTreeMap::from([(Fixture::input(0), token)]));
    let body = body(vec![Instruction::Dup0, Instruction::LocStore(0.into()), Instruction::Dup0]);
    let result = verifier.execute(&body, state, &mut replay::Cursor::default()).unwrap().unwrap();
    assert_eq!(result.names[&Fixture::output(0)], result.names[&Fixture::output(1)]);
    assert_eq!(result.cells[&0], result.names[&Fixture::output(0)]);
    assert_eq!(result.names[&Fixture::input(0)], token);
}

#[test]
fn trap_publishes_no_results() {
    let mut fixture = Fixture::new(0, 1);
    fixture.effects.may_trap = true;
    fixture.effects.normal_return = false;
    fixture.run(vec![push(0), inst(Instruction::Assert)]).unwrap();
    assert_eq!(
        fixture
            .run(vec![push(1), inst(Instruction::Assert), push(29)])
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}
