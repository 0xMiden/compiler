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
    let mut code = vec![];
    let selector = if family == Invocation::DynCall {
        fixture = fixture.scratch(4);
        for (offset, value) in [11, 13, 17, 19].into_iter().enumerate() {
            code.extend([push(value), inst(Instruction::LocStore((offset as u16).into()))]);
        }
        Some(SelectorEvidence::FrameRoot {
            reference: FrameRef::Scratch {
                object: ScratchId(0),
                offset: 0,
            },
            root: [11, 13, 17, 19]
                .into_iter()
                .map(|value| StoredValue::Literal(Felt::new_unchecked(value)))
                .collect(),
        })
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
        publications: (0..results).map(|index| (Fixture::output(index), index)).collect(),
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
    // A possible write forgets identity, but cannot uninitialize an initialized felt.
    fixture.run(code.clone()).unwrap();
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
    fixture.proof.scratch_outputs.clear();
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
        // Entry cleanup is an explicit scalar allocation plan. Result cleanup is a separate
        // lowering algorithm and may choose cheaper word operations for the same ABI effect.
        code.extend(plan.segment_actions[&SegmentId(0)].iter().filter_map(|action| {
            let instruction = match *action {
                PlanAction::Swap { depth } => Primitive::Swap(depth).instruction().unwrap(),
                PlanAction::MoveUp { depth } => Primitive::MoveUp(depth).instruction().unwrap(),
                PlanAction::Drop => Instruction::Drop,
                PlanAction::RunLowering { .. } => return None,
                _ => unreachable!("fixture uses only scalar entry cleanup"),
            };
            Some(AnnotatedOp::Instruction {
                instruction,
                unit: SelectedStepId(u32::MAX),
            })
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

fn internal_site(name: &str, arguments: u32, results: u32) -> CallSite {
    CallSite {
        instruction: Instruction::Exec(crate::masm::InvocationTarget::Symbol(
            crate::masm::Ident::new(name).unwrap(),
        )),
        abi: ProcedureAbi::new(CallingConvention::SameContext, arguments, results).unwrap(),
        inputs: vec![CallArgument::Local; arguments as usize],
        path: vec![],
        truncate: false,
        outputs: vec![],
        publications: vec![],
        selector: None,
    }
}

fn heterogeneous_fixture() -> (Fixture, Vec<AnnotatedOp>) {
    let mut fixture = Fixture::new(24, 20);
    let mut first = internal_site("pair", 2, 1);
    first.inputs = (0..2)
        .map(|i| CallArgument::Exact(StoredValue::Atom(Fixture::input(i))))
        .collect();
    let mut second = internal_site("triple", 3, 0);
    second.inputs = vec![
        CallArgument::Local,
        CallArgument::Exact(StoredValue::Atom(Fixture::input(2))),
        CallArgument::Exact(StoredValue::Atom(Fixture::input(3))),
    ];
    let code = vec![inst(first.instruction.clone()), inst(second.instruction.clone())];
    fixture.proof.calls = vec![first, second];
    fixture.proof.aliases =
        LaneAliases::new((0..20).map(|i| (Fixture::output(i), Fixture::input(i + 4))).collect());
    (fixture, code)
}

#[test]
fn heterogeneous_internal_calls_preserve_suffix() {
    let (fixture, code) = heterogeneous_fixture();
    fixture.run(code).unwrap();
}

fn preserving_fixture(inputs: u32, consumed: u32) -> Fixture {
    let mut fixture = Fixture::new(inputs, inputs - consumed);
    fixture.proof.aliases = LaneAliases::new(
        (0..inputs - consumed)
            .map(|i| (Fixture::output(i), Fixture::input(i + consumed)))
            .collect(),
    );
    fixture
}

#[test]
fn repeated_internal_sites_are_all_required() {
    let mut fixture = preserving_fixture(20, 0);
    let site = internal_site("repeat", 0, 0);
    fixture.proof.calls = vec![site.clone(), site.clone()];
    let call = inst(site.instruction);
    fixture.run(vec![call.clone(), call.clone()]).unwrap();
    for count in [0, 1, 3] {
        assert_eq!(
            fixture.run(vec![call.clone(); count]).unwrap_err().category,
            DiagnosticCategory::AbiMismatch,
            "occurrences {count}"
        );
    }
}

// Both sites consume locally introduced arguments and leave the original suffix untouched.
fn local_heterogeneous_calls(path: CallPath) -> (Vec<CallSite>, Vec<AnnotatedOp>) {
    let mut first = internal_site("pair", 2, 1);
    first.path = vec![path];
    first.inputs = [11, 13]
        .into_iter()
        .map(|n| CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(n))))
        .collect();
    let mut second = internal_site("triple", 3, 0);
    second.path = vec![path];
    second.inputs = vec![
        CallArgument::Local,
        CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(17))),
        CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(19))),
    ];
    let mut code: Vec<_> = [19, 17, 13, 11].into_iter().map(push).collect();
    code.extend([inst(first.instruction.clone()), inst(second.instruction.clone())]);
    (vec![first, second], code)
}

