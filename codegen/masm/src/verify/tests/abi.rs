use super::*;

fn target() -> crate::masm::InvocationTarget {
    crate::masm::InvocationTarget::Symbol(crate::masm::Ident::new("callee").unwrap())
}
fn invocation(family: Invocation) -> Instruction {
    match family {
        Invocation::Call => Instruction::Call(target()),
        Invocation::DynCall => Instruction::DynCall,
        Invocation::SysCall => Instruction::SysCall(target()),
        Invocation::Exec => Instruction::Exec(target()),
        Invocation::DynExec => Instruction::DynExec,
    }
}
fn context_fixture(family: Invocation, args: u32, results: u32) -> (Fixture, Vec<AnnotatedOp>) {
    let convention = if family == Invocation::SysCall {
        CallingConvention::Kernel
    } else {
        CallingConvention::Context
    };
    let abi = ProcedureAbi::new(convention, args, results).unwrap();
    let mut fixture = Fixture::new(args, results);
    fixture.effects.call = Some(abi.invocation(family).unwrap());
    let mut code = vec![];
    let selector = if family == Invocation::DynCall {
        fixture = fixture.scratch(4);
        for (offset, value) in [11, 13, 17, 19].into_iter().enumerate() {
            code.extend([push(value), inst(Instruction::LocStore((offset as u16).into()))]);
        }
        Some((
            FrameRef::Scratch {
                object: ScratchId(0),
                offset: 0,
            },
            [11, 13, 17, 19]
                .into_iter()
                .map(|value| StoredValue::Literal(Felt::new_unchecked(value)))
                .collect(),
        ))
    } else {
        None
    };
    fixture.proof.calls.push(CallSite {
        path: vec![],
        instruction: invocation(family),
        abi,
        inputs: (0..args)
            .map(|index| CallArgument::Exact(StoredValue::Atom(Fixture::input(index))))
            .collect(),
        truncate: false,
        outputs: vec![],
        publications: (0..results).map(|index| (Fixture::output(index), index as u8)).collect(),
        selector,
    });
    for _ in args..16 {
        code.push(push(0));
        if args != 0 {
            code.push(inst(Primitive::MoveDown(args as u8).instruction().unwrap()));
        }
    }
    if family == Invocation::DynCall {
        code.push(inst(Instruction::Locaddr(0.into())));
    }
    code.push(inst(invocation(family)));
    code.extend(context_result_cleanup(results as u8).unwrap().into_iter().map(inst));
    (fixture, code)
}

