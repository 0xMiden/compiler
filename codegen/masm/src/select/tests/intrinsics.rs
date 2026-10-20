use super::*;

pub(super) fn fixture(name: &str, inputs: usize, outputs: usize) -> Parsed {
    let args = (0..inputs).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(",");
    let operands = (0..inputs).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(",");
    let results = (0..outputs).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
    let types = vec!["felt"; outputs].join(",");
    let properties = if name == "m_tree_verify" {
        " <{ message = \"\" }>"
    } else {
        ""
    };
    parse(&format!(
        "builtin.function public extern(\"C\") @fixed({args}) -> ({types}) {{ \
         {results}=hir.{name} {operands}{properties}; builtin.ret {results} : ({types}); }};"
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
        ("m_tree_get", 6, 8),
        ("m_tree_set", 10, 8),
        ("m_tree_merge", 8, 4),
        ("m_tree_verify", 10, 10),
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

#[test]
fn merkle_source_selection_and_aliases() {
    for (name, inputs, outputs, aliases) in [
        ("m_tree_get", 6, 8, vec![(4, 2), (5, 3), (6, 4), (7, 5)]),
        ("m_tree_set", 10, 8, vec![]),
        ("m_tree_merge", 8, 4, vec![]),
        ("m_tree_verify", 10, 10, (0..10).map(|i| (i, i)).collect()),
    ] {
        let f = fixture(name, inputs, outputs);
        let selected = checked(&f, &SelectEnv::default());
        let unit = &selected.segments[0].units[0];
        assert!(unit.effects.may_trap, "{name}");
        let _ = ready(&f, &SelectEnv::default());
        for (output, input) in aliases {
            assert_eq!(
                crate::select::evidence(&f.borrow(), &SelectEnv::default())
                    .unwrap()
                    .source
                    .units[&unit.id]
                    .aliases
                    .pairs()
                    .iter()
                    .find(|(o, _)| *o == unit.outputs[output])
                    .map(|(_, i)| *i),
                Some(unit.stack_inputs[input])
            );
        }
    }
}

fn merkle_word(seed: u64) -> miden_core::Word {
    [seed, seed + 7, seed + 19, seed + 41]
        .map(miden_core::Felt::new_unchecked)
        .into()
}

fn merkle_lanes(word: miden_core::Word) -> Vec<usize> {
    word.as_elements().iter().map(|v| v.as_canonical_u64() as usize).collect()
}

/// Step the real processor so both successful and failed provider state remain observable.
fn merkle_vm(
    code: &str,
    input: &[usize],
    store: miden_core::crypto::merkle::MerkleStore,
    budget: Option<usize>,
) -> (miden_processor::FastProcessor, Result<(), miden_processor::ExecutionError>) {
    use miden_core::{Felt, advice::AdviceStack};
    use miden_processor::{ExecutionOptions, FastProcessor, StackInputs, advice::AdviceInputs};
    let program = miden_assembly::Assembler::default()
        .assemble_program("merkle_semantics", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let base = input[input.len().saturating_sub(16)..]
        .iter()
        .map(|&n| Felt::new_unchecked(n as u64))
        .collect::<Vec<_>>();
    let advice = AdviceInputs::default()
        .with_merkle_store(store)
        .with_stack(AdviceStack::from(vec![Felt::new_unchecked(97)]));
    let options = budget.map_or_else(ExecutionOptions::default, |n| {
        ExecutionOptions::default().with_max_advice_size_bytes(n)
    });
    let mut processor =
        FastProcessor::new_with_options(StackInputs::new(&base).unwrap(), advice, options).unwrap();
    let mut host = crate::plan::tests::lowering_units::host();
    let mut context = processor.get_initial_resume_context(&program).unwrap();
    let result = loop {
        match processor.step_sync(&mut host, context) {
            Ok(Some(next)) => context = next,
            Ok(None) => break Ok(()),
            Err(err) => break Err(err),
        }
    };
    (processor, result)
}

fn merkle_probe(procedure: &str, input: &[usize], expected: &[usize]) -> String {
    format!(
        "{procedure} begin {} exec.selected adv_push push.97 assert_eq {} end",
        crate::plan::tests::lowering_units::push_top(&input[..input.len().saturating_sub(16)]),
        crate::plan::tests::lowering_units::assert_top(expected)
    )
}

#[test]
fn merkle_selected_get_set_merge_verify_vm_provider_and_suffix() {
    use miden_core::{
        Felt,
        crypto::{
            hash::Poseidon2,
            merkle::{MerkleStore, MerkleTree, NodeIndex},
        },
    };
    let leaves = (0..8).map(|i| merkle_word(0x1_0000_0000 + i * 101)).collect::<Vec<_>>();
    let tree = MerkleTree::new(leaves.clone()).unwrap();
    let other = MerkleTree::new([merkle_word(5001), merkle_word(6001)]).unwrap();
    let mut store = MerkleStore::from(&tree);
    store.extend(other.inner_nodes());
    for suffix_len in [16, 20] {
        let suffix = (0..suffix_len).map(|i| 0x2_0000_0000 + i).collect::<Vec<_>>();
        for (depth, index) in [(1, 0), (1, 1), (3, 0), (3, 7)] {
            let node = store.get_node(tree.root(), NodeIndex::new(depth, index).unwrap()).unwrap();
            let mut input = vec![depth as usize, index as usize];
            input.extend(merkle_lanes(tree.root()));
            input.extend(&suffix);
            let mut expected = merkle_lanes(node);
            expected.extend(merkle_lanes(tree.root()));
            expected.extend(&suffix);
            for (name, n, m) in [("m_tree_get", 6, 8), ("m_tree_verify", 10, 10)] {
                let procedure = selected_procedure(&fixture(name, n, m), &SelectEnv::default());
                let actual_input = if n == 10 {
                    let mut v = merkle_lanes(node);
                    v.extend(&input);
                    v
                } else {
                    input.clone()
                };
                let actual_expected = if n == 10 {
                    actual_input.clone()
                } else {
                    expected.clone()
                };
                let (processor, result) = merkle_vm(
                    &merkle_probe(&procedure, &actual_input, &actual_expected),
                    &actual_input,
                    store.clone(),
                    None,
                );
                result.unwrap();
                let (advice, _) = processor.into_parts();
                assert!(advice.stack().is_empty());
                assert_eq!(
                    advice
                        .get_tree_node(
                            tree.root(),
                            Felt::new_unchecked(depth as u64),
                            Felt::new_unchecked(index)
                        )
                        .unwrap(),
                    node
                );
            }
        }
        for value in [merkle_word(9001), leaves[7], tree.root()] {
            let mut reference = tree.clone();
            reference.update_leaf(7, value).unwrap();
            let root = reference.root();
            let mut input = vec![3, 7];
            input.extend(merkle_lanes(tree.root()));
            input.extend(merkle_lanes(value));
            input.extend(&suffix);
            let mut expected = merkle_lanes(leaves[7]);
            expected.extend(merkle_lanes(root));
            expected.extend(&suffix);
            let procedure =
                selected_procedure(&fixture("m_tree_set", 10, 8), &SelectEnv::default());
            let mut query = vec![3, 7];
            query.extend(merkle_lanes(root));
            let mut query_result = merkle_lanes(value);
            query_result.extend(merkle_lanes(root));
            let mut verify = merkle_lanes(value);
            verify.extend(&query);
            let code = merkle_probe(&procedure, &input, &expected);
            let code = format!(
                "{} {} mtree_get {} {} mtree_verify repeat.10 drop end end",
                code.strip_suffix("end").unwrap(),
                crate::plan::tests::lowering_units::push_top(&query),
                crate::plan::tests::lowering_units::assert_top(&query_result),
                crate::plan::tests::lowering_units::push_top(&verify)
            );
            let (processor, result) = merkle_vm(&code, &input, store.clone(), None);
            result.unwrap();
            let (advice, _) = processor.into_parts();
            assert_eq!(
                advice
                    .get_tree_node(tree.root(), Felt::new_unchecked(3), Felt::new_unchecked(7))
                    .unwrap(),
                leaves[7]
            );
            assert_eq!(
                advice
                    .get_tree_node(root, Felt::new_unchecked(3), Felt::new_unchecked(7))
                    .unwrap(),
                value
            );
            if value == tree.root() {
                // Stored subtree nodes remain addressable beneath the updated leaf.
                assert_eq!(
                    advice
                        .get_tree_node(root, Felt::new_unchecked(6), Felt::new_unchecked(56))
                        .unwrap(),
                    leaves[0]
                );
            }
        }
        for (left, right) in [
            (tree.root(), other.root()),
            (tree.root(), merkle_word(6001)),
            (merkle_word(7001), merkle_word(8001)),
            (merkle_word(8001), merkle_word(7001)),
        ] {
            let root = Poseidon2::merge(&[left, right]);
            let mut input = merkle_lanes(left);
            input.extend(merkle_lanes(right));
            input.extend(&suffix);
            let mut expected = merkle_lanes(root);
            expected.extend(&suffix);
            let procedure =
                selected_procedure(&fixture("m_tree_merge", 8, 4), &SelectEnv::default());
            let (processor, result) = merkle_vm(
                &merkle_probe(&procedure, &input, &expected),
                &input,
                store.clone(),
                None,
            );
            result.unwrap();
            let (advice, _) = processor.into_parts();
            for (index, child) in [(0, left), (1, right)] {
                assert_eq!(
                    advice.get_tree_node(root, Felt::ONE, Felt::new_unchecked(index)).unwrap(),
                    child
                );
            }
            assert_eq!(
                advice.get_tree_node(tree.root(), Felt::new_unchecked(3), Felt::ZERO).unwrap(),
                leaves[0]
            );
            assert_eq!(
                advice.get_tree_node(other.root(), Felt::ONE, Felt::ZERO).unwrap(),
                merkle_word(5001)
            );
        }
    }
}

#[test]
fn merkle_vm_depth_index_missing_nodes_and_verify_semantic_mutations() {
    use miden_core::crypto::merkle::{MerkleStore, MerkleTree, SimpleSmt};
    let tree = MerkleTree::new([merkle_word(100), merkle_word(200)]).unwrap();
    let store = MerkleStore::from(&tree);
    // Both raw and final selected bodies use the same provider errors at invalid boundaries.
    for (name, n, m, native) in [
        ("m_tree_get", 6, 8, "mtree_get"),
        ("m_tree_set", 10, 8, "mtree_set"),
        ("m_tree_verify", 10, 10, "mtree_verify"),
    ] {
        let procedure = selected_procedure(&fixture(name, n, m), &SelectEnv::default());
        for (depth, index, root) in [
            (0, 0, tree.root()),
            (65, 0, tree.root()),
            (0x1_0000_0000, 0, tree.root()),
            (1, 2, tree.root()),
            (1, 0, merkle_word(999)),
            (2, 0, tree.root()),
        ] {
            let mut input = if n == 10 && m == 10 {
                merkle_lanes(merkle_word(100))
            } else {
                vec![]
            };
            input.extend([depth, index]);
            input.extend(merkle_lanes(root));
            if n == 10 && m == 8 {
                input.extend(merkle_lanes(merkle_word(999)));
            }
            input.extend(1000..1020);
            let mut errors = vec![];
            for body in [procedure.clone(), format!("proc selected {native} end")] {
                let code = format!(
                    "{body} begin {} exec.selected end",
                    crate::plan::tests::lowering_units::push_top(&input[..input.len() - 16])
                );
                let (_, result) = merkle_vm(&code, &input, store.clone(), None);
                errors.push(format!("{:?}", result.unwrap_err()));
            }
            assert_eq!(errors[0], errors[1], "{name}: {depth}/{index}");
        }
    }
    // Full depth-64 empty sparse tree: root lookup and path verification are real provider work.
    let deep = SimpleSmt::<64>::new().unwrap();
    let store64 = MerkleStore::from(&deep);
    for index in [0usize, 0xffff_ffff_0000_0000] {
        let mut input = vec![64, index];
        input.extend(merkle_lanes(deep.root()));
        input.extend(1000..1020);
        let mut expected = vec![0; 4];
        expected.extend(merkle_lanes(deep.root()));
        expected.extend(1000..1020);
        let procedure = selected_procedure(&fixture("m_tree_get", 6, 8), &SelectEnv::default());
        merkle_vm(&merkle_probe(&procedure, &input, &expected), &input, store64.clone(), None)
            .1
            .unwrap();
    }
    let f = fixture("m_tree_verify", 10, 10);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let mut input = merkle_lanes(merkle_word(999));
    input.extend([1, 0]);
    input.extend(merkle_lanes(tree.root()));
    input.extend(1000..1020);
    for (native, must_trap) in [("mtree_verify", true), ("nop", false)] {
        let body = procedure.replace("mtree_verify", native);
        let code = merkle_probe(&body, &input, &input);
        let (_, result) = merkle_vm(&code, &input, store.clone(), None);
        assert_eq!(result.is_err(), must_trap, "{native}");
    }
    // Changing the literal diagnostic remains physically valid; only the trap oracle tells them apart.
    for message in ["bad path", "different path", ""] {
        let args = (0..10).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(",");
        let operands = (0..10).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(",");
        let results = (0..10).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
        let types = ["felt"; 10].join(",");
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @verify({args}) -> ({types}) {{ \
             {results}=hir.m_tree_verify {operands} <{{ message = \"{message}\" }}>; builtin.ret \
             {results} : ({types}); }};"
        ));
        let body = selected_procedure(&f, &SelectEnv::default());
        if message.is_empty() {
            assert!(!body.contains("mtree_verify.err"));
        } else {
            assert!(body.contains(&format!("mtree_verify.err=\"{message}\"")), "{body}");
        }
        let (_, result) =
            merkle_vm(&merkle_probe(&body, &input, &input), &input, store.clone(), None);
        let error = result.unwrap_err();
        let miden_processor::ExecutionError::OperationError {
            err: miden_processor::operation::OperationError::MerklePathVerificationFailed { inner },
            ..
        } = error
        else {
            panic!("{error:?}")
        };
        // Executable Programs omit package debug text; the processor carries its exact hashed literal.
        let expected_code = if message.is_empty() {
            miden_core::Felt::ZERO
        } else {
            miden_core::mast::error_code_from_msg(message)
        };
        assert_eq!(inner.err_code, expected_code);
        assert_eq!(
            inner.err_code == miden_core::mast::error_code_from_msg("bad path"),
            message == "bad path"
        );
        if message == "bad path" {
            for mutated in [
                body.replace("bad path", "different path"),
                body.replace("mtree_verify.err=\"bad path\"", "mtree_verify"),
            ] {
                let (_, result) =
                    merkle_vm(&merkle_probe(&mutated, &input, &input), &input, store.clone(), None);
                let miden_processor::ExecutionError::OperationError {
                    err:
                        miden_processor::operation::OperationError::MerklePathVerificationFailed {
                            inner,
                        },
                    ..
                } = result.unwrap_err()
                else {
                    panic!("expected invalid-proof trap")
                };
                assert_ne!(inner.err_code, miden_core::mast::error_code_from_msg("bad path"));
            }
        }
    }
}

#[test]
fn merkle_source_tuple_candidate_alias_and_physical_violations_reject() {
    use midenc_hir::{Type, Value};
    for (name, n, m) in [
        ("m_tree_get", 6, 8),
        ("m_tree_set", 10, 8),
        ("m_tree_merge", 8, 4),
        ("m_tree_verify", 10, 10),
    ] {
        for ty in [
            Type::U32,
            Type::from(midenc_hir::PointerType::new(Type::Felt)),
            Type::from(midenc_hir::ArrayType::new(Type::Felt, 4)),
        ] {
            for index in 0..n + m {
                let f = fixture(name, n, m);
                let op = f
                    .borrow()
                    .entry_block()
                    .borrow()
                    .body()
                    .front()
                    .get()
                    .unwrap()
                    .as_operation_ref();
                if index < n {
                    let mut value = op.borrow().operands().all()[index].borrow().as_value_ref();
                    value.borrow_mut().set_type(ty.clone());
                } else {
                    let mut value = op.borrow().results().all()[index - n];
                    value.borrow_mut().set_type(ty.clone());
                }
                assert!(
                    select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                    "{name}: {index}, {ty}"
                );
            }
        }
        let f = fixture(name, n, m);
        let function = f.borrow();
        let env = SelectEnv::default();
        for mutation in 0..8 {
            let mut selected = select_function(&function, &env).unwrap();
            let unit = &mut selected.segments[0].units[0];
            match mutation {
                0 => {
                    unit.outputs.pop();
                }
                1 => unit.outputs.push(unit.outputs[0]),
                2 => unit.outputs.swap(0, 1),
                3 => unit.outputs[0] = unit.stack_inputs[n - 1],
                4 => unit.effects.may_trap = false,
                5 => unit.effects.memory.read = true,
                6 => unit.stack_inputs.swap(0, 1),
                _ => unit.kind = crate::plan::LoweringKind(u32::MAX),
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
                "{name}: mutation {mutation}"
            );
        }
        for instruction in [
            crate::masm::Instruction::Dup15,
            crate::masm::Instruction::Swap15,
            crate::masm::Instruction::Drop,
        ] {
            let (plan, frame, mut body) = ready(&f, &env);
            let index = body
                .ops()
                .iter()
                .position(|o| {
                    matches!(
                        o,
                        AnnotatedOp::Instruction {
                            instruction: crate::masm::Instruction::MTreeGet
                                | crate::masm::Instruction::MTreeSet
                                | crate::masm::Instruction::MTreeMerge
                                | crate::masm::Instruction::MTreeVerify,
                            ..
                        }
                    )
                })
                .unwrap();
            let unit = match body.ops()[index] {
                AnnotatedOp::Instruction { unit, .. } => unit,
                _ => unreachable!(),
            };
            body.ops_mut().insert(index, AnnotatedOp::Instruction { instruction, unit });
            assert!(verify::verify(&plan, &frame, body).is_err(), "{name}");
        }
    }
}

#[test]
fn merkle_vm_capacity_floor_and_computed_result_mutations() {
    use miden_core::crypto::{
        hash::Poseidon2,
        merkle::{MerkleStore, MerkleTree, NodeIndex},
    };
    let tree = MerkleTree::new([merkle_word(101), merkle_word(202)]).unwrap();
    let store = MerkleStore::from(&tree);
    // Provider counts unique internal nodes (including its built-in empty subtrees) and sentinel.
    let base_budget = store.num_internal_nodes() * 96 + 8;
    for (name, n, m, native) in [
        ("m_tree_get", 6, 8, "mtree_get"),
        ("m_tree_set", 10, 8, "mtree_set"),
        ("m_tree_merge", 8, 4, "mtree_merge"),
        ("m_tree_verify", 10, 10, "mtree_verify"),
    ] {
        let mut input = vec![];
        let expected = match native {
            "mtree_merge" => {
                input.extend(merkle_lanes(tree.root()));
                input.extend(merkle_lanes(merkle_word(999)));
                merkle_lanes(Poseidon2::merge(&[tree.root(), merkle_word(999)]))
            }
            "mtree_verify" => {
                input.extend(merkle_lanes(merkle_word(101)));
                input.extend([1, 0]);
                input.extend(merkle_lanes(tree.root()));
                input.clone()
            }
            _ => {
                input.extend([1, 0]);
                input.extend(merkle_lanes(tree.root()));
                if n == 10 {
                    input.extend(merkle_lanes(merkle_word(999)));
                }
                let root = if n == 10 {
                    store
                        .clone()
                        .set_node(tree.root(), NodeIndex::new(1, 0).unwrap(), merkle_word(999))
                        .unwrap()
                        .root
                } else {
                    tree.root()
                };
                let mut out = merkle_lanes(merkle_word(101));
                out.extend(merkle_lanes(root));
                out
            }
        };
        for suffix_len in [16 - n, 20] {
            let suffix = (0..suffix_len).map(|i| 7000 + i).collect::<Vec<_>>();
            let mut actual_input = input.clone();
            actual_input.extend(&suffix);
            let mut actual_expected = expected.clone();
            actual_expected.extend(&suffix);
            let selected = selected_procedure(&fixture(name, n, m), &SelectEnv::default());
            for body in [selected.clone(), format!("proc selected {native} end")] {
                merkle_vm(
                    &merkle_probe(&body, &actual_input, &actual_expected),
                    &actual_input,
                    store.clone(),
                    None,
                )
                .1
                .unwrap();
                if m != 10 {
                    let mutated = body.replace(native, &format!("{native} swap.1"));
                    assert!(
                        merkle_vm(
                            &merkle_probe(&mutated, &actual_input, &actual_expected),
                            &actual_input,
                            store.clone(),
                            None
                        )
                        .1
                        .is_err(),
                        "{native} ordered results"
                    );
                }
            }
        }
        let body = selected_procedure(&fixture(name, n, m), &SelectEnv::default());
        let mut actual_input = input.clone();
        actual_input.extend(7000..7020);
        let mut actual_expected = expected.clone();
        actual_expected.extend(7000..7020);
        for budget in [base_budget, base_budget + 31] {
            let (_, result) = merkle_vm(
                &merkle_probe(&body, &actual_input, &actual_expected),
                &actual_input,
                store.clone(),
                Some(budget),
            );
            if native == "mtree_verify" {
                result.unwrap();
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("advice") || error.contains("Advice"), "{error}");
            }
        }
        if native == "mtree_get" {
            merkle_vm(
                &merkle_probe(&body, &actual_input, &actual_expected),
                &actual_input,
                store.clone(),
                Some(base_budget + 32),
            )
            .1
            .unwrap();
        }
        if native == "mtree_set" {
            // Enough room for the temporary word, insufficient for the genuinely new path.
            assert!(
                merkle_vm(
                    &merkle_probe(&body, &actual_input, &actual_expected),
                    &actual_input,
                    store.clone(),
                    Some(base_budget + 32)
                )
                .1
                .is_err()
            );
            let mut no_op_input = vec![1, 0];
            no_op_input.extend(merkle_lanes(tree.root()));
            no_op_input.extend(merkle_lanes(merkle_word(101)));
            no_op_input.extend(7000..7020);
            let mut no_op_expected = merkle_lanes(merkle_word(101));
            no_op_expected.extend(merkle_lanes(tree.root()));
            no_op_expected.extend(7000..7020);
            merkle_vm(
                &merkle_probe(&body, &no_op_input, &no_op_expected),
                &no_op_input,
                store.clone(),
                Some(base_budget + 32),
            )
            .1
            .unwrap();
        }
        if native == "mtree_merge" {
            let mut merged = store.clone();
            merged.merge_roots(tree.root(), merkle_word(999)).unwrap();
            let budget = merged.num_internal_nodes() * 96 + 8;
            merkle_vm(
                &merkle_probe(&body, &actual_input, &actual_expected),
                &actual_input,
                merged,
                Some(budget),
            )
            .1
            .unwrap();
            merkle_vm(
                &merkle_probe(&body, &actual_input, &actual_expected),
                &actual_input,
                store.clone(),
                Some(base_budget + 96),
            )
            .1
            .unwrap();
        }
    }
}

#[test]
fn merkle_retained_alias_mutations_and_physically_valid_verify_omission() {
    for (name, n, m, retained) in [("m_tree_get", 6, 8, 4..8), ("m_tree_verify", 10, 10, 0..10)] {
        let f = fixture(name, n, m);
        let function = f.borrow();
        let env = SelectEnv::default();
        for output in retained {
            for input in 0..n {
                let expected = if m == 8 { output - 2 } else { output };
                if input == expected {
                    continue;
                }
                let mut bad = select_function(&function, &env).unwrap();
                let unit = &mut bad.segments[0].units[0];
                unit.outputs[output] = unit.stack_inputs[input];
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
            let mut bad = select_function(&function, &env).unwrap();
            let atom = bad.segments[0].units[0].outputs[output];
            bad.values.get_mut(&atom.value).unwrap().reconstruction =
                Some(Reconstruction::Alias(vec![bad.segments[0].units[0].stack_inputs[0]]));
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
        let (plan, frame, mut body) = ready(&f, &env);
        let index = body
            .ops()
            .iter()
            .position(|o| {
                matches!(
                    o,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::MTreeGet
                            | crate::masm::Instruction::MTreeVerify,
                        ..
                    }
                )
            })
            .unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        if m == 10 {
            body.ops_mut().remove(index);
            // Correctly physically legal: the invalid-proof VM test must establish execution.
            verify::verify(&plan, &frame, body).unwrap();
        } else {
            body.ops_mut().insert(
                index + 1,
                AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Swap4,
                    unit,
                },
            );
            assert!(verify::verify(&plan, &frame, body).is_err());
        }
    }
}

#[test]
fn merkle_native_substitution_requires_actual_provider_oracle() {
    use miden_core::{
        Felt,
        crypto::{
            hash::Poseidon2,
            merkle::{MerkleStore, MerkleTree},
        },
    };
    let tree = MerkleTree::new([merkle_word(101), merkle_word(202)]).unwrap();
    let store = MerkleStore::from(&tree);
    let right = merkle_word(999);
    let root = Poseidon2::merge(&[tree.root(), right]);
    let mut input = merkle_lanes(tree.root());
    input.extend(merkle_lanes(right));
    input.extend(7000..7020);
    let mut expected = merkle_lanes(root);
    expected.extend(7000..7020);
    let f = fixture("m_tree_merge", 8, 4);
    let selected = selected_procedure(&f, &SelectEnv::default());
    for (body, provider_updated) in
        [(selected.clone(), true), (selected.replace("mtree_merge", "hmerge"), false)]
    {
        // Both bodies calculate exactly the expected Poseidon root; only one stores the parent.
        let (processor, result) =
            merkle_vm(&merkle_probe(&body, &input, &expected), &input, store.clone(), None);
        result.unwrap();
        let (advice, _) = processor.into_parts();
        assert_eq!(advice.get_tree_node(root, Felt::ONE, Felt::ZERO).is_ok(), provider_updated);
    }
    let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
    for op in body.ops_mut() {
        if let AnnotatedOp::Instruction { instruction, .. } = op
            && *instruction == crate::masm::Instruction::MTreeMerge
        {
            *instruction = crate::masm::Instruction::HMerge;
        }
    }
    // HMerge has the same physical lane obligations; the provider oracle above detects substitution.
    verify::verify(&plan, &frame, body).unwrap();
}