#[test]
fn heterogeneous_internal_paths_are_checked() {
    for predicate in [None, Some(0), Some(1)] {
        let mut fixture = preserving_fixture(
            if predicate.is_none() { 21 } else { 20 },
            u32::from(predicate.is_none()),
        );
        let (mut sites, then_code) = local_heterogeneous_calls(CallPath::Then(ControlId(0)));
        let mut other = internal_site("single", 1, 0);
        other.path = vec![CallPath::Else(ControlId(0))];
        other.inputs = vec![CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(23)))];
        let else_code = vec![push(23), inst(other.instruction.clone())];
        sites.push(other);
        fixture.proof.calls = sites;
        let prefix: Vec<_> = predicate.into_iter().map(push).collect();
        let branch = |then_code| {
            let mut code = prefix.clone();
            code.push(AnnotatedOp::If {
                id: ControlId(0),
                then_body: ops(then_code),
                else_body: ops(else_code.clone()),
            });
            code
        };
        fixture.run(branch(then_code.clone())).unwrap();
        let mut missing = then_code;
        missing.pop();
        assert_eq!(
            fixture.run(branch(missing)).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );

        let mut fixture = preserving_fixture(
            if predicate.is_none() { 21 } else { 20 },
            u32::from(predicate.is_none()),
        );
        let (sites, mut iteration) = local_heterogeneous_calls(CallPath::Loop(ControlId(0)));
        fixture.proof.calls = sites;
        fixture.proof.loops.insert(ControlId(0), LoopSchema::default());
        iteration.push(push(0));
        let loop_code = |iteration| {
            let mut code = prefix.clone();
            code.push(AnnotatedOp::While {
                id: ControlId(0),
                body: ops(iteration),
            });
            code
        };
        fixture.run(loop_code(iteration.clone())).unwrap();
        // Static site accounting also checks the body of a known zero-trip loop.
        iteration.remove(4);
        assert_eq!(
            fixture.run(loop_code(iteration)).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );
    }
}

#[test]
fn registered_site_target_family_order_and_count_are_exact() {
    let mut fixture = preserving_fixture(20, 0);
    let first = internal_site("first", 0, 0);
    let second = internal_site("second", 0, 0);
    let code = vec![inst(first.instruction.clone()), inst(second.instruction.clone())];
    fixture.proof.calls = vec![first, second];
    fixture.run(code.clone()).unwrap();
    let Instruction::Exec(first_target) = &fixture.proof.calls[0].instruction else {
        unreachable!()
    };
    let mutations = [
        vec![inst(internal_site("wrong", 0, 0).instruction), code[1].clone()],
        vec![inst(Instruction::Call(first_target.clone())), code[1].clone()],
        vec![code[1].clone(), code[0].clone()],
        vec![code[0].clone()],
        vec![code[0].clone(), code[1].clone(), code[1].clone()],
    ];
    for (index, bad) in mutations.into_iter().enumerate() {
        assert_eq!(
            fixture.run(bad).unwrap_err().category,
            DiagnosticCategory::AbiMismatch,
            "mutation {index}"
        );
    }
    fixture.proof.calls[0].path = vec![CallPath::Then(ControlId(0))];
    assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}

