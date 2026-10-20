use super::*;

pub(super) fn fixture(name: &str, inputs: usize, outputs: usize) -> Parsed {
    let args = (0..inputs).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(",");
    let operands = (0..inputs).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(",");
    let results = (0..outputs).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
    let types = vec!["felt"; outputs].join(",");
    parse(&format!(
        "builtin.function public extern(\"C\") @fixed({args}) -> ({types}) {{ \
         {results}=hir.{name} {operands}; builtin.ret {results} : ({types}); }};"
    ))
}

#[test]
fn fixed_primitives_select_exact_source() {
    for (f, _) in fixtures() {
        let function = f.borrow();
        let env = SelectEnv::default();
        let selected = select_function(&function, &env).unwrap();
        check_selection(
            selected.clone(),
            &SourceContext::Hir {
                function: &function,
                env: &env,
            },
        )
        .unwrap();
        for mutation in 0..4 {
            let mut bad = selected.clone();
            match mutation {
                0 => {
                    let source = *bad.source_map.keys().next().unwrap();
                    bad.source_map.remove(&source);
                }
                1 => {
                    let unit = bad.segments[0].units[0].clone();
                    bad.segments[0].units.push(unit);
                }
                2 => {
                    bad.segments[0].units[0].outputs.pop();
                }
                _ => {
                    let atom = bad.segments[0].units[0].outputs[0];
                    bad.segments[0].units[0].outputs.push(atom);
                }
            }
            assert!(
                check_selection(
                    bad,
                    &SourceContext::Hir {
                        function: &function,
                        env: &env
                    }
                )
                .is_err(),
                "mutation {mutation}"
            );
        }
    }
}