#[test]
fn context_windows_all_widths_include_syscall_and_dynamic_root() {
    for family in [Invocation::Call, Invocation::DynCall, Invocation::SysCall] {
        for args in 0..=16 {
            for results in [0, args, 16] {
                let (fixture, code) = context_fixture(family, args, results);
                fixture.run(code).unwrap();
            }
        }
    }
}
#[test]
fn wrong_invocation_family_is_rejected() {
    let (fixture, mut code) = context_fixture(Invocation::Call, 0, 0);
    let index = code
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Call(_),
                    ..
                }
            )
        })
        .unwrap();
    code[index] = inst(Instruction::Exec(target()));
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}
#[test]
fn nonzero_context_padding_is_rejected_including_syscall() {
    for family in [Invocation::Call, Invocation::DynCall, Invocation::SysCall] {
        let (fixture, mut code) = context_fixture(family, 0, 0);
        let index = code.iter().position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Push(_),
                    ..
                }
            ) && family != Invocation::DynCall
        });
        let index = index.unwrap_or(8);
        code[index] = push(91);
        assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
    }
}
#[test]
fn retained_live_value_in_clobbered_nonresult_window_is_rejected() {
    for family in [Invocation::Call, Invocation::DynCall, Invocation::SysCall] {
        let (mut fixture, mut code) = context_fixture(family, 1, 0);
        fixture.outputs = 1;
        fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
        // Leave the final clobbered window lane as the alleged retained value.
        code.pop();
        assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::UnavailableValue);
    }
}
#[test]
fn bad_result_cleanup_cannot_publish_nonresult_lane() {
    let (fixture, mut code) = context_fixture(Invocation::Call, 0, 1);
    let call = code
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Call(_),
                    ..
                }
            )
        })
        .unwrap();
    code[call + 1] = inst(Instruction::Nop);
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}
#[test]
fn dynamic_root_requires_exact_digest_and_address_provenance() {
    let (fixture, mut code) = context_fixture(Invocation::DynCall, 0, 0);
    code[0] = push(97);
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
    let (fixture, mut code) = context_fixture(Invocation::DynCall, 0, 0);
    let index = code
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Locaddr(_),
                    ..
                }
            )
        })
        .unwrap();
    code[index] = push(0);
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}
#[test]
fn context_truncation_is_forbidden_on_exec_return() {
    let abi = ProcedureAbi::new(CallingConvention::SameContext, 1, 1).unwrap();
    let mut fixture = Fixture::new(1, 1);
    fixture.proof.calls.push(CallSite {
        path: vec![],
        instruction: Instruction::Exec(target()),
        abi,
        inputs: vec![],
        truncate: true,
        outputs: vec![],
        publications: vec![],
        selector: None,
    });
    assert_eq!(
        fixture.run(vec![inst(Instruction::Exec(target()))]).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
}

#[test]
fn conditional_call_sites_are_checked_per_registered_arm() {
    let (mut fixture, code) = context_fixture(Invocation::Call, 0, 0);
    fixture.inputs = 1;
    fixture.proof.calls[0].path = vec![CallPath::Then(ControlId(0))];
    fixture
        .run(vec![AnnotatedOp::If {
            id: ControlId(0),
            then_body: ops(code.clone()),
            else_body: ops(vec![]),
        }])
        .unwrap();
    let mut missing = code;
    missing.retain(|op| {
        !matches!(
            op,
            AnnotatedOp::Instruction {
                instruction: Instruction::Call(_),
                ..
            }
        )
    });
    assert_eq!(
        fixture
            .run(vec![AnnotatedOp::If {
                id: ControlId(0),
                then_body: ops(missing),
                else_body: ops(vec![])
            }])
            .unwrap_err()
            .category,
        DiagnosticCategory::AbiMismatch
    );
}

#[test]
fn loop_call_site_is_scoped_to_inductive_iteration() {
    let (mut fixture, mut code) = context_fixture(Invocation::Call, 0, 0);
    fixture.proof.calls[0].path = vec![CallPath::Loop(ControlId(0))];
    fixture.proof.loops.insert(ControlId(0), LoopSchema::default());
    code.push(push(0));
    fixture
        .run(vec![
            push(1),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(code),
            },
        ])
        .unwrap();
}

#[test]
fn internal_call_accepts_computed_local_argument() {
    let (mut fixture, mut code) = context_fixture(Invocation::Call, 1, 0);
    fixture.proof.calls[0].inputs = vec![CallArgument::Local];
    code.splice(0..0, [push(1), inst(Instruction::Add)]);
    fixture.run(code).unwrap();
}