#[test]
fn registered_site_abi_arguments_results_and_publications_are_enforced() {
    let mut fixture = Fixture::new(2, 1);
    let mut site = internal_site("produce", 2, 1);
    site.inputs = (0..2)
        .map(|i| CallArgument::Exact(StoredValue::Atom(Fixture::input(i))))
        .collect();
    site.publications = vec![(Fixture::output(0), 0)];
    let code = vec![inst(site.instruction.clone())];
    fixture.proof.calls = vec![site];
    fixture.run(code.clone()).unwrap();
    for mutation in 0..7 {
        let mut bad = fixture.clone();
        let mut raw = code.clone();
        match mutation {
            0 => {
                bad.proof.calls[0].abi =
                    ProcedureAbi::new(CallingConvention::Context, 2, 1).unwrap()
            }
            1 => {
                bad.proof.calls[0].inputs.pop();
            }
            2 => raw.insert(0, inst(Instruction::Swap1)),
            3 => {
                bad.proof.calls[0].abi =
                    ProcedureAbi::new(CallingConvention::SameContext, 2, 2).unwrap()
            }
            4 => bad.proof.calls[0].publications[0].1 = 1,
            5 => raw.extend([inst(Instruction::Drop), push(7)]),
            6 => raw.insert(0, AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0)))),
            _ => unreachable!(),
        }
        assert!(bad.run(raw).is_err(), "mutation {mutation}");
    }
    let (mut dynamic, code) = context_fixture(Invocation::DynCall, 0, 0);
    dynamic.run(code.clone()).unwrap();
    dynamic.proof.calls[0].selector = None;
    assert_eq!(dynamic.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}

#[test]
fn truncate_site_keeps_registered_boundary_restrictions() {
    for convention in [CallingConvention::Root, CallingConvention::Context] {
        let mut fixture = preserving_fixture(16, 0);
        fixture.convention = convention;
        let mut site = internal_site("cleanup", 0, 0);
        site.truncate = true;
        let instruction = site.instruction.clone();
        fixture.proof.calls = vec![site];
        let mut plan = fixture.plan();
        plan.contracts.get_mut(&SegmentId(0)).unwrap().tail =
            if convention == CallingConvention::Root {
                TailPolicy::RootWindow
            } else {
                TailPolicy::ContextWindow
            };
        verify(&plan, &fixture.frame(), body(vec![instruction.clone()])).unwrap();
        let Instruction::Exec(cleanup_target) = &instruction else {
            unreachable!()
        };
        for raw in [
            vec![],
            vec![internal_site("wrong_cleanup", 0, 0).instruction],
            vec![Instruction::Call(cleanup_target.clone())],
            vec![instruction.clone(), instruction.clone()],
        ] {
            assert_eq!(
                verify(&plan, &fixture.frame(), body(raw)).unwrap_err().category,
                DiagnosticCategory::AbiMismatch
            );
        }
    }
    context_truncation_is_forbidden_on_exec_return();
}

#[test]
fn commutative_helper_projection_preserves_literal_obligations() {
    let mut fixture = Fixture::new(2, 1);
    fixture.proof.input_symmetry = InputSymmetry::CommutativePair {
        lanes_per_operand: 1,
    };
    fixture.proof.calls.push(CallSite {
        path: vec![],
        instruction: invocation(Invocation::Exec),
        abi: ProcedureAbi::new(CallingConvention::SameContext, 3, 1).unwrap(),
        inputs: vec![
            CallArgument::Exact(StoredValue::Atom(Fixture::input(0))),
            CallArgument::Exact(StoredValue::Atom(Fixture::input(1))),
            CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(7))),
        ],
        truncate: false,
        outputs: vec![],
        publications: vec![(Fixture::output(0), 0)],
        selector: None,
    });
    let mut plan = fixture.plan();
    plan.segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .insert(0, PlanAction::Swap { depth: 1 });
    plan.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap().input_order = InputOrder::Reversed;
    let make = |literal, exchange| {
        let mut code = body(vec![
            Primitive::Push(Felt::new_unchecked(literal)).instruction().unwrap(),
            Instruction::MovDn2,
            if exchange {
                Instruction::Swap1
            } else {
                Instruction::Nop
            },
            invocation(Invocation::Exec),
        ]);
        code.ops_mut().insert(
            1,
            AnnotatedOp::Instruction {
                instruction: Instruction::Swap1,
                unit: SelectedStepId(u32::MAX),
            },
        );
        code
    };
    verify(&plan, &fixture.frame(), make(7, false)).unwrap();
    let mut wrong_target = make(7, false);
    for op in wrong_target.ops_mut() {
        if let AnnotatedOp::Instruction {
            instruction: Instruction::Exec(_),
            unit,
        } = op
        {
            *op = AnnotatedOp::Instruction {
                instruction: invocation(Invocation::Call),
                unit: *unit,
            };
        }
    }
    let mut wrong_publication = make(7, false);
    let end = wrong_publication.ops().len() - 2;
    wrong_publication.ops_mut().splice(end..end, [inst(Instruction::Drop), push(7)]);
    for wrong in [make(8, false), make(7, true), wrong_target, wrong_publication] {
        assert_eq!(
            verify(&plan, &fixture.frame(), wrong).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );
    }
}