#[test]
fn caller_materializes_word_without_overwriting_suffix() {
    use alloc::collections::BTreeSet;

    use midenc_hir::{SourceSpan, Type};
    let mut block = vec![];
    let mut stack = crate::OperandStack::new(Rc::new(Context::default()));
    stack.push(Type::Felt);
    let mut invoked = BTreeSet::new();
    crate::emit::OpEmitter::new(&mut invoked, &mut block, &mut stack).caller(SourceSpan::UNKNOWN);
    let instructions = block
        .iter()
        .map(|i| match i {
            crate::masm::Op::Inst(i) => i.to_string(),
            _ => panic!(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    execute_code(&format!(
        "begin push.91 {instructions} repeat.4 assertz end push.91 assert_eq end"
    ))
    .unwrap();
}

pub(super) fn word_fixture(caller: bool) -> Parsed {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{ArrayType, SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    let args = if caller {
        vec![]
    } else {
        vec![Type::from(ArrayType::new(Type::Felt, 4))]
    };
    let mut test = midenc_hir::testing::Test::new("word", &args, &vec![Type::Felt; 4]);
    let argument = test.function().borrow().entry_block().borrow().arguments().first().copied();
    {
        let mut b = test.function_builder();
        let word = if caller {
            b.caller(SourceSpan::UNKNOWN).unwrap()
        } else {
            argument.unwrap() as midenc_hir::ValueRef
        };
        let lanes = b.unpack_word(word, SourceSpan::UNKNOWN).unwrap();
        b.ret(lanes, SourceSpan::UNKNOWN).unwrap();
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

pub(super) fn fixtures() -> Vec<(Parsed, bool)> {
    let mut all = [
        ("hash", 4, 4),
        ("h_merge", 8, 4),
        ("h_perm", 12, 12),
        ("advice_pop", 0, 1),
        ("advice_load_word", 4, 4),
        ("clk", 0, 1),
    ]
    .into_iter()
    .map(|(n, i, o)| (fixture(n, i, o), false))
    .collect::<Vec<_>>();
    all.push((word_fixture(true), false));
    all.push((word_fixture(false), false));
    all
}

#[test]
fn poseidon_word_merge_and_permutation_match_vm() {
    use miden_core::{Felt, crypto::hash::Poseidon2};
    for zero in [true, false] {
        for (name, n, m) in [("hash", 4, 4), ("h_merge", 8, 4), ("h_perm", 12, 12)] {
            let input = (0..n).map(|i| if zero { 0 } else { 101 + i * 37 }).collect::<Vec<_>>();
            let felts = input.iter().map(|n| Felt::new_unchecked(*n as u64)).collect::<Vec<_>>();
            let output = if n == 12 {
                let mut state: [Felt; 12] = felts.try_into().unwrap();
                Poseidon2::apply_permutation(&mut state);
                state.to_vec()
            } else {
                Poseidon2::hash_elements(&felts).as_elements().to_vec()
            };
            let expected = output.iter().map(|f| f.as_canonical_u64() as usize).collect::<Vec<_>>();
            let f = fixture(name, n, m);
            execute_linked(&f, &input, &expected);
            // Semantic mutations remain physically legal and require VM assertions.
            if !zero {
                assert!(
                    execute_linked_memory_mutate(&f, &input, &expected, "", "", |body| {
                        let index = body
                            .ops()
                            .iter()
                            .position(|op| {
                                matches!(
                                    op,
                                    AnnotatedOp::Instruction {
                                        instruction: crate::masm::Instruction::Hash
                                            | crate::masm::Instruction::HMerge
                                            | crate::masm::Instruction::HPerm,
                                        ..
                                    }
                                )
                            })
                            .unwrap();
                        let unit = match body.ops()[index] {
                            AnnotatedOp::Instruction { unit, .. } => unit,
                            _ => unreachable!(),
                        };
                        body.ops_mut().insert(
                            index,
                            AnnotatedOp::Instruction {
                                instruction: crate::masm::Instruction::Swap1,
                                unit,
                            },
                        );
                    })
                    .is_err()
                );
            }
        }
    }
}

#[test]
fn unpack_word_is_exact_lane_alias() {
    let f = word_fixture(false);
    let selected = checked(&f, &SelectEnv::default());
    let unit = &selected.segments[0].units[0];
    assert_eq!(unit.stack_inputs.len(), 4);
    assert_eq!(unit.outputs.len(), 4);
    for (output, input) in unit.outputs.iter().zip(&unit.stack_inputs) {
        assert_eq!(
            selected.values[&output.value].reconstruction,
            Some(Reconstruction::Alias(vec![*input]))
        );
    }
    let (_, _, body) = ready(&f, &SelectEnv::default());
    assert!(!body.ops().iter().any(|o| matches!(o, AnnotatedOp::Instruction { .. })));
    execute_linked(&f, &[13, 29, 47, 71], &[13, 29, 47, 71]);
    let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
    let i = body
        .ops()
        .iter()
        .position(|o| matches!(o, AnnotatedOp::Boundary(Boundary::UnitEnd(_))))
        .unwrap();
    let unit = match body.ops()[i] {
        AnnotatedOp::Boundary(Boundary::UnitEnd(unit)) => unit,
        _ => unreachable!(),
    };
    body.ops_mut().insert(
        i,
        AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::Swap1,
            unit,
        },
    );
    assert!(verify::verify(&plan, &frame, body).is_err());
    let function = f.borrow();
    let env = SelectEnv::default();
    let mut bad = select_function(&function, &env).unwrap();
    let output = bad.segments[0].units[0].outputs[0];
    bad.values.get_mut(&output.value).unwrap().reconstruction =
        Some(Reconstruction::Alias(vec![bad.segments[0].units[0].stack_inputs[1]]));
    assert!(
        check_selection(
            bad,
            &SourceContext::Hir {
                function: &function,
                env: &env
            }
        )
        .is_err()
    );
}

#[test]
fn malformed_fixed_primitive_source_tuples_reject_before_construction() {
    use midenc_hir::{SourceSpan, Type, Value};
    for mutation in 0..6 {
        for (f, _) in fixtures() {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            match mutation {
                0 => {
                    let mut r = op.borrow().results().all()[0];
                    r.borrow_mut().set_type(Type::U32);
                }
                1 => {
                    let last = op.borrow().results().groups().count() - 1;
                    op.borrow_mut().results_mut().group_mut(last).pop().unwrap();
                }
                2 => {
                    let n = op.borrow().results().len();
                    let r = f.context.make_result(SourceSpan::UNKNOWN, Type::Felt, op, n as u16);
                    op.borrow_mut().results_mut().push(r);
                }
                3 if !op.borrow().operands().is_empty() => {
                    let mut a = op.borrow().operands().all()[0].borrow().as_value_ref();
                    a.borrow_mut().set_type(Type::U64);
                }
                4 if !op.borrow().operands().is_empty() => {
                    op.borrow_mut().operands_mut().group_mut(0).erase(0);
                }
                5 => {
                    let value = op.borrow().results().all()[0] as midenc_hir::ValueRef;
                    op.borrow_mut().operands_mut().push(f.context.make_operand(value, op, 0));
                }
                _ => continue,
            }
            assert!(
                select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                "mutation {mutation}"
            );
        }
    }
}

#[test]
fn fixed_word_requires_exact_four_felt_array() {
    use midenc_hir::{ArrayType, Type, Value};
    for ty in [
        Type::from(ArrayType::new(Type::Felt, 3)),
        Type::from(ArrayType::new(Type::Felt, 5)),
        Type::from(ArrayType::new(Type::U32, 4)),
    ] {
        for caller in [false, true] {
            let f = word_fixture(caller);
            let op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            if caller {
                let mut result = op.borrow().results().all()[0];
                result.borrow_mut().set_type(ty.clone());
            } else {
                let mut argument = op.borrow().operands().all()[0].borrow().as_value_ref();
                argument.borrow_mut().set_type(ty.clone());
            }
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
    }
}

#[test]
fn mutated_primitive_publication_and_target_effects_reject() {
    use crate::masm::Instruction;
    let f = word_fixture(true);
    execute_linked(&f, &[], &[0, 0, 0, 0]);
    for mutation in 0..7 {
        let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
        let index = body
            .ops()
            .iter()
            .position(|o| {
                matches!(
                    o,
                    AnnotatedOp::Instruction {
                        instruction: Instruction::Caller,
                        ..
                    }
                )
            })
            .unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        match mutation {
            0 => {
                let pad = body
                    .ops()
                    .iter()
                    .position(|o| {
                        matches!(
                            o,
                            AnnotatedOp::Instruction {
                                instruction: Instruction::PadW,
                                ..
                            }
                        )
                    })
                    .unwrap();
                body.ops_mut().remove(pad);
            }
            1 => body.ops_mut().insert(
                index,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Dup15,
                    unit,
                },
            ),
            2 => body.ops_mut().insert(
                index + 1,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Drop,
                    unit,
                },
            ),
            3 => body.ops_mut().insert(
                index + 1,
                AnnotatedOp::Instruction {
                    instruction: Instruction::PadW,
                    unit,
                },
            ),
            4 => body.ops_mut().insert(index, AnnotatedOp::Boundary(Boundary::UnitEnd(unit))),
            5 => {
                body.ops_mut()[index] = AnnotatedOp::Instruction {
                    instruction: Instruction::AdvPush,
                    unit,
                }
            }
            _ => body.ops_mut().insert(
                index + 1,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Swap4,
                    unit,
                },
            ),
        }
        assert!(verify::verify(&plan, &frame, body).is_err(), "mutation {mutation}");
    }
}

#[test]
fn caller_inside_syscall_returns_actual_initiating_procedure_hash() {
    let f = word_fixture(true);
    let suffix = (0..20).map(|i| 701 + i).collect::<Vec<_>>();
    let probe = format!(
        "mem_load.120 push.4242 assert_eq mem_storew_le.100 dropw {} exec.selected padw \
         mem_loadw_le.100 assert_eqw {} push.9191 mem_store.120",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::assert_top(&suffix)
    );
    execute_kernel_probe(
        &f,
        &probe,
        "proc origin syscall.probe repeat.16 drop end end begin push.4242 mem_store.120 \
         exec.origin mem_load.120 push.9191 assert_eq end",
    )
    .unwrap();
}

#[test]
fn advice_pop_preserves_order_suffix_and_traps() {
    let f = parse(
        "builtin.function public extern(\"C\") @advice() -> (felt,felt) { %a=hir.advice_pop; \
         %b=hir.advice_pop; builtin.ret %a,%b : (felt,felt); };",
    );
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
    let mut expected = vec![17, 29];
    expected.extend(&suffix);
    let code = format!(
        "{procedure} begin {} exec.selected {} end",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::assert_top(&expected)
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("advice_order", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    execute_program(&program, &mut crate::plan::tests::lowering_units::host(), &[17, 29]).unwrap();
    for advice in [vec![], vec![17]] {
        let (processor, error) = execute_failure_state(&program, &advice);
        assert!(error.to_string().contains("advice"), "{error:?}");
        let actual = processor
            .stack()
            .iter()
            .rev()
            .map(|f| f.as_canonical_u64() as usize)
            .collect::<Vec<_>>();
        // Failure occurs at the exhausted AdvPop, before normal unit completion/publication.
        let prefix = usize::from(!advice.is_empty());
        assert_eq!(&actual[prefix..prefix + suffix.len()], suffix);
        if prefix == 1 {
            assert_eq!(actual[0], 17);
        }
    }
    let selected = checked(&f, &SelectEnv::default());
    assert!(selected.segments[0].units.iter().all(|u| u.effects.may_trap));
}

#[test]
fn clock_result_is_actual_vm_clock() {
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicU64, Ordering};

    use miden_processor::event::EventName;
    let f = fixture("clk", 0, 1);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    // Test-only observation immediately after the selected clk. Trace expands to two pushes,
    // emit, two drops: the observed value is beneath the two event lanes at clock c+3.
    assert_eq!(procedure.matches("clk").count(), 1);
    let observed = Arc::new(AtomicU64::new(0));
    let record = observed.clone();
    let mut host = crate::plan::tests::lowering_units::host();
    host.register_trace_handler(
        EventName::new("test::fixed_clock"),
        Arc::new(move |state: &miden_processor::ProcessorState<'_>| {
            let clock = state.clock().as_u32() as u64;
            assert_eq!(state.get_stack_item(2).as_canonical_u64(), clock - 3);
            record.store(clock, Ordering::Relaxed);
            Ok(())
        }),
    )
    .unwrap();
    let observed_procedure = procedure.replace("clk", "clk trace.event(\"test::fixed_clock\")");
    let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
    let program = miden_assembly::Assembler::default()
        .assemble_program(
            "clock",
            format!(
                "{observed_procedure} begin {} exec.selected drop {} end",
                crate::plan::tests::lowering_units::push_top(&suffix),
                crate::plan::tests::lowering_units::assert_top(&suffix)
            ),
        )
        .unwrap()
        .try_into_program()
        .unwrap();
    execute_program(&program, &mut host, &[]).unwrap();
    assert!(observed.load(Ordering::Relaxed) > 0);
}

#[test]
fn advice_word_selects_exact_source() {
    let f = fixture("advice_load_word", 4, 4);
    let selected = checked(&f, &SelectEnv::default());
    assert!(selected.segments[0].units[0].effects.may_trap);
    let _ = ready(&f, &SelectEnv::default());
}

fn advice_word_typed_fixture(types: &[midenc_hir::Type]) -> Parsed {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    let mut test = midenc_hir::testing::Test::new("advice_word", types, &vec![Type::Felt; 4]);
    let args = test
        .function()
        .borrow()
        .entry_block()
        .borrow()
        .arguments()
        .iter()
        .map(|a| *a as midenc_hir::ValueRef)
        .collect::<Vec<_>>();
    {
        let mut builder = test.function_builder();
        let (a, b, c, d) = builder
            .advice_load_word(args[0], args[1], args[2], args[3], SourceSpan::UNKNOWN)
            .unwrap();
        builder.ret([a, b, c, d], SourceSpan::UNKNOWN).unwrap();
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn advice_word_independent_source_contract() {
    use midenc_hir::{AddressSpace, ArrayType, PointerType, Type, Value};

    use crate::lower::units::intrinsics::{Intrinsic, Kind};
    for types in [
        vec![Type::Felt; 4],
        vec![
            Type::U8,
            Type::from(PointerType::new(Type::U128)),
            Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element)),
            Type::from(ArrayType::new(Type::U8, 4)),
        ],
        vec![Type::I1, Type::I16, Type::I32, Type::U32],
    ] {
        let f = advice_word_typed_fixture(&types);
        let _ = ready(&f, &SelectEnv::default());
    }
    for ty in [
        Type::U64,
        Type::I64,
        Type::U128,
        Type::from(ArrayType::new(Type::Felt, 4)),
        Type::Never,
        Type::from(ArrayType::new(Type::U8, 0)),
        Type::Unknown,
    ] {
        for index in 0..4 {
            let f = fixture("advice_load_word", 4, 4);
            let op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            let mut arg = op.borrow().operands().all()[index].borrow().as_value_ref();
            arg.borrow_mut().set_type(ty.clone());
            assert!(
                Intrinsic::from_source(Kind::AdviceLoadWord, &op.borrow()).is_err(),
                "{ty}, operand {index}"
            );
            assert_eq!(arg.borrow().ty(), &ty);
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
    }
    for mutation in 0..5 {
        let f = fixture("advice_load_word", 4, 4);
        let mut op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        match mutation {
            0 => {
                let mut result = op.borrow().results().all()[0];
                result.borrow_mut().set_type(Type::U32);
            }
            1 => {
                op.borrow_mut().operands_mut().group_mut(0).erase(0);
            }
            2 => {
                let value = op.borrow().operands().all()[0].borrow().as_value_ref();
                op.borrow_mut().operands_mut().push(f.context.make_operand(value, op, 4));
            }
            3 => {
                op.borrow_mut().results_mut().group_mut(0).pop().unwrap();
            }
            _ => {
                let result =
                    f.context.make_result(midenc_hir::SourceSpan::UNKNOWN, Type::Felt, op, 4);
                op.borrow_mut().results_mut().push(result);
            }
        }
        let before = op
            .borrow()
            .results()
            .all()
            .iter()
            .map(|v| v.borrow().ty().clone())
            .collect::<Vec<_>>();
        assert!(Intrinsic::from_source(Kind::AdviceLoadWord, &op.borrow()).is_err());
        assert_eq!(
            op.borrow()
                .results()
                .all()
                .iter()
                .map(|v| v.borrow().ty().clone())
                .collect::<Vec<_>>(),
            before
        );
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

#[test]
fn advice_word_candidate_publications_and_physical_mutations_reject() {
    let f = fixture("advice_load_word", 4, 4);
    let function = f.borrow();
    let env = SelectEnv::default();
    for mutation in 0..7 {
        let mut selected = select_function(&function, &env).unwrap();
        let unit = &mut selected.segments[0].units[0];
        match mutation {
            0 => {
                unit.outputs.pop();
            }
            1 => unit.outputs.push(unit.outputs[0]),
            2 => unit.outputs.swap(0, 1),
            3 => unit.outputs[0] = unit.stack_inputs[0],
            4 => unit.effects.may_trap = false,
            5 => {
                unit.semantic_inputs.pop();
            }
            _ => unit.stack_inputs.swap(0, 1),
        }
        assert!(
            check_selection(
                selected,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
    for instruction in [crate::masm::Instruction::AdvPush, crate::masm::Instruction::HPerm] {
        let (plan, frame, mut body) = ready(&f, &env);
        let index = body
            .ops()
            .iter()
            .position(|o| {
                matches!(
                    o,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::AdvLoadW,
                        ..
                    }
                )
            })
            .unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        body.ops_mut()[index] = AnnotatedOp::Instruction { instruction, unit };
        assert!(verify::verify(&plan, &frame, body).is_err());
    }
}

#[test]
fn advice_word_vm_order_consumption_suffix_floor_and_exhaustion() {
    use miden_core::{Felt, advice::AdviceStack};
    use miden_processor::{FastProcessor, StackInputs, advice::AdviceInputs};
    let f = fixture("advice_load_word", 4, 4);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    for suffix_len in [12, 20] {
        let suffix = (0..suffix_len).map(|i| 0x1_0000_0000usize + i).collect::<Vec<_>>();
        let mut input = vec![101, 202, 303, 404];
        input.extend(&suffix);
        let mut expected = vec![3, 4, 5, 6];
        expected.extend(&suffix);
        let code = format!(
            "{procedure} begin {} sdepth push.{} assert_eq exec.selected sdepth push.{} assert_eq \
             adv_push push.97 assert_eq {} end",
            crate::plan::tests::lowering_units::push_top(&input[..input.len() - 16]),
            input.len(),
            input.len(),
            crate::plan::tests::lowering_units::assert_top(&expected)
        );
        let program = miden_assembly::Assembler::default()
            .assemble_program("advice_word", &code)
            .unwrap()
            .try_into_program()
            .unwrap();
        let mut advice = AdviceStack::new();
        advice
            .append_word([3, 4, 5, 6].map(Felt::new_unchecked).into())
            .append_element(Felt::new_unchecked(97));
        let base = input[input.len() - 16..]
            .iter()
            .map(|n| Felt::new_unchecked(*n as u64))
            .collect::<Vec<_>>();
        let processor = |advice: AdviceStack| {
            FastProcessor::new(StackInputs::new(&base).unwrap())
                .with_advice(AdviceInputs::default().with_stack(advice))
                .unwrap()
        };
        processor(advice)
            .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
            .unwrap();
        for count in 0..4 {
            let mut processor = processor(AdviceStack::from(vec![Felt::ONE; count]));
            let mut host = crate::plan::tests::lowering_units::host();
            let mut context = processor.get_initial_resume_context(&program).unwrap();
            let error = loop {
                match processor.step_sync(&mut host, context) {
                    Ok(Some(next)) => context = next,
                    Ok(None) => panic!("partial word must trap"),
                    Err(error) => break error,
                }
            };
            assert!(
                matches!(
                    error,
                    miden_processor::ExecutionError::AdviceError {
                        err: miden_processor::advice::AdviceError::StackReadFailed,
                        ..
                    }
                ),
                "{error:?}"
            );
            let actual = processor
                .stack()
                .iter()
                .rev()
                .map(|f| f.as_canonical_u64() as usize)
                .collect::<Vec<_>>();
            assert_eq!(actual, input, "exhaustion with {count} elements");
        }
    }
}

/// Physical replay does not establish the algorithm: independently observe both result lanes
/// and the next advice element after the selected body and each semantic mutation.
#[test]
fn advice_word_semantic_mutations_violate_vm_results_and_consumption() {
    use miden_core::{Felt, advice::AdviceStack, crypto::hash::Poseidon2};
    use miden_processor::{FastProcessor, StackInputs, advice::AdviceInputs};
    let f = fixture("advice_load_word", 4, 4);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert_eq!(procedure.matches("adv_loadw").count(), 1);
    let inputs = [101, 202, 303, 404].map(Felt::new_unchecked);
    let hash = Poseidon2::hash_elements(&inputs).as_elements().to_vec();
    let word = [3, 4, 5, 6].map(Felt::new_unchecked).to_vec();
    for (replacement, expected_next, expected_results, wrong_results, wrong_consumption) in [
        ("adv_loadw", 97, word.clone(), false, false),
        ("nop", 3, inputs.to_vec(), true, true),
        ("hash", 3, hash, true, true),
        (
            "adv_loadw swap.1",
            97,
            [4, 3, 5, 6].map(Felt::new_unchecked).to_vec(),
            true,
            false,
        ),
    ] {
        let mutated = procedure.replace("adv_loadw", replacement);
        // Observe a scalar advice read after the body. Remove one padding lane so the program
        // returns at the VM floor while retaining the observed next element and four results.
        let program = miden_assembly::Assembler::default()
            .assemble_program(
                "advice_word_semantics",
                format!("{mutated} begin exec.selected adv_push movup.15 drop end"),
            )
            .unwrap()
            .try_into_program()
            .unwrap();
        let mut advice = AdviceStack::new();
        advice
            .append_word([3, 4, 5, 6].map(Felt::new_unchecked).into())
            .append_element(Felt::new_unchecked(97));
        let output = FastProcessor::new(StackInputs::new(&inputs).unwrap())
            .with_advice(AdviceInputs::default().with_stack(advice))
            .unwrap()
            .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
            .unwrap();
        let next = output.stack.get_element(0).unwrap();
        let results = (1..5).map(|i| output.stack.get_element(i).unwrap()).collect::<Vec<_>>();
        assert_eq!(next, Felt::new_unchecked(expected_next), "{replacement}");
        assert_eq!(results, expected_results, "{replacement}");
        assert_eq!(results != word, wrong_results, "{replacement}");
        assert_eq!(next != Felt::new_unchecked(97), wrong_consumption, "{replacement}");
    }
}