#[test]
fn borrowed_scratch_writes_invalidate_old_identity_and_require_output_relation() {
    let abi = ProcedureAbi::new(CallingConvention::SameContext, 1, 1).unwrap();
    let mut fixture = Fixture::new(0, 1).scratch(1);
    fixture.effects.call = Some(abi.invocation(Invocation::Exec).unwrap());
    fixture.scratch_effects.push(ScratchEffect::Borrow {
        object: ScratchId(0),
        offset: 0,
        size: 1,
        read: true,
        write: true,
        definite_write: false,
        retained: false,
    });
    fixture.proof.calls.push(CallSite {
        path: vec![],
        instruction: Instruction::Exec(target()),
        abi,
        inputs: vec![CallArgument::Local],
        truncate: false,
        outputs: vec![],
        publications: vec![(Fixture::output(0), 0)],
        selector: None,
    });
    let code = vec![
        push(17),
        inst(Instruction::LocStore(0.into())),
        inst(Instruction::Locaddr(0.into())),
        inst(Instruction::Exec(target())),
        inst(Instruction::LocLoad(0.into())),
        inst(Instruction::Drop),
    ];
    assert_eq!(
        fixture.run(code.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    fixture.scratch_effects[0] = ScratchEffect::Borrow {
        object: ScratchId(0),
        offset: 0,
        size: 1,
        read: true,
        write: true,
        definite_write: true,
        retained: false,
    };
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    assert_eq!(
        fixture.run(code.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    fixture.proof.calls[0].outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    fixture.run(code).unwrap();
}

#[test]
fn context_entry_owns_arbitrary_nonargument_lanes_and_returns_at_exact_depth() {
    for args in 0..=16 {
        let mut fixture = Fixture::new(args, args);
        fixture.convention = CallingConvention::Context;
        fixture.proof.aliases = LaneAliases::new(
            (0..args).map(|index| (Fixture::output(index), Fixture::input(index))).collect(),
        );
        let mut plan = fixture.plan();
        plan.contracts.get_mut(&SegmentId(0)).unwrap().tail = TailPolicy::ContextWindow;
        let mut actions = vec![];
        for _ in args..16 {
            if args == 1 {
                actions.push(PlanAction::Swap { depth: 1 });
            } else if args > 1 {
                actions.push(PlanAction::MoveUp { depth: args as u8 });
            }
            actions.push(PlanAction::Drop);
        }
        actions.push(PlanAction::RunLowering {
            step: SelectedStepId(0),
        });
        plan.segment_actions.insert(SegmentId(0), actions);
        let mut code = vec![AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0)))];
        code.extend(context_result_cleanup(args as u8).unwrap().into_iter().map(|instruction| {
            AnnotatedOp::Instruction {
                instruction,
                unit: SelectedStepId(u32::MAX),
            }
        }));
        code.extend([
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]);
        verify(&plan, &fixture.frame(), ops(code)).unwrap();
    }
}
#[test]
fn root_entry_models_all_sixteen_vm_lanes() {
    let mut fixture = Fixture::new(16, 16);
    fixture.convention = CallingConvention::Root;
    fixture.proof.aliases = LaneAliases::new(
        (0..16).map(|index| (Fixture::output(index), Fixture::input(index))).collect(),
    );
    let mut plan = fixture.plan();
    plan.contracts.get_mut(&SegmentId(0)).unwrap().tail = TailPolicy::RootWindow;
    verify(&plan, &fixture.frame(), body(vec![])).unwrap();
    let mut invalid = Fixture::new(1, 1);
    invalid.convention = CallingConvention::Root;
    assert_eq!(invalid.run(vec![]).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}
#[test]
fn context_call_preserves_owned_overflow_above_an_unknown_tail() {
    for family in [Invocation::Call, Invocation::DynCall, Invocation::SysCall] {
        let (mut fixture, code) = context_fixture(family, 0, 0);
        fixture.inputs = 17;
        fixture.outputs = 17;
        fixture.proof.aliases = LaneAliases::new(
            (0..17).map(|index| (Fixture::output(index), Fixture::input(index))).collect(),
        );
        fixture.run(code).unwrap();
    }
}

#[test]
fn source_local_call_arguments_do_not_require_compiler_scratch_borrow() {
    let mut fixture = Fixture::new(0, 0);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 1,
    };
    let abi = ProcedureAbi::new(CallingConvention::SameContext, 1, 0).unwrap();
    fixture.effects.call = Some(abi.invocation(Invocation::Exec).unwrap());
    fixture.proof.calls.push(CallSite {
        path: vec![],
        instruction: Instruction::Exec(target()),
        abi,
        inputs: vec![CallArgument::Local],
        truncate: false,
        outputs: vec![],
        publications: vec![],
        selector: None,
    });
    fixture
        .run(vec![inst(Instruction::Locaddr(0.into())), inst(Instruction::Exec(target()))])
        .unwrap();
}

#[test]
fn branch_compiler_origin_cannot_escape_through_call_arguments() {
    for other in [inst(Instruction::Locaddr(1.into())), push(77)] {
        let (fixture, code) = context_fixture(Invocation::Call, 1, 0);
        let mut fixture = fixture.scratch(2);
        fixture.proof.calls[0].inputs = vec![CallArgument::Local];
        let mut program = vec![super::provenance::pointer_branch(other)];
        program.extend(code);
        assert_eq!(fixture.run(program).unwrap_err().category, DiagnosticCategory::AbiMismatch);
    }
    let (mut fixture, code) = context_fixture(Invocation::Call, 1, 0);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0, 1],
        extent: 2,
    };
    fixture.proof.calls[0].inputs = vec![CallArgument::Local];
    let mut program = vec![super::provenance::pointer_branch(inst(Instruction::Locaddr(1.into())))];
    program.extend(code);
    fixture.run(program).unwrap();
}