#[test]
fn source_pointer_selector_checks_word_read_authority_before_exact_token_equality() {
    use super::super::{
        replay::{ActiveUnit, Cursor, Verifier},
        stack::State,
        tokens::Identity,
    };
    for (extent, identity, tainted, read, expected) in [
        (4, Identity::FrameAddress(0), false, true, None),
        (
            5,
            Identity::FrameAddress(4),
            false,
            true,
            Some(DiagnosticCategory::InvalidSpillSlotRead),
        ),
        (
            4,
            Identity::FrameAddress(4),
            false,
            true,
            Some(DiagnosticCategory::InvalidSpillSlotRead),
        ),
        (
            8,
            Identity::FrameAddress(1),
            false,
            true,
            Some(DiagnosticCategory::IllegalStackAccess),
        ),
        (
            4,
            Identity::FrameAddress(0),
            true,
            true,
            Some(DiagnosticCategory::InvalidSpillSlotRead),
        ),
        (
            4,
            Identity::Input(0),
            true,
            true,
            Some(DiagnosticCategory::InvalidSpillSlotRead),
        ),
        (4, Identity::Input(0), false, true, None),
        (4, Identity::Literal(400), false, true, None),
        (4, Identity::Literal((1u64 << 32) - 4), false, true, None),
        (
            4,
            Identity::Literal(401),
            false,
            true,
            Some(DiagnosticCategory::IllegalStackAccess),
        ),
        (
            4,
            Identity::Literal(1u64 << 32),
            false,
            true,
            Some(DiagnosticCategory::IllegalStackAccess),
        ),
        (
            4,
            Identity::FrameAddress(0),
            false,
            false,
            Some(DiagnosticCategory::InvalidSelectedLowering),
        ),
    ] {
        let mut fixture = Fixture::new(1, 0).scratch(4);
        fixture.locals = SourceLocalLayout {
            offsets: vec![0],
            extent,
        };
        fixture.effects.may_trap = true;
        if read {
            fixture.effects.memory = SourceMemoryEffects::READ;
        }
        // Even an explicit compiler borrow cannot authorize the source-memory call form.
        fixture.scratch_effects.push(ScratchEffect::Borrow {
            object: ScratchId(0),
            offset: 0,
            size: 4,
            read: true,
            write: false,
            definite_write: false,
            retained: false,
        });
        fixture.proof.calls.push(CallSite {
            instruction: Instruction::DynExec,
            abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
            inputs: vec![],
            path: vec![],
            truncate: false,
            outputs: vec![],
            publications: vec![],
            selector: Some(SelectorEvidence::SourcePointer {
                address: Fixture::input(0),
            }),
        });
        let plan = fixture.plan();
        let frame = fixture.frame();
        let mut verifier = Verifier::new(&plan, &frame).unwrap();
        let pointer = verifier.tokens.intern(identity.clone());
        if tainted {
            let compiler = verifier.tokens.intern(Identity::FrameAddress(extent));
            verifier.tokens.add_origins(pointer, verifier.tokens.origins(compiler));
        }
        let mut state = State::new(vec![pointer], BTreeMap::from([(Fixture::input(0), pointer)]));
        // Borrowed cells initialized independently; source-pointer rejection cannot rely on staleness.
        let value = verifier.tokens.fresh();
        for cell in extent..extent + 4 {
            state.cells.insert(cell, value);
        }
        let mut cursor = Cursor {
            active: Some(ActiveUnit {
                id: SelectedStepId(0),
                incoming: state.clone(),
                readonly: BTreeMap::new(),
                call: 0,
                path: vec![],
            }),
            ..Default::default()
        };
        let result = verifier.instruction(&Instruction::DynExec, &mut state, &mut cursor);
        match expected {
            None => {
                result.unwrap();
            }
            Some(category) => assert_eq!(
                result.unwrap_err().category,
                category,
                "{identity:?}, tainted={tainted}, read={read}"
            ),
        }
    }
}