fn result_code(valid: bool) -> (Fixture, Vec<AnnotatedOp>) {
    let (fixture, mut code) = context_fixture(Invocation::Call, 0, 1);
    if !valid {
        // Drop the true result and fourteen clobbered lanes, leaving one nonresult.
        code.truncate(17);
        code.extend((0..15).map(|_| inst(Instruction::Drop)));
    }
    (fixture, code)
}

#[test]
fn conditional_result_obligation_survives_arm_without_call() {
    for both_arms_call in [false, true] {
        for valid in [true, false] {
            let (mut fixture, code) = result_code(valid);
            fixture.inputs = 1;
            fixture.proof.calls[0].path = vec![CallPath::Then(ControlId(0))];
            let otherwise = if both_arms_call {
                let mut site = fixture.proof.calls[0].clone();
                site.path = vec![CallPath::Else(ControlId(0))];
                fixture.proof.calls.push(site);
                result_code(true).1
            } else {
                vec![push(7)]
            };
            let result = fixture.run(vec![AnnotatedOp::If {
                id: ControlId(0),
                then_body: ops(code),
                else_body: ops(otherwise),
            }]);
            if valid {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
            }
        }
    }
}

#[test]
fn conditional_iterated_result_keeps_preceding_iteration_obligation() {
    for valid in [true, false] {
        let (mut fixture, code) = result_code(true);
        fixture.inputs = 1;
        fixture.proof.calls[0].path =
            vec![CallPath::Loop(ControlId(0)), CallPath::Then(ControlId(1))];
        fixture.proof.loops.insert(
            ControlId(0),
            LoopSchema {
                stack: vec![LoopPosition::Carry(0), LoopPosition::Invariant],
                cells: vec![],
            },
        );
        let mut taken = vec![inst(Instruction::Drop)];
        taken.extend(code);
        let otherwise = if valid {
            vec![]
        } else {
            vec![inst(Instruction::Drop), push(7)]
        };
        let result = fixture.run(vec![
            push(7),
            push(1),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(vec![
                    inst(Instruction::Dup1),
                    AnnotatedOp::If {
                        id: ControlId(1),
                        then_body: ops(taken),
                        else_body: ops(otherwise),
                    },
                    push(0),
                ]),
            },
            inst(Instruction::Swap1),
            inst(Instruction::Drop),
        ]);
        if valid {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
        }
    }
}

#[test]
fn iterated_result_obligation_survives_carried_exit() {
    for valid in [true, false] {
        let (mut fixture, code) = result_code(valid);
        fixture.proof.calls[0].path = vec![CallPath::Loop(ControlId(0))];
        fixture.proof.loops.insert(
            ControlId(0),
            LoopSchema {
                stack: vec![LoopPosition::Carry(0)],
                cells: vec![],
            },
        );
        let mut body = vec![inst(Instruction::Drop)];
        body.extend(code);
        body.push(push(0));
        let result = fixture.run(vec![
            push(7),
            push(1),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(body),
            },
        ]);
        if valid {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
        }
    }
}

#[test]
fn zero_trip_loop_keeps_entry_result_obligation() {
    for carried in [false, true] {
        for valid in [true, false] {
            let (mut fixture, mut code) = result_code(true);
            fixture.inputs = 1;
            fixture.proof.loops.insert(
                ControlId(0),
                LoopSchema {
                    stack: vec![if carried {
                        LoopPosition::Carry(0)
                    } else {
                        LoopPosition::Invariant
                    }],
                    cells: vec![],
                },
            );
            code.extend([
                inst(Instruction::Swap1),
                AnnotatedOp::While {
                    id: ControlId(0),
                    body: ops(vec![push(0)]),
                },
            ]);
            if !valid {
                code.extend([inst(Instruction::Drop), push(7)]);
            }
            let result = fixture.run(code);
            if valid {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
            }
        }
    }
}

fn nested_result_code(
    valid: bool,
    inner_predicate: u64,
    corrupt_exit: bool,
) -> (Fixture, Vec<AnnotatedOp>) {
    let (mut fixture, code) = result_code(valid);
    fixture.inputs = 1;
    fixture.proof.calls[0].path = vec![CallPath::Loop(ControlId(0)), CallPath::Loop(ControlId(1))];
    for id in [ControlId(0), ControlId(1)] {
        fixture.proof.loops.insert(
            id,
            LoopSchema {
                stack: vec![LoopPosition::Carry(0), LoopPosition::Invariant],
                cells: vec![],
            },
        );
    }
    let mut inner_body = vec![inst(Instruction::Drop)];
    inner_body.extend(code);
    inner_body.push(push(0));
    let mut outer_body = vec![
        if inner_predicate == 2 {
            inst(Instruction::Dup1)
        } else {
            push(inner_predicate)
        },
        AnnotatedOp::While {
            id: ControlId(1),
            body: ops(inner_body),
        },
    ];
    if corrupt_exit {
        outer_body.extend([inst(Instruction::Drop), push(7)]);
    }
    outer_body.push(push(0));
    (
        fixture,
        vec![
            push(7),
            push(1),
            AnnotatedOp::While {
                id: ControlId(0),
                body: ops(outer_body),
            },
            inst(Instruction::Swap1),
            inst(Instruction::Drop),
        ],
    )
}

#[test]
fn nested_result_obligation_rebinds_to_each_loop_header() {
    for inner_predicate in [0, 1, 2] {
        for valid in [true, false] {
            let (fixture, code) = nested_result_code(valid, inner_predicate, false);
            let result = fixture.run(code);
            if valid || inner_predicate == 0 {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
            }
        }
    }
}

#[test]
fn nested_result_zero_trip_does_not_forget_applicable_prior_result() {
    let (fixture, code) = nested_result_code(true, 2, true);
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
    // A proven zero-trip inner call contributes no result requirement.
    let (fixture, code) = nested_result_code(false, 0, true);
    fixture.run(code).unwrap();
}

#[test]
fn conditional_entry_result_rebinds_on_zero_trip_and_no_call_paths() {
    for known_call in [Some(false), Some(true), None] {
        for corrupt in [false, true] {
            let (mut fixture, call) = result_code(true);
            fixture.inputs = 2;
            fixture.proof.calls[0].path = vec![CallPath::Then(ControlId(1))];
            fixture.proof.loops.insert(
                ControlId(0),
                LoopSchema {
                    stack: vec![LoopPosition::Carry(0)],
                    cells: vec![],
                },
            );
            let mut code = vec![];
            if let Some(taken) = known_call {
                code.extend([inst(Instruction::Drop), push(u64::from(taken))]);
            }
            let mut body = if corrupt {
                vec![inst(Instruction::Drop), push(7)]
            } else {
                vec![]
            };
            body.push(push(0));
            code.extend([
                AnnotatedOp::If {
                    id: ControlId(1),
                    then_body: ops(call),
                    else_body: ops(vec![push(7)]),
                },
                inst(Instruction::Swap1),
                AnnotatedOp::While {
                    id: ControlId(0),
                    body: ops(body),
                },
            ]);
            let result = fixture.run(code);
            if !corrupt || known_call == Some(false) {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
            }
        }
    }
}
