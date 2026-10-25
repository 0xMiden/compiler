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
        ("horner_base", 16, 16),
        ("horner_ext", 16, 16),
        ("log_deferred", 12, 12),
        ("fri_ext_2_fold_4", 17, 16),
        ("mem_stream", 13, 13),
        ("advice_pipe", 13, 13),
        ("crypto_stream", 14, 14),
        ("eval_circuit", 3, 3),
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
         procref.origin call.origin mem_load.120 push.9191 assert_eq repeat.4 drop end end",
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

#[test]
fn horner_and_log_deferred_exact_source_selection() {
    for (name, count) in [("horner_base", 16), ("horner_ext", 16), ("log_deferred", 12)] {
        let f = fixture(name, count, count);
        let selected = checked(&f, &SelectEnv::default());
        assert!(selected.segments[0].units[0].effects.may_trap);
        ready(&f, &SelectEnv::default());
    }
}

#[test]
fn horner_selected_vm_oracle_and_semantic_mutations() {
    for (name, native, extension) in [
        ("horner_base", "horner_eval_base", false),
        ("horner_ext", "horner_eval_ext", true),
    ] {
        for alpha in [[0, 0], [3, 5]] {
            for initial in [[0, 0], [17, 29]] {
                let mut input =
                    (0..16).map(|i| 0xffff_fffe_0000_0100usize + i * 37).collect::<Vec<_>>();
                input[13] = 400;
                input[14..].copy_from_slice(&initial);
                let expected = horner_expected(&input, alpha, extension);
                let initialize =
                    format!("push.{} mem_store.400 push.{} mem_store.401", alpha[0], alpha[1]);
                let f = fixture(name, 16, 16);
                execute_linked_memory(&f, &input, &expected, &initialize, "").unwrap();
                if alpha != [0, 0] && initial != [0, 0] {
                    let procedure = selected_procedure(&f, &SelectEnv::default());
                    for replacement in [
                        "nop",
                        if extension {
                            "horner_eval_base"
                        } else {
                            "horner_eval_ext"
                        },
                    ] {
                        let bad = procedure.replace(native, replacement);
                        let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
                        let all = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
                        let source = format!(
                            "{bad} begin {initialize} {} {} exec.selected {} end",
                            crate::plan::tests::lowering_units::push_top(&suffix),
                            crate::plan::tests::lowering_units::push_top(&input),
                            crate::plan::tests::lowering_units::assert_top(&all)
                        );
                        let error = execute_code(&source).unwrap_err().to_string();
                        assert!(error.contains("assertion failed"), "{error}");
                    }
                }
                for (address, padding) in [(401, 0), (1usize << 32, 0), (400, 1)] {
                    input[13] = address;
                    let initialize = format!("{initialize} push.{padding} mem_store.402");
                    assert!(execute_linked_memory(&f, &input, &expected, &initialize, "").is_err());
                }
            }
        }
    }
}

#[test]
fn log_deferred_selected_two_folds_and_state_oracle() {
    use miden_core::{
        Word,
        deferred::{Node, TRUE_DIGEST},
    };

    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let f = fixture("log_deferred", 12, 12);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let first = deferred_expected(TRUE_DIGEST, TRUE_DIGEST);
    let root = Word::new(first[..4].try_into().unwrap());
    let second = deferred_expected(root, root);
    let mut input = [13, 17, 19, 23, 0, 0, 0, 0, 29, 31, 37, 41];
    let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
    let first_checks = first
        .iter()
        .map(|f| f.as_canonical_u64() as usize)
        .chain(suffix.iter().copied())
        .collect::<Vec<_>>();
    let second_checks = second
        .iter()
        .map(|f| f.as_canonical_u64() as usize)
        .chain(suffix.iter().copied())
        .collect::<Vec<_>>();
    let initial = push_top(&input);
    for (lane, value) in input[4..8].iter_mut().zip(root.as_elements()) {
        *lane = value.as_canonical_u64() as usize;
    }
    let source = format!(
        "{procedure} begin {} {initial} exec.selected {} {} {} exec.selected {} end",
        push_top(&suffix),
        assert_top(&first_checks),
        push_top(&suffix),
        push_top(&input),
        assert_top(&second_checks)
    );
    let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    let program = assembler
        .clone()
        .assemble_program("deferred", source.clone())
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host();
    host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
    let mut processor = processor_with_advice(&[]);
    let mut context = processor.get_initial_resume_context(&program).unwrap();
    while let Some(next) = processor.step_sync(&mut host, context).unwrap() {
        context = next;
    }
    let first_node = Node::and(TRUE_DIGEST, TRUE_DIGEST);
    let second_node = Node::and(root, root);
    assert_eq!(processor.deferred_state().root(), second_node.digest());
    assert_eq!(processor.deferred_state().get_node(&root), Some(&first_node));
    assert_eq!(processor.deferred_state().get_node(&second_node.digest()), Some(&second_node));
    for replacement in ["nop", "hperm", "log_deferred swapw"] {
        let mutant = source.replace("log_deferred", replacement);
        let program = assembler
            .clone()
            .assemble_program("mutant", mutant)
            .unwrap()
            .try_into_program()
            .unwrap();
        let error = processor_with_advice(&[]).execute_sync(&program, &mut host).unwrap_err();
        assert!(
            matches!(
                &error,
                miden_processor::ExecutionError::OperationError {
                    err: miden_processor::operation::OperationError::FailedAssertion { .. },
                    ..
                }
            ),
            "{replacement}: {error}"
        );
    }
    input[4..8].copy_from_slice(&[101, 103, 107, 109]);
    assert!(execute_linked_memory(&f, &input, &[0; 12], "", "").is_err());
}

fn horner_expected(input: &[usize], alpha: [u64; 2], extension: bool) -> Vec<usize> {
    use miden_core::{
        Felt,
        field::{BasedVectorSpace, QuadFelt},
    };
    let alpha = QuadFelt::new(alpha.map(Felt::new_unchecked));
    let mut accumulator =
        QuadFelt::new([input[14], input[15]].map(|n| Felt::new_unchecked(n as u64)));
    if extension {
        for pair in input[..8].as_chunks::<2>().0 {
            accumulator = accumulator * alpha
                + QuadFelt::new([pair[0], pair[1]].map(|n| Felt::new_unchecked(n as u64)));
        }
    } else {
        for coefficient in &input[..8] {
            accumulator =
                accumulator * alpha + QuadFelt::from(Felt::new_unchecked(*coefficient as u64));
        }
    }
    let mut expected = input.to_vec();
    let coefficients: &[Felt] = accumulator.as_basis_coefficients_slice();
    for (lane, value) in expected[14..].iter_mut().zip(coefficients) {
        *lane = value.as_canonical_u64() as usize;
    }
    expected
}

fn deferred_expected(
    root: miden_core::Word,
    statement: miden_core::Word,
) -> [miden_core::Felt; 12] {
    use miden_core::{Felt, crypto::hash::Poseidon2, deferred::Tag};
    let mut state = [Felt::ZERO; 12];
    state[..4].copy_from_slice(root.as_elements());
    state[4..8].copy_from_slice(statement.as_elements());
    state[8..].copy_from_slice(&Tag::AND.as_word());
    Poseidon2::apply_permutation(&mut state);
    state
}

#[test]
fn horner_log_deferred_source_tuples_effects_aliases_and_physical_mutations_reject() {
    use midenc_hir::{SourceSpan, Type};
    for (name, n, m) in [
        ("horner_base", 16, 16),
        ("horner_ext", 16, 16),
        ("log_deferred", 12, 12),
        ("mem_stream", 13, 13),
        ("advice_pipe", 13, 13),
        ("crypto_stream", 14, 14),
        ("eval_circuit", 3, 3),
        ("fri_ext_2_fold_4", 17, 16),
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
                let mut value = if index < n {
                    op.borrow().operands().all()[index].borrow().as_value_ref()
                } else {
                    op.borrow().results().all()[index - n]
                };
                value.borrow_mut().set_type(ty.clone());
                assert!(
                    select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                    "{name} lane {index}"
                );
            }
        }
        for added in [false, true] {
            let f = fixture(name, n, m);
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            if added {
                let extra = f.context.make_result(SourceSpan::UNKNOWN, Type::Felt, op, m as u16);
                op.borrow_mut().results_mut().push(extra);
            } else {
                op.borrow_mut().results_mut().group_mut(0).pop().unwrap();
            }
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
        let f = fixture(name, n, m);
        let function = f.borrow();
        let env = SelectEnv::default();
        let selected = select_function(&function, &env).unwrap();
        let source_unit = &selected.segments[0].units[0];
        assert_eq!(
            source_unit.effects.memory.read,
            matches!(
                name,
                "horner_base" | "horner_ext" | "mem_stream" | "crypto_stream" | "eval_circuit"
            )
        );
        assert_eq!(
            source_unit.effects.memory.write,
            matches!(name, "advice_pipe" | "crypto_stream")
        );
        let aliases = match n {
            3 => (0..3).map(|i| (i, i)).collect::<Vec<_>>(),
            16 => (0..14).map(|i| (i, i)).collect::<Vec<_>>(),
            13 | 14 => (8..12).map(|i| (i, i)).collect(),
            17 => vec![(11, 8), (15, 16)],
            _ => vec![],
        };
        let evidence = crate::select::evidence(&function, &env).unwrap();
        for (output, input) in aliases {
            assert!(
                evidence.source.units[&source_unit.id]
                    .aliases
                    .pairs()
                    .contains(&(source_unit.outputs[output], source_unit.stack_inputs[input]))
            );
        }
        for mutation in 0..7 {
            let mut bad = selected.clone();
            let unit = &mut bad.segments[0].units[0];
            match mutation {
                0 => {
                    unit.outputs.pop();
                }
                1 => {
                    unit.stack_inputs.pop();
                }
                2 => unit.outputs.swap(0, 1),
                3 => unit.effects.may_trap = false,
                4 => unit.effects.memory.read = !unit.effects.memory.read,
                5 => unit.effects.memory.write = !unit.effects.memory.write,
                _ => unit.stack_inputs.swap(0, 1),
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
                "{name} mutation {mutation}"
            );
        }
        for instruction in [
            crate::masm::Instruction::Dup15,
            if n == 17 {
                crate::masm::Instruction::Swap8
            } else {
                crate::masm::Instruction::Swap15
            },
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
                            instruction: crate::masm::Instruction::HornerBase
                                | crate::masm::Instruction::HornerExt
                                | crate::masm::Instruction::LogDeferred
                                | crate::masm::Instruction::MemStream
                                | crate::masm::Instruction::AdvPipe
                                | crate::masm::Instruction::CryptoStream
                                | crate::masm::Instruction::EvalCircuit
                                | crate::masm::Instruction::FriExt2Fold4,
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
            assert!(verify::verify(&plan, &frame, body).is_err());
        }
    }
}

#[test]
fn horner_source_local_alpha_and_spill_normalization() {
    use midenc_hir::Type;
    for (name, extension) in [("horner_base", false), ("horner_ext", true)] {
        let args = (0..20)
            .map(|i| format!("%s{i}: felt"))
            .chain((0..16).filter(|i| *i != 13).map(|i| format!("%a{i}: felt")))
            .chain(["%alpha: u128".into()])
            .collect::<Vec<_>>()
            .join(",");
        let operands = (0..16)
            .map(|i| {
                if i == 13 {
                    "%address".into()
                } else {
                    format!("%a{i}")
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let results = (0..16).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
        // Returning only accumulator lanes avoids publishing the local pointer; all fourteen
        // retained lanes are independently checked in the full-tuple VM tests.
        let returned = (0..20)
            .map(|i| format!("%s{i}"))
            .chain(["%r14".into(), "%r15".into()])
            .collect::<Vec<_>>()
            .join(",");
        let types = vec!["felt"; 22].join(",");
        let source = format!(
            "builtin.function public extern(\"C\") @local_alpha({args}) -> ({types}) {{ \
             hir.store_local %alpha <{{ local = #builtin.local_variable<0, u128> }}> : (u128); \
             %ptr = hir.local_address <{{ local = #builtin.local_variable<0, u128> }}>; %bits = \
             hir.ptr_to_int %ptr <{{ ty = #builtin.type<u32> }}>; %address = hir.bitcast %bits \
             <{{ ty = #builtin.type<felt> }}>; {results} = hir.{name} {operands}; builtin.ret \
             {returned} : ({types}); }};"
        );
        let mut f = parse(&source);
        f.function.borrow_mut().alloc_local(Type::U128);
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        assert!(!frame.spills().is_empty());
        verify::verify(&plan, &frame, body).unwrap();
        let mut native = (0..16).map(|i| 101 + i * 37).collect::<Vec<_>>();
        native[13] = 0;
        let expected_native = horner_expected(&native, [3, 5], extension);
        let sentinels = (0..20).map(|i| 7000 + i).collect::<Vec<_>>();
        let inputs = sentinels
            .iter()
            .copied()
            .chain(native.iter().enumerate().filter(|(i, _)| *i != 13).map(|(_, v)| *v))
            .chain([3, 5, 0, 0])
            .collect::<Vec<_>>();
        let expected = sentinels
            .iter()
            .copied()
            .chain(expected_native[14..].iter().copied())
            .collect::<Vec<_>>();
        execute_linked(&f, &inputs, &expected);
        // Redirect the actual source-local address to compiler storage in the same frame.
        for unknown in [false, true] {
            let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
            let index = body
                .ops()
                .iter()
                .position(|o| {
                    matches!(
                        o,
                        AnnotatedOp::Instruction {
                            instruction: crate::masm::Instruction::Locaddr(_),
                            ..
                        }
                    )
                })
                .unwrap();
            let unit = match body.ops()[index] {
                AnnotatedOp::Instruction { unit, .. } => unit,
                _ => unreachable!(),
            };
            let instruction = match &mut body.ops_mut()[index] {
                AnnotatedOp::Instruction { instruction, .. } => instruction,
                _ => unreachable!(),
            };
            *instruction = crate::masm::Instruction::Locaddr(4u16.into());
            if unknown {
                body.ops_mut().insert(
                    index + 1,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::AddImm(miden_core::Felt::ONE.into()),
                        unit,
                    },
                );
            }
            assert!(verify::verify(&plan, &frame, body).is_err());
        }
    }
}

#[test]
fn memstream_exact_source_selection() {
    let f = fixture("mem_stream", 13, 13);
    let selected = checked(&f, &SelectEnv::default());
    assert!(selected.segments[0].units[0].effects.memory.read);
    ready(&f, &SelectEnv::default());
}

#[test]
fn memstream_selected_vm_values_pointer_suffix_and_semantic_mutations() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let f = fixture("mem_stream", 13, 13);
    let (_, frame, _) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.aligned_size(), 0);
    assert!(frame.spills().is_empty());
    let procedure = selected_procedure(&f, &SelectEnv::default());
    for address in [400usize, 0xffff_fff8] {
        for written in [0, 4, 8] {
            let memory = (0..8)
                .map(|i| {
                    if address == 0xffff_fff8 && i == 6 {
                        miden_core::FMP_INIT_VALUE.as_canonical_u64() as usize
                    } else if i < written {
                        0xffff_fffe_0000_0100usize + i * 37
                    } else {
                        0
                    }
                })
                .collect::<Vec<_>>();
            let initialize = memory
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    address == 0xffff_fff8 && *i != 6
                        || *i < written && !(address == 0xffff_fff8 && *i == 6)
                })
                .map(|(i, v)| format!("push.{v} mem_store.{}", address + i))
                .collect::<Vec<_>>()
                .join(" ");
            let mut input = (0..13).map(|i| 101 + i * 37).collect::<Vec<_>>();
            input[12] = address;
            let expected = memory
                .iter()
                .copied()
                .chain(input[8..12].iter().copied())
                .chain([address + 8])
                .collect::<Vec<_>>();
            execute_linked_memory(
                &f,
                &input,
                &expected,
                &initialize,
                "mem_load.4294967294 push.2147483648 assert_eq",
            )
            .unwrap();
            // The actual selected definition also executes at exactly sixteen physical lanes.
            let suffix = [901, 902, 903];
            let all = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
            let source = format!(
                "{procedure} begin {initialize} {} {} exec.selected {} end",
                push_top(&suffix),
                push_top(&input),
                assert_top(&all)
            );
            execute_code(&source).unwrap();
            if written == 8 {
                for replacement in
                    ["nop", "mem_stream swap.1", "mem_stream movup.12 add.1 movdn.12"]
                {
                    let error = execute_code(&source.replace("mem_stream", replacement))
                        .unwrap_err()
                        .to_string();
                    assert!(error.contains("assertion failed"), "{error}");
                }
            }
        }
    }
    for address in [401usize, 1usize << 32, 0xffff_fffc] {
        let mut input = (0..13).map(|i| 101 + i * 37).collect::<Vec<_>>();
        input[12] = address;
        let source = format!("{procedure} begin {} exec.selected end", push_top(&input));
        let program = miden_assembly::Assembler::default()
            .assemble_program("stream_trap", source)
            .unwrap()
            .try_into_program()
            .unwrap();
        let (processor, error) = execute_failure_state(&program, &[]);
        assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
        assert_eq!(
            processor
                .stack_top()
                .iter()
                .rev()
                .take(13)
                .map(|f| f.as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            input
        );
    }
    // Preserve the end pointer as Felt2^32; its next native read must trap, not wrap.
    let input = (0..12).map(|i| 101 + i * 37).chain([0xffff_fff8]).collect::<Vec<_>>();
    let source = format!("{procedure} begin {} exec.selected exec.selected end", push_top(&input));
    let program = miden_assembly::Assembler::default()
        .assemble_program("stream_end", source)
        .unwrap()
        .try_into_program()
        .unwrap();
    let (processor, error) = execute_failure_state(&program, &[]);
    assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
    assert_eq!(processor.stack_top()[3].as_canonical_u64(), 1u64 << 32);
}

#[test]
fn memstream_source_local_storage_spills_ranges_and_alignment() {
    use midenc_hir::Type;

    use crate::plan::tests::lowering_units::push_top;
    let args = (0..20)
        .map(|i| format!("%s{i}: felt"))
        .chain((0..12).map(|i| format!("%a{i}: felt")))
        .chain(["%word0: u128".into(), "%word1: u128".into()])
        .collect::<Vec<_>>()
        .join(",");
    let operands = (0..12)
        .map(|i| format!("%a{i}"))
        .chain(["%address".into()])
        .collect::<Vec<_>>()
        .join(",");
    let results = (0..13).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
    let returned = (0..20)
        .map(|i| format!("%s{i}"))
        .chain((0..12).map(|i| format!("%r{i}")))
        .collect::<Vec<_>>()
        .join(",");
    let types = vec!["felt"; 32].join(",");
    let source = format!(
        "builtin.function public extern(\"C\") @stream_local({args}) -> ({types}) {{ \
         hir.store_local %word0 <{{ local = #builtin.local_variable<0,u128> }}> : (u128); \
         hir.store_local %word1 <{{ local = #builtin.local_variable<1,u128> }}> : (u128); %ptr = \
         hir.local_address <{{ local = #builtin.local_variable<0,u128> }}>; %bits = \
         hir.ptr_to_int %ptr <{{ ty = #builtin.type<u32> }}>; %address = hir.bitcast %bits <{{ ty \
         = #builtin.type<felt> }}>; {results} = hir.mem_stream {operands}; builtin.ret {returned} \
         : ({types}); }};"
    );
    let mut f = parse(&source);
    for _ in 0..3 {
        f.function.borrow_mut().alloc_local(Type::U128);
    }
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert!(!frame.spills().is_empty());
    verify::verify(&plan, &frame, body).unwrap();
    let memory = [101, 103, 107, 109, 127, 131, 137, 139];
    let sentinels = (0..20).map(|i| 7000 + i).collect::<Vec<_>>();
    let old = (0..12).map(|i| 201 + i * 37).collect::<Vec<_>>();
    let input = sentinels
        .iter()
        .chain(&old)
        .copied()
        .chain(memory[..4].iter().copied())
        .chain(memory[4..].iter().copied())
        .collect::<Vec<_>>();
    let expected = sentinels.iter().chain(&memory).chain(&old[8..12]).copied().collect::<Vec<_>>();
    execute_linked(&f, &input, &expected);
    for offset in [1u16, 8, 12, 13] {
        let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
        let index = body
            .ops()
            .iter()
            .position(|op| {
                matches!(
                    op,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::MemStream,
                        ..
                    }
                )
            })
            .unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        let ops = [
            crate::masm::Instruction::MovUp12,
            crate::masm::Instruction::Drop,
            crate::masm::Instruction::Locaddr(offset.min(12).into()),
            crate::masm::Instruction::MovDn12,
        ];
        for (i, instruction) in ops.into_iter().enumerate() {
            body.ops_mut().insert(index + i, AnnotatedOp::Instruction { instruction, unit });
        }
        if offset == 13 {
            body.ops_mut().insert(
                index + 3,
                AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Dup1,
                    unit,
                },
            );
            body.ops_mut().insert(
                index + 4,
                AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Add,
                    unit,
                },
            );
        }
        if offset == 1 {
            let procedure = verify::verify(&plan, &frame, body)
                .unwrap()
                .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
                .unwrap()
                .to_pretty_string();
            let source = format!("{procedure} begin {} exec.selected end", push_top(&input));
            let intrinsics = crate::intrinsics::load(f.context.session()).unwrap();
            let mut assembler = miden_assembly::Assembler::default();
            assembler
                .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
                .unwrap();
            let program = assembler
                .assemble_program("misaligned_local", source)
                .unwrap()
                .try_into_program()
                .unwrap();
            let mut host = crate::plan::tests::lowering_units::host();
            host.load_library(miden_processor::HostLibrary::from(intrinsics)).unwrap();
            let mut processor = processor_with_advice(&[]);
            let mut context = processor.get_initial_resume_context(&program).unwrap();
            let error = loop {
                match processor.step_sync(&mut host, context) {
                    Ok(Some(next)) => context = next,
                    Ok(None) => panic!("expected alignment trap"),
                    Err(error) => break error,
                }
            };
            assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
        } else {
            let error = verify::verify(&plan, &frame, body).unwrap_err();
            if offset == 13 {
                assert_eq!(error.category, DiagnosticCategory::IllegalStackAccess);
                assert!(error.to_string().contains("compiler address arithmetic is not exact"));
            } else {
                assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
            }
        }
    }
}

#[test]
fn memstream_repeated_reads() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let f = fixture("mem_stream", 13, 13);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let memory = (0..16).map(|i| 0xffff_fffe_0000_0100usize + i * 37).collect::<Vec<_>>();
    let initialize = memory
        .iter()
        .enumerate()
        .map(|(i, v)| format!("push.{v} mem_store.{}", 400 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let input = (0..12).map(|i| 101 + i * 37).chain([400]).collect::<Vec<_>>();
    let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
    let expected = memory[8..]
        .iter()
        .chain(&input[8..12])
        .copied()
        .chain([416])
        .chain(suffix.iter().copied())
        .collect::<Vec<_>>();
    execute_code(&format!(
        "{procedure} begin {initialize} {} {} exec.selected exec.selected {} end",
        push_top(&suffix),
        push_top(&input),
        assert_top(&expected)
    ))
    .unwrap();
}

#[test]
fn horner_selected_floor_and_traps_preserve_unpublished_inputs() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    for (name, native, extension) in [
        ("horner_base", "horner_eval_base", false),
        ("horner_ext", "horner_eval_ext", true),
    ] {
        let f = fixture(name, 16, 16);
        let selected = selected_procedure(&f, &SelectEnv::default());
        let mut input = (0..16).map(|i| 101 + i * 37).collect::<Vec<_>>();
        input[13] = 400;
        let expected = horner_expected(&input, [3, 5], extension);
        execute_code(&format!(
            "{selected} begin push.3 mem_store.400 push.5 mem_store.401 {} exec.selected {} end",
            push_top(&input),
            assert_top(&expected)
        ))
        .unwrap();
        for procedure in [selected, format!("proc selected {native} end")] {
            for (address, padding) in [(401usize, 0), (1usize << 32, 0), (400, 1)] {
                input[13] = address;
                let source = format!(
                    "{procedure} begin push.{padding} mem_store.402 {} exec.selected end",
                    push_top(&input)
                );
                let program = miden_assembly::Assembler::default()
                    .assemble_program("horner_trap", source)
                    .unwrap()
                    .try_into_program()
                    .unwrap();
                let (processor, error) = execute_failure_state(&program, &[]);
                if padding == 0 {
                    assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
                } else {
                    assert!(matches!(
                        error,
                        miden_processor::ExecutionError::OperationError {
                            err: miden_processor::operation::OperationError::InvalidHornerEvaluationPointWord { addr: 400, .. },
                            ..
                        }
                    ));
                }
                assert_eq!(
                    processor
                        .stack_top()
                        .iter()
                        .rev()
                        .map(|f| f.as_canonical_u64() as usize)
                        .collect::<Vec<_>>(),
                    input
                );
            }
        }
    }
}

#[test]
fn fri_exact_seventeen_input_source_selection() {
    let f = fixture("fri_ext_2_fold_4", 17, 16);
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments[0].units[0].stack_inputs.len(), 17);
    ready(&f, &SelectEnv::default());
}

/// Build samples from a polynomial, independently of the processor's folding helpers.
fn fri_case(coset: usize, constant: bool) -> (Vec<usize>, Vec<usize>) {
    use miden_core::{
        Felt,
        field::{BasedVectorSpace, Field, QuadFelt, TwoAdicField},
    };
    let q = |a, b| QuadFelt::new([Felt::new_unchecked(a), Felt::new_unchecked(b)]);
    let coefficients = if constant {
        [q(19, 23), q(0, 0), q(0, 0), q(0, 0)]
    } else {
        [q(0xffff_fffe_0000_0100, 23), q(29, 31), q(37, 41), q(43, 47)]
    };
    let evaluate =
        |point: QuadFelt| coefficients.iter().rev().fold(q(0, 0), |acc, c| acc * point + *c);
    let alpha = q(53, 59);
    let poe = Felt::new_unchecked(7);
    let tau = Felt::two_adic_generator(2);
    let x = poe * tau.exp_u64(coset as u64).inverse();
    let samples = [
        evaluate(QuadFelt::from(x)),
        evaluate(QuadFelt::from(-x)),
        evaluate(QuadFelt::from(x * tau)),
        evaluate(QuadFelt::from(-x * tau)),
    ];
    let flatten = |value: QuadFelt| {
        let limbs: &[Felt] = value.as_basis_coefficients_slice();
        limbs.iter().map(|f| f.as_canonical_u64() as usize).collect::<Vec<_>>()
    };
    let mut input = samples.into_iter().flat_map(flatten).collect::<Vec<_>>();
    input.extend([701, coset, poe.as_canonical_u64() as usize]);
    input.extend(flatten(evaluate(QuadFelt::from(poe))));
    input.extend(flatten(alpha));
    let cptr = Felt::new_unchecked(0xffff_ffff_0000_0000);
    input.extend([cptr.as_canonical_u64() as usize, 809]);
    let x2 = x * x;
    let tmp0 =
        coefficients[0] + coefficients[2] * x2 + alpha * (coefficients[1] + coefficients[3] * x2);
    let tmp1 =
        coefficients[0] - coefficients[2] * x2 + alpha * (coefficients[1] - coefficients[3] * x2);
    let pointer = (cptr + Felt::new_unchecked(8)).as_canonical_u64() as usize;
    let mut expected = flatten(tmp0);
    expected.extend(flatten(tmp1));
    expected.extend((1..4).map(|i| usize::from(i == coset)));
    expected.extend([
        (poe * poe).as_canonical_u64() as usize,
        pointer,
        pointer,
        poe.exp_u64(4).as_canonical_u64() as usize,
        701,
    ]);
    expected.extend(flatten(evaluate(alpha)));
    expected.extend([pointer, 809]);
    (input, expected)
}

fn fri_ready(
    f: &Parsed,
    force: bool,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    use crate::allocate::schedule::SchedulingTier;
    let function = f.borrow();
    let env = SelectEnv::default();
    let evidence = crate::select::evidence(&function, &env).unwrap();
    let selected = check_selection(
        evidence.source.program,
        &SourceContext::Hir {
            function: &function,
            env: &env,
        },
    )
    .unwrap();
    let options = AllocationOptions {
        force_tier: force.then_some(SchedulingTier::Normalization),
        ..Default::default()
    };
    let mut plan = allocate(selected, &options).unwrap();
    let frame = assign_frame(&plan, &evidence.source.source_locals).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut registry = LoweringRegistry::default();
    for definition in &evidence.definitions {
        registry.register(definition).unwrap();
    }
    let body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    (plan, frame, body)
}

#[test]
fn fri_selected_polynomial_oracle_normalization_live_through_and_semantics() {
    use crate::plan::{
        actions::PlanAction,
        tests::lowering_units::{assert_top, push_top},
    };
    for live in [false, true] {
        let original = fixture("fri_ext_2_fold_4", 17, 16);
        let f = if live {
            let args = (0..17).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(",");
            let operands = (0..17).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(",");
            let results = (0..16).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
            let types = vec!["felt"; 18].join(",");
            parse(&format!(
                "builtin.function public extern(\"C\") @fri_live({args}) -> ({types}) {{ \
                 {results} = hir.fri_ext_2_fold_4 {operands}; builtin.ret {results},%a8,%a16 : \
                 ({types}); }};"
            ))
        } else {
            original
        };
        for force in [false, true] {
            let (plan, frame, body) = fri_ready(&f, force);
            let unit = &plan.selected.segments[0].units[0];
            assert_eq!(unit.stack_inputs.len(), 17);
            if force {
                assert!(!frame.spills().is_empty());
            }
            for action in plan
                .segment_actions
                .values()
                .flatten()
                .chain(plan.edge_actions.values().flatten())
            {
                if let PlanAction::Dup { depth }
                | PlanAction::Swap { depth }
                | PlanAction::MoveUp { depth }
                | PlanAction::MoveDown { depth } = action
                {
                    assert!(*depth <= 15);
                }
            }
            if force {
                assert!(
                    plan.segment_actions
                        .values()
                        .flatten()
                        .any(|a| matches!(a, PlanAction::StoreLane(_)))
                );
            }
            let procedure = verify::verify(&plan, &frame, body)
                .unwrap()
                .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
                .unwrap()
                .to_pretty_string();
            for coset in 0..4 {
                for constant in [false, true] {
                    let (input, mut expected) = fri_case(coset, constant);
                    if live {
                        expected.extend([input[8], input[16]]);
                    }
                    for &suffix_len in if live { &[20][..] } else { &[0, 20][..] } {
                        let suffix = (0..suffix_len).map(|i| 900 + i).collect::<Vec<_>>();
                        let all = input.iter().chain(&suffix).copied().collect::<Vec<_>>();
                        let expected = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
                        let base = all[all.len() - 16..]
                            .iter()
                            .map(|n| miden_core::Felt::new_unchecked(*n as u64))
                            .collect::<Vec<_>>();
                        let source = format!(
                            "{procedure} begin {} exec.selected {} end",
                            push_top(&all[..all.len() - 16]),
                            assert_top(&expected)
                        );
                        let program = miden_assembly::Assembler::default()
                            .assemble_program("fri_selected", source)
                            .unwrap()
                            .try_into_program()
                            .unwrap();
                        miden_processor::FastProcessor::new(
                            miden_processor::StackInputs::new(&base).unwrap(),
                        )
                        .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
                        .unwrap();
                    }
                }
            }
        }
    }
    let f = fixture("fri_ext_2_fold_4", 17, 16);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let (input, expected) = fri_case(1, false);
    let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
    let all_expected = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
    for replacement in ["drop", "fri_ext2fold4 swap.1", "fri_ext2fold4 movup.8 add.1 movdn.8"] {
        let source = format!(
            "{} begin {} {} exec.selected {} end",
            procedure.replace("fri_ext2fold4", replacement),
            push_top(&suffix),
            push_top(&input),
            assert_top(&all_expected)
        );
        let error = execute_code(&source).unwrap_err().to_string();
        assert!(error.contains("assertion failed"), "{error}");
    }
}

#[test]
fn fri_and_compound_actual_source_operand_counts_reject() {
    for (name, n, m) in [
        ("horner_base", 16, 16),
        ("horner_ext", 16, 16),
        ("log_deferred", 12, 12),
        ("mem_stream", 13, 13),
        ("advice_pipe", 13, 13),
        ("crypto_stream", 14, 14),
        ("fri_ext_2_fold_4", 17, 16),
    ] {
        for added in [false, true] {
            let f = fixture(name, n, m);
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            if added {
                let value = op.borrow().operands().all()[0].borrow().as_value_ref();
                op.borrow_mut().operands_mut().push(f.context.make_operand(value, op, n as u16));
            } else {
                op.borrow_mut().operands_mut().group_mut(0).pop().unwrap();
            }
            assert!(
                select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                "{name} added={added}"
            );
        }
    }
}

#[test]
fn fri_and_compound_physically_valid_omissions_need_vm_oracles() {
    for (name, n, m) in [
        ("horner_base", 16, 16),
        ("horner_ext", 16, 16),
        ("log_deferred", 12, 12),
        ("mem_stream", 13, 13),
        ("fri_ext_2_fold_4", 17, 16),
    ] {
        let f = fixture(name, n, m);
        let mut input = (0..n).map(|i| 101 + i * 37).collect::<Vec<_>>();
        let (expected, initialize) = match name {
            "horner_base" | "horner_ext" => {
                input[13] = 400;
                (
                    horner_expected(&input, [3, 5], name == "horner_ext"),
                    "push.3 mem_store.400 push.5 mem_store.401".into(),
                )
            }
            "log_deferred" => {
                input[4..8].fill(0);
                (
                    deferred_expected(
                        miden_core::deferred::TRUE_DIGEST,
                        miden_core::deferred::TRUE_DIGEST,
                    )
                    .iter()
                    .map(|f| f.as_canonical_u64() as usize)
                    .collect(),
                    String::new(),
                )
            }
            "mem_stream" => {
                input[12] = 400;
                let memory = (0..8).map(|i| 701 + i * 37).collect::<Vec<_>>();
                let initialize = memory
                    .iter()
                    .enumerate()
                    .map(|(i, v)| format!("push.{v} mem_store.{}", 400 + i))
                    .collect::<Vec<_>>()
                    .join(" ");
                (
                    memory.into_iter().chain(input[8..12].iter().copied()).chain([408]).collect(),
                    initialize,
                )
            }
            _ => {
                let (values, expected) = fri_case(1, false);
                input = values;
                (expected, String::new())
            }
        };
        let error = execute_linked_memory_mutate(&f, &input, &expected, &initialize, "", |body| {
            let index = body
                .ops()
                .iter()
                .position(|op| {
                    matches!(
                        op,
                        AnnotatedOp::Instruction {
                            instruction: crate::masm::Instruction::HornerBase
                                | crate::masm::Instruction::HornerExt
                                | crate::masm::Instruction::LogDeferred
                                | crate::masm::Instruction::MemStream
                                | crate::masm::Instruction::FriExt2Fold4,
                            ..
                        }
                    )
                })
                .unwrap();
            let unit = match body.ops()[index] {
                AnnotatedOp::Instruction { unit, .. } => unit,
                _ => unreachable!(),
            };
            body.ops_mut().remove(index);
            let replacement = if n == 17 {
                vec![
                    crate::masm::Instruction::MovUp8,
                    crate::masm::Instruction::MovDn12,
                    crate::masm::Instruction::Drop,
                ]
            } else {
                vec![crate::masm::Instruction::Nop]
            };
            for (i, instruction) in replacement.into_iter().enumerate() {
                body.ops_mut().insert(index + i, AnnotatedOp::Instruction { instruction, unit });
            }
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("assertion failed"), "{name}: {error}");
    }
}

#[test]
fn horner_and_compound_selected_actual_minimum_floor() {
    for (name, n) in [
        ("horner_base", 16),
        ("horner_ext", 16),
        ("log_deferred", 12),
        ("mem_stream", 13),
    ] {
        let f = fixture(name, n, n);
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        assert_eq!(frame.aligned_size(), 0);
        assert!(frame.spills().is_empty());
        let procedure = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
            .unwrap()
            .to_pretty_string();
        let mut input = (0..n).map(|i| 101 + i * 37).collect::<Vec<_>>();
        let (mut expected, initialize) = match name {
            "horner_base" | "horner_ext" => {
                input[13] = 400;
                (
                    horner_expected(&input, [3, 5], name == "horner_ext"),
                    "push.3 mem_store.400 push.5 mem_store.401",
                )
            }
            "log_deferred" => {
                input[4..8].fill(0);
                (
                    deferred_expected(
                        miden_core::deferred::TRUE_DIGEST,
                        miden_core::deferred::TRUE_DIGEST,
                    )
                    .iter()
                    .map(|f| f.as_canonical_u64() as usize)
                    .collect(),
                    "",
                )
            }
            _ => {
                input[12] = 400;
                ((0..8).map(|_| 0).chain(input[8..12].iter().copied()).chain([408]).collect(), "")
            }
        };
        let suffix = (0..16 - n).map(|i| 900 + i).collect::<Vec<_>>();
        input.extend(&suffix);
        expected.extend(&suffix);
        let felts = input
            .iter()
            .map(|n| miden_core::Felt::new_unchecked(*n as u64))
            .collect::<Vec<_>>();
        let program = miden_assembly::Assembler::default()
            .assemble_program(
                "minimum_floor",
                format!("{procedure} begin {initialize} exec.selected end"),
            )
            .unwrap()
            .try_into_program()
            .unwrap();
        let output =
            miden_processor::FastProcessor::new(miden_processor::StackInputs::new(&felts).unwrap())
                .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
                .unwrap();
        assert_eq!(
            (0..16)
                .map(|i| output.stack.get_element(i).unwrap().as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn fri_selected_traps_no_publication_and_seventeenth_preparation() {
    use crate::plan::tests::lowering_units::push_top;
    let f = fixture("fri_ext_2_fold_4", 17, 16);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let (input, _) = fri_case(1, false);
    for (index, value) in [
        (9, 4),
        (9, 0xffff_fffe_0000_0100),
        (10, 0),
        (11, input[11] + 1),
        (12, input[12] + 1),
    ] {
        let mut bad = input.clone();
        bad[index] = value;
        let program = miden_assembly::Assembler::default()
            .assemble_program(
                "fri_trap",
                format!("{procedure} begin {} exec.selected end", push_top(&bad)),
            )
            .unwrap()
            .try_into_program()
            .unwrap();
        let (processor, error) = execute_failure_state(&program, &[]);
        assert!(matches!(
            error,
            miden_processor::ExecutionError::OperationError {
                err: miden_processor::operation::OperationError::FriError(_),
                ..
            }
        ));
        assert_eq!(
            (0..17)
                .map(|i| processor.stack_get(i).as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            bad
        );
    }
    let (mut plan, frame, _) = fri_ready(&f, true);
    let actions = plan.segment_actions.values_mut().next().unwrap();
    let run = actions
        .iter()
        .position(|a| matches!(a, crate::plan::actions::PlanAction::RunLowering { .. }))
        .unwrap();
    let index = actions[..run]
        .iter()
        .rposition(|a| matches!(a, crate::plan::actions::PlanAction::LoadLane(_)))
        .unwrap();
    actions.remove(index);
    let evidence = crate::select::evidence(&f.borrow(), &SelectEnv::default()).unwrap();
    let mut registry = LoweringRegistry::default();
    for definition in &evidence.definitions {
        registry.register(definition).unwrap();
    }
    match expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()) {
        Ok(body) => assert!(verify::verify(&plan, &frame, body).is_err()),
        Err(error) => assert!(
            error.to_string().contains("stack") || error.to_string().contains("prefix"),
            "{error}"
        ),
    }
}

#[test]
fn log_deferred_unknown_statement_has_no_result_publication() {
    use crate::plan::tests::lowering_units::push_top;
    let input = [13, 17, 19, 23, 101, 103, 107, 109, 29, 31, 37, 41];
    let f = fixture("log_deferred", 12, 12);
    for procedure in [
        selected_procedure(&f, &SelectEnv::default()),
        "proc selected log_deferred end".into(),
    ] {
        let program = miden_assembly::Assembler::default()
            .assemble_program(
                "unknown_statement",
                format!("{procedure} begin {} exec.selected end", push_top(&input)),
            )
            .unwrap()
            .try_into_program()
            .unwrap();
        let (processor, error) = execute_failure_state(&program, &[]);
        assert!(matches!(
            error,
            miden_processor::ExecutionError::OperationError {
                err: miden_processor::operation::OperationError::Deferred(_),
                ..
            }
        ));
        assert_eq!(
            (0..12)
                .map(|i| processor.stack_get(i).as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            input
        );
        assert_eq!(processor.deferred_state().root(), miden_core::deferred::TRUE_DIGEST);
    }
}

#[test]
fn memstream_and_horner_read_permission_does_not_authorize_native_writes() {
    for (name, n) in [("horner_base", 16), ("horner_ext", 16), ("mem_stream", 13)] {
        let f = fixture(name, n, n);
        let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
        let index = body
            .ops()
            .iter()
            .position(|op| {
                matches!(
                    op,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::HornerBase
                            | crate::masm::Instruction::HornerExt
                            | crate::masm::Instruction::MemStream,
                        ..
                    }
                )
            })
            .unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        let push =
            |value| Primitive::Push(miden_core::Felt::new_unchecked(value)).instruction().unwrap();
        for (i, instruction) in
            [push(0), push(400), crate::masm::Instruction::MemStore].into_iter().enumerate()
        {
            body.ops_mut().insert(index + i, AnnotatedOp::Instruction { instruction, unit });
        }
        let error = verify::verify(&plan, &frame, body).unwrap_err().to_string();
        assert!(error.contains("unregistered source memory effect"), "{name}: {error}");
    }
}

#[test]
fn stream_write_exact_source_selection() {
    for (name, width) in [("advice_pipe", 13), ("crypto_stream", 14)] {
        let f = fixture(name, width, width);
        let selected = checked(&f, &SelectEnv::default());
        assert!(selected.segments[0].units[0].effects.memory.write);
        assert_eq!(selected.segments[0].units[0].effects.memory.read, width == 14);
        ready(&f, &SelectEnv::default());
    }
}

/// Execute the public selected body with an actual sixteen-lane initial floor and overflow.
fn stream_write_run(
    procedure: &str,
    input: &[usize],
    expected: &[usize],
    initialize: &str,
    checks: &str,
    advice: &[u64],
) -> Result<(), midenc_hir::Report> {
    use miden_core::Felt;
    use miden_processor::{FastProcessor, StackInputs, advice::AdviceInputs};

    use crate::plan::tests::lowering_units::{assert_top, push_top};
    assert!(input.len() >= 16);
    let code = format!(
        "{procedure} begin {initialize} {} sdepth push.{} assert_eq exec.selected sdepth push.{} \
         assert_eq {checks} {} end",
        push_top(&input[..input.len() - 16]),
        input.len(),
        input.len(),
        assert_top(expected)
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("stream_write", code)?
        .try_into_program()?;
    let base = input[input.len() - 16..]
        .iter()
        .map(|n| Felt::new_unchecked(*n as u64))
        .collect::<Vec<_>>();
    FastProcessor::new(StackInputs::new(&base).unwrap())
        .with_advice(
            AdviceInputs::default()
                .with_stack(advice.iter().copied().map(Felt::new_unchecked).collect()),
        )
        .unwrap()
        .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
        .map_err(|e| midenc_hir::Report::msg(e.to_string()))?;
    Ok(())
}

#[test]
fn stream_write_advice_selected_all_lanes_provider_memory_floor_and_mutants() {
    let f = fixture("advice_pipe", 13, 13);
    let (_, frame, _) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.aligned_size(), 0);
    assert!(frame.spills().is_empty());
    let procedure = selected_procedure(&f, &SelectEnv::default());
    for suffix_len in [3, 20] {
        for dst in [400usize, 0xffff_fff8] {
            let advice = [0, 1, 0xffff_ffff_0000_0000, 37, 53, 79, 2147483648, 113, 97, 98];
            let mut input = (0..12).map(|i| 101 + i * 37).chain([dst]).collect::<Vec<_>>();
            input.extend((0..suffix_len).map(|i| 10001 + i));
            let mut expected = advice[..8].iter().map(|v| *v as usize).collect::<Vec<_>>();
            expected.extend(&input[8..12]);
            expected.push(dst + 8);
            expected.extend(&input[13..]);
            let initialize = (0..8)
                .map(|i| format!("push.71 mem_store.{}", dst + i))
                .collect::<Vec<_>>()
                .join(" ");
            let checks = advice[..8]
                .iter()
                .enumerate()
                .map(|(i, v)| format!("mem_load.{} push.{v} assert_eq", dst + i))
                .chain(["adv_push push.97 assert_eq".into()])
                .collect::<Vec<_>>()
                .join(" ");
            stream_write_run(&procedure, &input, &expected, &initialize, &checks, &advice).unwrap();
            for replacement in [
                "nop",
                "adv_pipe swap.1",
                "adv_pipe movup.12 add.1 movdn.12",
                "adv_pipe adv_push drop",
            ] {
                let error = stream_write_run(
                    &procedure.replace("adv_pipe", replacement),
                    &input,
                    &expected,
                    &initialize,
                    &checks,
                    &advice,
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("assertion failed"), "{replacement}: {error}");
            }
        }
    }
    // Two selected invocations consume exactly sixteen advice elements and advance twice.
    let mut input =
        (0..12).map(|i| 101 + i).chain([400]).chain([901, 902, 903]).collect::<Vec<_>>();
    let advice = (1..=16).chain([97]).collect::<Vec<u64>>();
    let mut expected = (9..=16).collect::<Vec<usize>>();
    expected.extend(&input[8..12]);
    expected.push(416);
    expected.extend(&input[13..]);
    let checks = (1..=16)
        .enumerate()
        .map(|(i, v)| format!("mem_load.{} push.{v} assert_eq", 400 + i))
        .chain(["adv_push push.97 assert_eq".into()])
        .collect::<Vec<_>>()
        .join(" ");
    let twice = procedure.replace("adv_pipe", "adv_pipe adv_pipe");
    stream_write_run(&twice, &input, &expected, "", &checks, &advice).unwrap();
    input.clear();
}

#[test]
fn stream_write_crypto_selected_field_memory_floor_adjacency_and_mutants() {
    use miden_core::Felt;
    let f = fixture("crypto_stream", 14, 14);
    let (_, frame, _) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.aligned_size(), 0);
    assert!(frame.spills().is_empty());
    let procedure = selected_procedure(&f, &SelectEnv::default());
    for suffix_len in [2, 20] {
        for (src, dst) in [
            (400usize, 408usize),
            (408, 400),
            (400, 800),
            (0xffff_fff8, 800),
            (400, 0xffff_fff8),
        ] {
            for written in [false, true] {
                let mut plain = (0..8)
                    .map(|i| {
                        if written {
                            0xffff_ffff_0000_0000usize - i * 17
                        } else {
                            0
                        }
                    })
                    .collect::<Vec<_>>();
                if src == 0xffff_fff8 {
                    plain[6] = 2147483648;
                }
                let mut input = (0..12)
                    .map(|i| 101 + i * 37)
                    .chain([src, dst])
                    .chain((0..suffix_len).map(|i| 10001 + i))
                    .collect::<Vec<_>>();
                // A terminal destination includes the reserved FMP initializer. Preserve it so
                // the selected procedure's zero-sized frame is safe even at this endpoint.
                if dst == 0xffff_fff8 {
                    input[6] = (Felt::new_unchecked(2147483648)
                        - Felt::new_unchecked(plain[6] as u64))
                    .as_canonical_u64() as usize;
                }
                let cipher = (0..8)
                    .map(|i| {
                        (Felt::new_unchecked(plain[i] as u64)
                            + Felt::new_unchecked(input[i] as u64))
                        .as_canonical_u64() as usize
                    })
                    .collect::<Vec<_>>();
                let mut expected = cipher.clone();
                expected.extend(&input[8..12]);
                expected.extend([src + 8, dst + 8]);
                expected.extend(&input[14..]);
                let initialize = plain
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| written && !(src == 0xffff_fff8 && *i == 6))
                    .map(|(i, v)| format!("push.{v} mem_store.{}", src + i))
                    .collect::<Vec<_>>()
                    .join(" ");
                let checks = cipher
                    .iter()
                    .enumerate()
                    .map(|(i, v)| format!("mem_load.{} push.{v} assert_eq", dst + i))
                    .chain(
                        plain
                            .iter()
                            .enumerate()
                            .map(|(i, v)| format!("mem_load.{} push.{v} assert_eq", src + i)),
                    )
                    .chain(["adv_push push.97 assert_eq".into()])
                    .collect::<Vec<_>>()
                    .join(" ");
                stream_write_run(&procedure, &input, &expected, &initialize, &checks, &[97])
                    .unwrap();
                for replacement in [
                    "nop",
                    "crypto_stream push.1 add",
                    "crypto_stream movup.12 add.1 movdn.12",
                    "crypto_stream movup.13 add.1 movdn.13",
                ] {
                    let error = stream_write_run(
                        &procedure.replace("crypto_stream", replacement),
                        &input,
                        &expected,
                        &initialize,
                        &checks,
                        &[97],
                    )
                    .unwrap_err()
                    .to_string();
                    assert!(error.contains("assertion failed"), "{replacement}: {error}");
                }
            }
        }
    }
    let input = (0..12).map(|i| 101 + i).chain([400, 800]).chain([901, 902]).collect::<Vec<_>>();
    let mut expected = (0..8).map(|i| 101 + i + 2 * (i + 1)).collect::<Vec<_>>();
    expected.extend(&input[8..12]);
    expected.extend([416, 816]);
    expected.extend(&input[14..]);
    let init = (0..16)
        .map(|i| format!("push.{} mem_store.{}", i % 8 + 1, 400 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let checks = (0..16)
        .map(|i| {
            format!(
                "mem_load.{} push.{} assert_eq",
                800 + i,
                101 + i % 8 + (i / 8 + 1) * (i % 8 + 1)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    stream_write_run(
        &procedure.replace("crypto_stream", "crypto_stream crypto_stream"),
        &input,
        &expected,
        &init,
        &checks,
        &[],
    )
    .unwrap();
}

fn stream_write_failure(
    procedure: &str,
    input: &[usize],
    initialize: &str,
    advice: &[u64],
    limit: Option<usize>,
) -> (miden_processor::FastProcessor, miden_processor::ExecutionError) {
    use miden_core::Felt;
    use miden_processor::{ExecutionOptions, FastProcessor, StackInputs, advice::AdviceInputs};
    let code = format!(
        "{procedure} begin {initialize} {} exec.selected end",
        crate::plan::tests::lowering_units::push_top(input)
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("stream_write_failure", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut processor = FastProcessor::new(StackInputs::default())
        .with_advice(
            AdviceInputs::default()
                .with_stack(advice.iter().copied().map(Felt::new_unchecked).collect()),
        )
        .unwrap();
    if let Some(limit) = limit {
        processor = processor
            .with_options(ExecutionOptions::default().with_max_memory_elements(limit))
            .unwrap();
    }
    let mut host = crate::plan::tests::lowering_units::host();
    let mut context = processor.get_initial_resume_context(&program).unwrap();
    loop {
        match processor.step_sync(&mut host, context) {
            Ok(Some(next)) => context = next,
            Ok(None) => panic!("expected failure"),
            Err(error) => return (processor, error),
        }
    }
}

#[test]
fn stream_write_advice_failures_preserve_provider_order_and_partial_writes() {
    use miden_core::Felt;
    let f = fixture("advice_pipe", 13, 13);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    for count in 0..8 {
        // Exhaustion precedes destination validation and consumes no partial advice word.
        let input = (0..12).map(|i| 101 + i).chain([401]).collect::<Vec<_>>();
        let advice = (1..=count).collect::<Vec<u64>>();
        let initialize = (401..409)
            .map(|address| format!("push.{} mem_store.{address}", 9000 + address))
            .collect::<Vec<_>>()
            .join(" ");
        let (processor, error) =
            stream_write_failure(&procedure, &input, &initialize, &advice, None);
        assert!(matches!(error, miden_processor::ExecutionError::AdviceError { .. }), "{error}");
        assert_eq!(
            processor
                .stack_top()
                .iter()
                .rev()
                .take(13)
                .map(|f| f.as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            input
        );
        for address in 401..409 {
            assert_eq!(
                processor
                    .memory()
                    .read_element(Default::default(), Felt::new_unchecked(address))
                    .unwrap(),
                Felt::new_unchecked(9000 + address)
            );
        }
        let (provider, _) = processor.into_parts();
        assert_eq!(
            provider.stack().iter().map(|f| f.as_canonical_u64()).collect::<Vec<_>>(),
            advice
        );
    }
    for (dst, limit, first_written) in [
        (401usize, None, false),
        (1usize << 32, None, false),
        (0xffff_fffc, None, true),
        (400, Some(8), true),
    ] {
        let input = (0..12).map(|i| 101 + i).chain([dst]).collect::<Vec<_>>();
        let advice = (1..=8).chain([97]).collect::<Vec<u64>>();
        let (processor, error) = stream_write_failure(&procedure, &input, "", &advice, limit);
        assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }), "{error}");
        assert_eq!(
            processor
                .stack_top()
                .iter()
                .rev()
                .take(13)
                .map(|f| f.as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            input
        );
        if dst <= u32::MAX as usize {
            for i in 0..4 {
                let value = processor
                    .memory()
                    .read_element(Default::default(), Felt::new_unchecked((dst + i) as u64))
                    .unwrap();
                // The final memory word includes the VM's reserved FMP initializer.
                let before = if dst + i == 4294967294 { 2147483648 } else { 0 };
                assert_eq!(
                    value.as_canonical_u64(),
                    if first_written { i as u64 + 1 } else { before }
                );
            }
        }
        if dst == 400 {
            for i in 4..8 {
                assert_eq!(
                    processor
                        .memory()
                        .read_element(Default::default(), Felt::new_unchecked((dst + i) as u64))
                        .unwrap(),
                    Felt::ZERO
                );
            }
        }
        let (provider, _) = processor.into_parts();
        assert_eq!(provider.stack(), vec![Felt::new_unchecked(97)]);
    }
    let input = (0..12).map(|i| 101 + i).chain([0xffff_fff8]).collect::<Vec<_>>();
    let (processor, error) = stream_write_failure(
        &procedure.replace("adv_pipe", "adv_pipe adv_pipe"),
        &input,
        "",
        &(1..=16).chain([97]).collect::<Vec<_>>(),
        None,
    );
    assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
    assert_eq!(processor.stack_top()[3].as_canonical_u64(), 1u64 << 32);
    assert_eq!(processor.into_parts().0.stack(), vec![Felt::new_unchecked(97)]);
}

#[test]
fn stream_write_crypto_failures_validate_ranges_and_overlap_before_writes() {
    use miden_core::Felt;
    let f = fixture("crypto_stream", 14, 14);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    for (src, dst) in [
        (400usize, 400usize),
        (400, 404),
        (404, 400),
        (401, 401),
        (400, 401),
        (401, 800),
        (400, 801),
        (1usize << 32, 800),
        (400, 1usize << 32),
        (0xffff_fffc, 800),
        (400, 0xffff_fffc),
    ] {
        let input = (0..12).map(|i| 101 + i).chain([src, dst]).collect::<Vec<_>>();
        let (processor, error) = stream_write_failure(&procedure, &input, "", &[97], None);
        assert!(
            matches!(error, miden_processor::ExecutionError::MemoryError { .. }),
            "{src}/{dst}: {error}"
        );
        if src == dst || src.abs_diff(dst) < 8 {
            assert!(
                matches!(
                    error,
                    miden_processor::ExecutionError::MemoryError {
                        err: miden_processor::MemoryError::IllegalMemoryAccess { .. },
                        ..
                    }
                ),
                "range/overlap validation must precede unaligned access: {error}"
            );
        }
        assert_eq!(
            processor
                .stack_top()
                .iter()
                .rev()
                .take(14)
                .map(|f| f.as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            input
        );
        if dst < (1usize << 32) {
            assert_eq!(
                processor
                    .memory()
                    .read_element(Default::default(), Felt::new_unchecked(dst as u64))
                    .unwrap(),
                Felt::ZERO
            );
        }
        assert_eq!(processor.into_parts().0.stack(), vec![Felt::new_unchecked(97)]);
    }
    let input = (0..12).map(|i| 101 + i).chain([400, 800]).collect::<Vec<_>>();
    // The program initializer owns one word; cap two allows only destination word0.
    let (processor, error) = stream_write_failure(&procedure, &input, "", &[97], Some(8));
    assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
    assert_eq!(
        processor
            .stack_top()
            .iter()
            .rev()
            .take(14)
            .map(|f| f.as_canonical_u64() as usize)
            .collect::<Vec<_>>(),
        input
    );
    for (i, lane) in input.iter().take(8).enumerate() {
        assert_eq!(
            processor
                .memory()
                .read_element(Default::default(), Felt::new_unchecked(800 + i as u64))
                .unwrap()
                .as_canonical_u64(),
            if i < 4 { *lane as u64 } else { 0 }
        );
    }
    assert_eq!(processor.into_parts().0.stack(), vec![Felt::new_unchecked(97)]);
}

#[test]
fn stream_write_selected_spill_and_normalization_preserves_all_live_values() {
    for (name, n) in [("advice_pipe", 13), ("crypto_stream", 14)] {
        let args = (0..20)
            .map(|i| format!("%s{i}: felt"))
            .chain((0..n).map(|i| format!("%a{i}: felt")))
            .collect::<Vec<_>>()
            .join(",");
        let operands = (0..n).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(",");
        let results = (0..n).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(",");
        let returned = (0..20)
            .map(|i| format!("%s{i}"))
            .chain((0..n).map(|i| format!("%r{i}")))
            .collect::<Vec<_>>()
            .join(",");
        let types = vec!["felt"; 20 + n].join(",");
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @stream_spill({args}) -> ({types}) {{ \
             {results}=hir.{name} {operands}; builtin.ret {returned} : ({types}); }};"
        ));
        let (_, frame, _) = ready(&f, &SelectEnv::default());
        assert!(!frame.spills().is_empty());
        let procedure = selected_procedure(&f, &SelectEnv::default());
        let mut input =
            (0..20).map(|i| 10001 + i).chain((0..12).map(|i| 101 + i)).collect::<Vec<_>>();
        if n == 14 {
            input.extend([400, 800]);
        } else {
            input.push(800);
        }
        let mut expected = input[..20].to_vec();
        expected.extend(if n == 14 {
            (0..8).map(|i| 101 + i).collect::<Vec<_>>()
        } else {
            (1..=8).collect()
        });
        expected.extend(&input[28..32]);
        if n == 14 {
            expected.extend([408, 808]);
        } else {
            expected.push(808);
        }
        let checks = (0..8)
            .map(|i| format!("mem_load.{} push.{} assert_eq", 800 + i, expected[20 + i]))
            .chain(["adv_push push.97 assert_eq".into()])
            .collect::<Vec<_>>()
            .join(" ");
        let advice = if n == 14 {
            vec![97]
        } else {
            (1..=8).chain([97]).collect::<Vec<_>>()
        };
        // Keep a real suffix under all arguments: spill/normalization may reach the VM floor
        // without it and introduce padding, so equal physical depth would be the wrong oracle.
        input.extend((0..20).map(|i| 20001 + i));
        expected.extend((0..20).map(|i| 20001 + i));
        stream_write_run(&procedure, &input, &expected, "", &checks, &advice).unwrap();
    }
}

#[test]
fn eval_circuit_exact_source_selection_and_aliases() {
    let f = fixture("eval_circuit", 3, 3);
    let selected = checked(&f, &SelectEnv::default());
    let unit = &selected.segments[0].units[0];
    assert!(unit.effects.memory.read);
    assert!(!unit.effects.memory.write);
    assert!(unit.effects.may_trap);
    let evidence = crate::select::evidence(&f.borrow(), &SelectEnv::default()).unwrap();
    assert_eq!(evidence.source.units[&unit.id].aliases.pairs().len(), 3);
    for (output, input) in unit.outputs.iter().zip(&unit.stack_inputs) {
        assert!(evidence.source.units[&unit.id].aliases.pairs().contains(&(*output, *input)));
    }
    ready(&f, &SelectEnv::default());
}

fn eval_circuit_gate(left: u64, right: u64, op: u64) -> usize {
    (left + (right << 30) + (op << 60)) as usize
}

/// Independent circuit: wire3=x-x, wire2=x+y, wire1=wire2*wire2, wire0=wire1-wire1.
/// Distinct nonzero extension inputs ensure replacing the final subtraction by addition fails.
fn eval_circuit_cells() -> Vec<usize> {
    vec![
        3,
        7,
        11,
        13,
        eval_circuit_gate(5, 5, 0),
        eval_circuit_gate(5, 4, 2),
        eval_circuit_gate(2, 2, 1),
        eval_circuit_gate(1, 1, 0),
    ]
}

fn eval_circuit_initialize(base: usize, cells: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(i, value)| format!("push.{value} mem_store.{}", base + i))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn eval_circuit_selected_vm_independent_oracle_retained_lanes_suffix_floor_and_mutants() {
    let f = fixture("eval_circuit", 3, 3);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let cells = eval_circuit_cells();
    let initialize = eval_circuit_initialize(400, &cells);
    let checks = cells
        .iter()
        .enumerate()
        .map(|(i, value)| format!("mem_load.{} push.{value} assert_eq", 400 + i))
        .collect::<Vec<_>>()
        .join(" ");
    for suffix in [13, 20] {
        let input = [400, 2, 4]
            .into_iter()
            .chain((0..suffix).map(|i| 901 + i * 17))
            .collect::<Vec<_>>();
        stream_write_run(&procedure, &input, &input, &initialize, &checks, &[]).unwrap();
    }
    for (lane, gate, message) in [
        (7, eval_circuit_gate(1, 1, 2), "does not evaluate to zero"),
        (4, eval_circuit_gate(5, 5, 3), "decode"),
        (4, eval_circuit_gate(0, 5, 0), "wiring bus"),
    ] {
        let mut bad = cells.clone();
        bad[lane] = gate;
        let init = eval_circuit_initialize(400, &bad);
        let (processor, error) = stream_write_failure(&procedure, &[400, 2, 4], &init, &[], None);
        let miden_processor::ExecutionError::AceChipError {
            error: miden_processor::AceError(error),
            ..
        } = error
        else {
            panic!("expected circuit failure: {error}");
        };
        assert!(error.contains(message), "{error}");
        assert_eq!(
            processor
                .stack_top()
                .iter()
                .rev()
                .take(3)
                .map(|v| v.as_canonical_u64())
                .collect::<Vec<_>>(),
            [400, 2, 4]
        );
        // Omitting the native assertion remains physically legal under USA044. The actual
        // invalid-circuit execution is the oracle: the mutant incorrectly completes.
        let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
        for op in body.ops_mut() {
            if let AnnotatedOp::Instruction { instruction, .. } = op
                && matches!(instruction, crate::masm::Instruction::EvalCircuit)
            {
                *instruction = crate::masm::Instruction::Nop;
            }
        }
        let mutant = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
            .unwrap()
            .to_pretty_string();
        let input = [400, 2, 4].into_iter().chain((0..13).map(|i| 900 + i)).collect::<Vec<_>>();
        stream_write_run(&mutant, &input, &input, &init, "", &[]).unwrap();
    }
}

#[test]
fn eval_circuit_selected_vm_count_guard_order_and_address_failures() {
    let f = fixture("eval_circuit", 3, 3);
    let (_, frame, _) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.aligned_size(), 0);
    // The terminal EVAL fixture writes the reserved FMP cell as operand data. This selected
    // procedure owns no frame and never accesses FMP, so its value cannot redirect a local access.
    let procedure = selected_procedure(&fixture("eval_circuit", 3, 3), &SelectEnv::default());
    for (ptr, read, eval, message) in [
        (1, 32766, 4, "num of wires cannot exceed 32768 but was 32770"),
        (1usize << 32, 2, 32768, "num of wires cannot exceed 32768 but was 32770"),
        (1, 0xffff_ffff_0000_0000, 0x1_0000_0004, "num of wires cannot exceed"),
        (1, 0, 0, "num of variables"),
        (1, 1, 3, "num of variables"),
        (1, 2, 0, "num of evaluation gates"),
        (1, 2, 3, "num of evaluation gates"),
    ] {
        let (_, error) = stream_write_failure(&procedure, &[ptr, read, eval], "", &[], None);
        let miden_processor::ExecutionError::AceChipError {
            error: miden_processor::AceError(error),
            ..
        } = error
        else {
            panic!("count guard must precede memory: {error}");
        };
        assert!(error.contains(message), "{error}");
    }
    for (ptr, read, eval, init) in [
        (401, 2, 4, String::new()),
        (1usize << 32, 2, 4, String::new()),
        // A valid first READ followed by an out-of-u32 EVAL address.
        (0xffff_fffc, 2, 4, String::new()),
        // The first EVAL succeeds before the second scalar address exceeds u32.
        (
            0xffff_fff8,
            2,
            8,
            format!(
                "push.{} mem_store.4294967292 push.{} mem_store.4294967293 push.{} \
                 mem_store.4294967294 push.{} mem_store.4294967295",
                eval_circuit_gate(9, 9, 0),
                eval_circuit_gate(9, 9, 0),
                eval_circuit_gate(9, 9, 0),
                eval_circuit_gate(9, 9, 0)
            ),
        ),
        (0xffff_fffc, 4, 4, String::new()),
    ] {
        let (_, error) = stream_write_failure(&procedure, &[ptr, read, eval], &init, &[], None);
        match error {
            miden_processor::ExecutionError::MemoryError {
                err: miden_processor::MemoryError::UnalignedWordAccess { addr, .. },
                ..
            } if ptr == 401 => assert_eq!(addr, 401),
            miden_processor::ExecutionError::MemoryError {
                err: miden_processor::MemoryError::AddressOutOfBounds { addr },
                ..
            } if ptr != 401 => assert_eq!(addr, 1u64 << 32),
            error => panic!("unexpected terminal memory failure for {ptr}/{read}/{eval}: {error}"),
        }
    }
}

#[test]
fn eval_circuit_selected_vm_wide_footprint_wire_limit_and_traced_invocation_limit() {
    use miden_processor::{AceError, ExecutionError, FastProcessor, StackInputs};
    let procedure = selected_procedure(&fixture("eval_circuit", 3, 3), &SelectEnv::default());
    for read in [128, 32764] {
        let wire = read + 3;
        let cells = vec![eval_circuit_gate(wire, wire, 0); 4];
        let init = eval_circuit_initialize(400 + 2 * read as usize, &cells);
        let input = [400, read as usize, 4]
            .into_iter()
            .chain((0..13).map(|i| 901 + i))
            .collect::<Vec<_>>();
        stream_write_run(&procedure, &input, &input, &init, "", &[]).unwrap();
    }
    let init = eval_circuit_initialize(400, &eval_circuit_cells());
    for count in [32, 33] {
        let source = format!(
            "{procedure} begin {init} repeat.{count} push.4.2.400 exec.selected drop drop drop \
             end end"
        );
        let program = miden_assembly::Assembler::default()
            .assemble_program("circuit_limit", source)
            .unwrap()
            .try_into_program()
            .unwrap();
        FastProcessor::new(StackInputs::default())
            .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
            .unwrap();
        let traced = FastProcessor::new(StackInputs::default())
            .execute_for_proving_sync(&program, &mut crate::plan::tests::lowering_units::host());
        if count == 32 {
            traced.unwrap();
        } else {
            let ExecutionError::AceChipError {
                error: AceError(message),
                ..
            } = traced.unwrap_err()
            else {
                panic!("expected traced invocation limit");
            };
            assert_eq!(message, "number of recorded eval_circuit invocations cannot exceed 32");
        }
    }
}

#[test]
fn eval_circuit_selected_source_local_dynamic_counts_execute_without_worst_case_reservation() {
    use midenc_hir::Type;
    let args = (0..8).map(|i| format!("%w{i}: felt")).collect::<Vec<_>>().join(", ");
    let stores = (0..8)
        .map(|i| {
            format!(
                "hir.store_local %w{i} <{{ local = #builtin.local_variable<{i},felt> }}> : (felt);"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!(
        "builtin.function public extern(\"C\") @circuit_local(%r: felt, %e: felt, {args}) -> \
         (felt, felt) {{ {stores} %ptr = hir.local_address <{{ local = \
         #builtin.local_variable<0,felt> }}>; %bits = hir.ptr_to_int %ptr <{{ ty = \
         #builtin.type<u32> }}>; %address = hir.bitcast %bits <{{ ty = #builtin.type<felt> }}>; \
         %p, %rr, %ee = hir.eval_circuit %address, %r, %e; builtin.ret %rr, %ee : (felt, felt); \
         }};"
    );
    let mut f = parse(&source);
    for _ in 0..8 {
        f.function.borrow_mut().alloc_local(Type::Felt);
    }
    let (_, frame, _) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.source().extent, 8);
    let input = [2, 4].into_iter().chain(eval_circuit_cells()).collect::<Vec<_>>();
    execute_linked(&f, &input, &[2, 4]);
}

#[test]
fn eval_circuit_source_operand_arity_is_checked_without_inference_or_panics() {
    for mutation in 0..4 {
        let f = fixture("eval_circuit", 3, 3);
        let mut op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        if mutation < 3 {
            op.borrow_mut().operands_mut().group_mut(0).erase(mutation);
        } else {
            let value = op.borrow().operands().all()[0].borrow().as_value_ref();
            let operand = f.context.make_operand(value, op, 0);
            op.borrow_mut().operands_mut().push(operand);
        }
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

pub(super) fn system_event_cases() -> Vec<(miden_core::events::SystemEvent, usize)> {
    use miden_core::events::SystemEvent::*;
    vec![
        (MapValueToStack, 4),
        (MapValueCountToStack, 4),
        (MapValueToStackN0, 4),
        (MapValueToStackN4, 4),
        (MapValueToStackN8, 4),
        (HasMapKey, 4),
        (MerkleNodeToStack, 6),
        (HdwordToMap, 8),
        (MerkleNodeMerge, 8),
        (HdwordToMapWithDomain, 9),
        (HqwordToMap, 16),
        (HpermToMap, 12),
        (DeferredRegister, 12),
        (DeferredEvaluate, 4),
        (DeferredEvaluateTag, 4),
        (DeferredEvaluatePayload, 4),
        (Ext2Inv, 2),
        (U32Clz, 1),
        (U32Ctz, 1),
        (U32Clo, 1),
        (U32Cto, 1),
        (ILog2, 1),
        (MemToMap, 6),
        (DeferredRegisterData, 6),
    ]
}

pub(super) fn system_event_fixture(event: miden_core::events::SystemEvent, n: usize) -> Parsed {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    let mut test =
        midenc_hir::testing::Test::new("system_event", &vec![Type::Felt; n], &vec![Type::Felt; n]);
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
        let mut b = test.function_builder();
        let results =
            b.system_event(args, event.event_id().as_felt(), SourceSpan::UNKNOWN).unwrap();
        b.ret(results, SourceSpan::UNKNOWN).unwrap();
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn memory_system_event_source_selection_preserves_all_six_aliases_and_reads() {
    use miden_core::events::SystemEvent::{DeferredRegisterData, MemToMap};
    assert_eq!(system_event_cases().len(), 24);
    for event in [MemToMap, DeferredRegisterData] {
        let f = system_event_fixture(event, 6);
        let selected = checked(&f, &SelectEnv::default());
        let unit = &selected.segments[0].units[0];
        assert_eq!(unit.effects.memory, SourceMemoryEffects::READ);
        assert_eq!(unit.outputs.len(), 6);
        let evidence = crate::select::evidence(&f.borrow(), &SelectEnv::default()).unwrap();
        assert_eq!(evidence.source.units[&unit.id].aliases.pairs().len(), 6);
        ready(&f, &SelectEnv::default());
    }
}

#[test]
fn system_event_exact_source_selection_and_aliases() {
    for (event, n) in system_event_cases() {
        let f = system_event_fixture(event, n);
        let selected = checked(&f, &SelectEnv::default());
        let unit = &selected.segments[0].units[0];
        assert_eq!(unit.stack_inputs.len(), n, "{event:?}");
        assert_eq!(unit.outputs.len(), n, "{event:?}");
        assert_eq!(
            unit.effects.memory,
            if matches!(
                event,
                miden_core::events::SystemEvent::MemToMap
                    | miden_core::events::SystemEvent::DeferredRegisterData
            ) {
                SourceMemoryEffects::READ
            } else {
                SourceMemoryEffects::default()
            }
        );
        assert!(unit.effects.may_trap);
        assert!(unit.scratch.is_empty());
        let evidence = crate::select::evidence(&f.borrow(), &SelectEnv::default()).unwrap();
        assert_eq!(evidence.source.units[&unit.id].aliases.pairs().len(), n);
        for (output, input) in unit.outputs.iter().zip(&unit.stack_inputs) {
            assert!(evidence.source.units[&unit.id].aliases.pairs().contains(&(*output, *input)));
        }
        ready(&f, &SelectEnv::default());
    }
}

#[test]
fn system_event_malformed_source_tuples_and_coverage_reject_without_inference() {
    use midenc_hir::{SourceSpan, Type, Value};
    for (event, n) in system_event_cases() {
        for mutation in 0..6 {
            let f = system_event_fixture(event, n);
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
                    op.borrow_mut().results_mut().group_mut(0).pop().unwrap();
                }
                2 => {
                    let r = f.context.make_result(SourceSpan::UNKNOWN, Type::Felt, op, n as u16);
                    op.borrow_mut().results_mut().push(r);
                }
                3 => {
                    let mut a = op.borrow().operands().all()[0].borrow().as_value_ref();
                    a.borrow_mut().set_type(Type::U32);
                }
                4 => {
                    op.borrow_mut().operands_mut().group_mut(0).erase(0);
                }
                _ => {
                    let value = op.borrow().operands().all()[0].borrow().as_value_ref();
                    let operand = f.context.make_operand(value, op, 0);
                    op.borrow_mut().operands_mut().push(operand);
                }
            }
            assert!(
                select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                "{event:?} mutation{mutation}"
            );
        }
        let f = system_event_fixture(event, n);
        let function = f.borrow();
        let env = SelectEnv::default();
        let selected = select_function(&function, &env).unwrap();
        for duplicate in [false, true] {
            let mut bad = selected.clone();
            let unit = bad.segments[0].units[0].clone();
            if duplicate {
                bad.segments[0].units.push(unit);
            } else {
                bad.segments[0].units.clear();
            }
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
    }
}

struct SystemEventProcedure {
    text: String,
    /// Forced normalization drains the complete fixture argument tuple before reloading.
    drained: usize,
}

fn system_event_procedure(
    f: &Parsed,
    force: bool,
    mutate: impl FnOnce(&mut AnnotatedMasmBody),
) -> SystemEventProcedure {
    let (plan, frame, mut body) = fri_ready(f, force);
    mutate(&mut body);
    let text = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    let drained = if force {
        f.borrow().entry_block().borrow().arguments().len()
    } else {
        0
    };
    SystemEventProcedure { text, drained }
}

/// Real provider state survives both normal completion and handler errors.
fn system_event_vm(
    procedure: &SystemEventProcedure,
    input: &[usize],
    advice: miden_processor::advice::AdviceInputs,
    budget: Option<usize>,
    setup: &str,
) -> (miden_processor::FastProcessor, Result<(), miden_processor::ExecutionError>) {
    system_event_vm_outputs(procedure, input, input, advice, budget, setup)
}

fn system_event_vm_outputs(
    procedure: &SystemEventProcedure,
    input: &[usize],
    expected: &[usize],
    advice: miden_processor::advice::AdviceInputs,
    budget: Option<usize>,
    setup: &str,
) -> (miden_processor::FastProcessor, Result<(), miden_processor::ExecutionError>) {
    use miden_core::Felt;
    use miden_processor::{ExecutionOptions, FastProcessor, StackInputs};

    use crate::plan::tests::lowering_units::push_top;
    assert!(input.len() >= 16);
    // A floor pop appends a zero when normalization drains below sixteen physical lanes.
    let padding = procedure.drained.saturating_sub(input.len() - 16);
    let procedure = &procedure.text;
    let checks = expected
        .iter()
        .enumerate()
        .map(|(i, v)| format!("push.{v} assert_eq.err=\"retained lane{i}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let padding_checks = "assertz ".repeat(padding);
    let code = format!(
        "{procedure} begin {setup} {} sdepth push.{} assert_eq.err=\"input depth\" exec.selected \
         sdepth push.{} assert_eq.err=\"output depth\" {checks} {padding_checks} end",
        push_top(&input[..input.len() - 16]),
        input.len(),
        expected.len() + padding
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("system_event", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let base = input[input.len() - 16..]
        .iter()
        .map(|v| Felt::new_unchecked(*v as u64))
        .collect::<Vec<_>>();
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
            Err(error) => break Err(error),
        }
    };
    (processor, result)
}

fn system_event_input(payload: &[usize], suffix: usize) -> Vec<usize> {
    payload.iter().copied().chain((0..suffix).map(|i| 20001 + i)).collect()
}

fn system_event_advice() -> miden_processor::advice::AdviceInputs {
    miden_processor::advice::AdviceInputs::default()
        .with_stack(vec![miden_core::Felt::from(97u32)].into())
}

#[test]
fn system_event_selected_map_queries_exact_values_count_padding_presence_and_errors() {
    use miden_core::{Felt, Word, events::SystemEvent::*};
    let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
    let payload = [11, 23, 37, 53];
    for event in [
        MapValueToStack,
        MapValueCountToStack,
        MapValueToStackN0,
        MapValueToStackN4,
        MapValueToStackN8,
        HasMapKey,
    ] {
        let f = system_event_fixture(event, 4);
        for force in [false, true] {
            let procedure = system_event_procedure(&f, force, |_| {});
            for count in [0usize, 1, 4, 5, 8, 9] {
                let values =
                    (0..count).map(|i| Felt::from(101 + i as u32 * 17)).collect::<Vec<_>>();
                for suffix in [12, 20] {
                    let input = system_event_input(&payload, suffix);
                    let advice = system_event_advice().with_map([(key, values.clone())]);
                    let (processor, result) = system_event_vm(&procedure, &input, advice, None, "");
                    result.unwrap_or_else(|error| {
                        panic!("{event:?} count{count} force{force} suffix{suffix}: {error}")
                    });
                    let (provider, _) = processor.into_parts();
                    let mut expected = match event {
                        HasMapKey => vec![Felt::ONE],
                        MapValueCountToStack => vec![Felt::from(count as u32)],
                        _ => {
                            let mut result = values.clone();
                            let alignment = match event {
                                MapValueToStackN4 => 4,
                                MapValueToStackN8 => 8,
                                _ => 1,
                            };
                            result.resize(count.next_multiple_of(alignment), Felt::ZERO);
                            if event != MapValueToStack {
                                result.insert(0, Felt::from(count as u32));
                            }
                            result
                        }
                    };
                    expected.push(Felt::from(97u32));
                    assert_eq!(provider.stack(), expected, "{event:?} count{count} force{force}");
                    assert_eq!(provider.get_mapped_values(&key), Some(values.as_slice()));
                }
            }
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&payload, 20),
                system_event_advice(),
                None,
                "",
            );
            if event == HasMapKey {
                result.unwrap();
                assert_eq!(processor.into_parts().0.stack(), [Felt::ZERO, Felt::from(97u32)]);
            } else {
                let error = result.unwrap_err();
                assert!(
                    matches!(error, miden_processor::ExecutionError::AdviceError { err: miden_processor::advice::AdviceError::MapKeyNotFound { key: k }, .. } if k == key),
                    "{error}"
                );
                assert_eq!(processor.into_parts().0.stack(), [Felt::from(97u32)]);
            }
        }
    }
}

#[test]
fn system_event_selected_map_insertions_observe_all_lanes_domain_rate_and_overflow() {
    use miden_core::{Felt, Word, crypto::hash::Poseidon2, events::SystemEvent::*};
    for (event, n) in [
        (HdwordToMap, 8),
        (HdwordToMapWithDomain, 9),
        (HqwordToMap, 16),
        (HpermToMap, 12),
    ] {
        let payload = (0..n).map(|i| 101 + i * 37).collect::<Vec<_>>();
        let felts = payload.iter().map(|v| Felt::new_unchecked(*v as u64)).collect::<Vec<_>>();
        let (key, values) = match event {
            HdwordToMap | HdwordToMapWithDomain => (
                Poseidon2::merge_in_domain(
                    &[
                        Word::new(felts[..4].try_into().unwrap()),
                        Word::new(felts[4..8].try_into().unwrap()),
                    ],
                    if n == 9 { felts[8] } else { Felt::ZERO },
                ),
                felts[..8].to_vec(),
            ),
            HqwordToMap => (Poseidon2::hash_elements(&felts), felts.clone()),
            _ => {
                let mut state: [Felt; 12] = felts.clone().try_into().unwrap();
                Poseidon2::apply_permutation(&mut state);
                (
                    Word::new(state[Poseidon2::DIGEST_RANGE].try_into().unwrap()),
                    felts[..8].to_vec(),
                )
            }
        };
        let f = system_event_fixture(event, n);
        for force in [false, true] {
            let procedure = system_event_procedure(&f, force, |_| {});
            for suffix in [16 - n, 20] {
                let input = system_event_input(&payload, suffix);
                let (processor, result) =
                    system_event_vm(&procedure, &input, system_event_advice(), None, "");
                result.unwrap();
                let (provider, _) = processor.into_parts();
                assert_eq!(
                    provider.get_mapped_values(&key),
                    Some(values.as_slice()),
                    "{event:?} force{force} suffix{suffix}"
                );
                assert_eq!(provider.stack(), [Felt::from(97u32)]);
            }
        }
    }
}

#[test]
fn system_event_ids_require_recognized_felt_and_generic_events_select() {
    use miden_core::{Felt, events::SystemEvent::*};
    use midenc_hir::Immediate;
    for id in [
        Immediate::U32(7),
        Immediate::Felt(Felt::from(7u32)),
        Immediate::Felt(MemToMap.event_id().as_felt()),
        Immediate::Felt(DeferredRegisterData.event_id().as_felt()),
        Immediate::Felt(TraceEvent.event_id().as_felt()),
    ] {
        let f = system_event_fixture(HasMapKey, 4);
        let mut op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        *op.borrow_mut()
            .downcast_mut::<midenc_dialect_hir::SystemEvent>()
            .unwrap()
            .get_event_id_mut() = id;
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err(), "{id:?}");
    }
    // Generic source families also select a known system ID without interpreting it.
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    for family in 0..2 {
        let args = if family == 0 {
            vec![Type::Felt]
        } else {
            vec![]
        };
        let mut test = midenc_hir::testing::Test::new("unsupported_event", &args, &[]);
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
            let mut b = test.function_builder();
            match family {
                0 => {
                    b.emit_event(args[0], SourceSpan::UNKNOWN).unwrap();
                }
                _ => {
                    b.emit_event_imm(HasMapKey.event_id().as_felt(), SourceSpan::UNKNOWN).unwrap();
                }
            }
            b.ret([], SourceSpan::UNKNOWN).unwrap();
        }
        assert!(select_function(&test.function().borrow(), &SelectEnv::default()).is_ok());
    }
}

#[test]
fn system_event_selected_numeric_advice_edges_and_exact_errors() {
    use miden_core::{
        Felt,
        events::SystemEvent::*,
        field::{BasedVectorSpace, Field, PrimeCharacteristicRing, QuadFelt},
    };
    for event in [U32Clz, U32Ctz, U32Clo, U32Cto, ILog2] {
        let f = system_event_fixture(event, 1);
        for force in [false, true] {
            let procedure = system_event_procedure(&f, force, |_| {});
            for value in [
                0u64,
                1,
                2,
                3,
                4,
                7,
                8,
                0x8000_0000,
                0xffff_ffff,
                1u64 << 32,
                0xffff_ffff_0000_0000,
            ] {
                let input = system_event_input(&[value as usize], 15);
                let (processor, result) =
                    system_event_vm(&procedure, &input, system_event_advice(), None, "");
                if event == ILog2 && value == 0 {
                    assert!(matches!(
                        result.unwrap_err(),
                        miden_processor::ExecutionError::OperationError {
                            err: miden_processor::operation::OperationError::LogArgumentZero,
                            ..
                        }
                    ));
                } else if event != ILog2 && value > u32::MAX as u64 {
                    assert!(
                        matches!(result.unwrap_err(), miden_processor::ExecutionError::OperationError { err: miden_processor::operation::OperationError::NotU32Values { values }, .. } if values == [Felt::new_unchecked(value)])
                    );
                } else {
                    result.unwrap();
                    let want = match event {
                        U32Clz => (value as u32).leading_zeros(),
                        U32Ctz => (value as u32).trailing_zeros(),
                        U32Clo => (value as u32).leading_ones(),
                        U32Cto => (value as u32).trailing_ones(),
                        _ => value.ilog2(),
                    };
                    assert_eq!(
                        processor.into_parts().0.stack(),
                        [Felt::from(want), Felt::from(97u32)],
                        "{event:?} value{value}"
                    );
                }
            }
        }
    }
    let f = system_event_fixture(Ext2Inv, 2);
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, |_| {});
        for coefficients in [[3u32, 5], [1, 0], [0, 1], [0, 0]] {
            let input = system_event_input(&coefficients.map(|v| v as usize), 20);
            let (processor, result) =
                system_event_vm(&procedure, &input, system_event_advice(), None, "");
            if coefficients == [0, 0] {
                assert!(matches!(
                    result.unwrap_err(),
                    miden_processor::ExecutionError::OperationError {
                        err: miden_processor::operation::OperationError::DivideByZero,
                        ..
                    }
                ));
            } else {
                result.unwrap();
                let value = QuadFelt::from_basis_coefficients_fn(|i| Felt::from(coefficients[i]));
                let inverse = value.inverse();
                assert_eq!(inverse * value, QuadFelt::ONE);
                let c = inverse.as_basis_coefficients_slice();
                assert_eq!(processor.into_parts().0.stack(), [c[1], c[0], Felt::from(97u32)]);
            }
        }
    }
}

#[test]
fn system_event_selected_merkle_node_and_merge_preserve_exact_store_and_errors() {
    use miden_core::{
        Felt,
        crypto::{
            hash::Poseidon2,
            merkle::{MerkleStore, MerkleTree, NodeIndex},
        },
        events::SystemEvent::*,
    };
    let leaves = (0..8).map(|i| merkle_word(100 + i * 50)).collect::<Vec<_>>();
    let tree = MerkleTree::new(leaves).unwrap();
    let store = MerkleStore::from(&tree);
    let f = system_event_fixture(MerkleNodeToStack, 6);
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, |_| {});
        for (depth, index) in [(0, 0), (1, 0), (1, 1), (3, 7)] {
            let payload =
                [depth, index].into_iter().chain(merkle_lanes(tree.root())).collect::<Vec<_>>();
            let expected = store
                .get_node(tree.root(), NodeIndex::new(depth as u8, index as u64).unwrap())
                .unwrap();
            let advice = system_event_advice().with_merkle_store(store.clone());
            let (processor, result) =
                system_event_vm(&procedure, &system_event_input(&payload, 20), advice, None, "");
            result.unwrap();
            let (provider, _) = processor.into_parts();
            assert_eq!(
                provider.stack(),
                expected
                    .as_elements()
                    .iter()
                    .copied()
                    .chain([Felt::from(97u32)])
                    .collect::<Vec<_>>()
            );
            assert!(provider.has_merkle_root(tree.root()));
        }
        for (depth, index, root) in
            [(256, 0, tree.root()), (2, 4, tree.root()), (2, 0, merkle_word(9999))]
        {
            let payload = [depth, index].into_iter().chain(merkle_lanes(root)).collect::<Vec<_>>();
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&payload, 20),
                system_event_advice().with_merkle_store(store.clone()),
                None,
                "",
            );
            let error = result.unwrap_err();
            match depth {
                256 => assert!(matches!(
                    error,
                    miden_processor::ExecutionError::AdviceError {
                        err: miden_processor::advice::AdviceError::InvalidMerkleTreeNodeIndex { .. },
                        ..
                    }
                )),
                _ if index == 4 => assert!(matches!(
                    error,
                    miden_processor::ExecutionError::AdviceError {
                        err: miden_processor::advice::AdviceError::InvalidMerkleTreeNodeIndex { .. },
                        ..
                    }
                )),
                _ => assert!(matches!(
                    error,
                    miden_processor::ExecutionError::AdviceError {
                        err: miden_processor::advice::AdviceError::MerkleStoreLookupFailed(_),
                        ..
                    }
                )),
            }
            assert_eq!(processor.into_parts().0.stack(), [Felt::from(97u32)]);
        }
    }
    let left = merkle_word(101);
    let right = merkle_word(701);
    let root = Poseidon2::merge(&[left, right]);
    let payload = merkle_lanes(left).into_iter().chain(merkle_lanes(right)).collect::<Vec<_>>();
    let f = system_event_fixture(MerkleNodeMerge, 8);
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, |_| {});
        let (processor, result) = system_event_vm(
            &procedure,
            &system_event_input(&payload, 20),
            system_event_advice(),
            None,
            "",
        );
        result.unwrap();
        let (provider, _) = processor.into_parts();
        assert!(provider.has_merkle_root(root));
        assert_eq!(provider.get_tree_node(root, Felt::ONE, Felt::ZERO).unwrap(), left);
        assert_eq!(provider.get_tree_node(root, Felt::ONE, Felt::ONE).unwrap(), right);
        assert_eq!(provider.stack(), [Felt::from(97u32)]);
    }
}

fn system_event_register_setup(node: &miden_core::deferred::Node) -> String {
    use miden_core::events::SystemEvent::DeferredRegister;
    let payload = node.payload().as_chunks()[0]
        .iter()
        .chain(node.tag().as_word().iter())
        .map(|v| v.as_canonical_u64() as usize)
        .collect::<Vec<_>>();
    let instruction = crate::masm::Instruction::EmitImm(
        crate::masm::Immediate::Value(crate::masm::Span::unknown(
            DeferredRegister.event_id().as_felt(),
        ))
        .into(),
    );
    format!(
        "{} {instruction} repeat.12 drop end",
        crate::plan::tests::lowering_units::push_top(&payload)
    )
}

#[test]
fn system_event_selected_deferred_registration_canonical_evaluations_order_and_true_payload() {
    use miden_core::{
        Felt,
        deferred::{Node, TRUE_DIGEST, Tag, precompile_id},
        events::SystemEvent::*,
    };
    let data = [11u32, 23, 37, 53, 71, 89, 107, 127].map(Felt::from);
    let node = Node::chunks(vec![data]).unwrap();
    let registration = data
        .iter()
        .chain(Tag::CHUNKS.as_word().iter())
        .map(|v| v.as_canonical_u64() as usize)
        .collect::<Vec<_>>();
    let f = system_event_fixture(DeferredRegister, 12);
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, |_| {});
        for repeated in [false, true] {
            let setup = if repeated {
                system_event_register_setup(&node)
            } else {
                String::new()
            };
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&registration, 20),
                system_event_advice(),
                None,
                &setup,
            );
            result.unwrap();
            assert_eq!(processor.deferred_state().get_node(&node.digest()), Some(&node));
            assert_eq!(
                processor.deferred_state().get_canonical_digest(node.digest()),
                Some(node.digest())
            );
            assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
            assert_eq!(processor.into_parts().0.stack(), [Felt::from(97u32)]);
        }
    }
    // Pinned uint256 VALUE uses op0/domain bound pointer1; EQ (op4) canonicalizes
    // equal registered values to TRUE. Framework AND registration remains USA048.
    let id = precompile_id("uint256");
    let value_tag = Tag::precompile(id, [Felt::ZERO, Felt::ONE, Felt::ZERO]).unwrap();
    let value = Node::value(value_tag, data).unwrap();
    let eq_tag = Tag::precompile(id, [Felt::from(4u32), Felt::ZERO, Felt::ZERO]).unwrap();
    let predicate = Node::join(eq_tag, value.digest(), value.digest()).unwrap();
    let initial_elements = processor_with_advice(&[]).deferred_state().num_elements();
    for (evaluated, setup, added_elements) in [
        (&node, system_event_register_setup(&node), 12),
        (
            &predicate,
            format!(
                "{} {}",
                system_event_register_setup(&value),
                system_event_register_setup(&predicate)
            ),
            24,
        ),
        (&Node::TRUE, String::new(), 0),
    ] {
        for event in [DeferredEvaluate, DeferredEvaluateTag, DeferredEvaluatePayload] {
            let f = system_event_fixture(event, 4);
            for force in [false, true] {
                let procedure = system_event_procedure(&f, force, |_| {});
                let payload = merkle_lanes(evaluated.digest());
                let (processor, result) = system_event_vm(
                    &procedure,
                    &system_event_input(&payload, 20),
                    system_event_advice(),
                    None,
                    &setup,
                );
                result.unwrap_or_else(|error| {
                    panic!("event{event:?} force{force} tag{:?}: {error:?}", evaluated.tag())
                });
                let canonical = if evaluated == &node {
                    &node
                } else {
                    &Node::TRUE
                };
                assert_eq!(
                    processor.deferred_state().get_canonical_digest(evaluated.digest()),
                    Some(canonical.digest())
                );
                assert_eq!(
                    processor.deferred_state().get_node(&evaluated.digest()),
                    Some(evaluated)
                );
                assert_eq!(
                    processor.deferred_state().num_elements(),
                    initial_elements + added_elements
                );
                assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
                assert_eq!(processor.deferred_state().get_node(&TRUE_DIGEST), Some(&Node::TRUE));
                if evaluated == &predicate {
                    assert_eq!(processor.deferred_state().get_node(&value.digest()), Some(&value));
                    assert_eq!(
                        processor.deferred_state().get_canonical_digest(value.digest()),
                        Some(value.digest())
                    );
                }
                let mut want = vec![];
                if event != DeferredEvaluatePayload {
                    want.extend(canonical.tag().as_word());
                }
                if event != DeferredEvaluateTag && canonical != &Node::TRUE {
                    want.extend(data[4..].iter().chain(&data[..4]).copied());
                }
                want.push(Felt::from(97u32));
                assert_eq!(
                    processor.into_parts().0.stack(),
                    want,
                    "{event:?} force{force} canonical{:?}",
                    canonical.tag()
                );
            }
        }
    }
}

#[test]
fn system_event_selected_deferred_invalid_tags_missing_nodes_and_false_predicate_errors() {
    use miden_core::{
        Felt,
        deferred::{Node, PrecompileError, Tag, precompile_id},
        events::SystemEvent::*,
    };
    use miden_processor::ExecutionError;
    let procedure =
        system_event_procedure(&system_event_fixture(DeferredRegister, 12), false, |_| {});
    for tag in [
        Tag::TRUE.as_word(),
        [Felt::from(2u32), Felt::ONE, Felt::ZERO, Felt::ZERO],
        [Felt::from(999u32); 4],
    ] {
        let payload = [Felt::ZERO; 8]
            .into_iter()
            .chain(tag)
            .map(|v| v.as_canonical_u64() as usize)
            .collect::<Vec<_>>();
        let (_, result) = system_event_vm(
            &procedure,
            &system_event_input(&payload, 20),
            system_event_advice(),
            None,
            "",
        );
        assert!(
            matches!(result.unwrap_err(), ExecutionError::DeferredError { err: error, .. } if matches!(error.root(), PrecompileError::InvalidNode))
        );
    }
    for event in [DeferredEvaluate, DeferredEvaluateTag, DeferredEvaluatePayload] {
        let procedure = system_event_procedure(&system_event_fixture(event, 4), false, |_| {});
        let (_, result) = system_event_vm(
            &procedure,
            &system_event_input(&[11, 23, 37, 53], 20),
            system_event_advice(),
            None,
            "",
        );
        assert!(
            matches!(result.unwrap_err(), ExecutionError::DeferredError { err: error, .. } if matches!(error.root(), PrecompileError::MissingNode))
        );
    }
    // Pinned uint256 VALUE uses op0/domain bound pointer1, and EQ uses op4.
    let id = precompile_id("uint256");
    let tag = Tag::precompile(id, [Felt::ZERO, Felt::ONE, Felt::ZERO]).unwrap();
    let a = Node::value(tag, [1u32, 2, 3, 4, 5, 6, 7, 8].map(Felt::from)).unwrap();
    let b = Node::value(tag, [11u32, 12, 13, 14, 15, 16, 17, 18].map(Felt::from)).unwrap();
    let eq = Tag::precompile(id, [Felt::from(4u32), Felt::ZERO, Felt::ZERO]).unwrap();
    let payload = a
        .digest()
        .as_elements()
        .iter()
        .chain(b.digest().as_elements())
        .copied()
        .chain(eq.as_word())
        .map(|v| v.as_canonical_u64() as usize)
        .collect::<Vec<_>>();
    let setup = format!("{} {}", system_event_register_setup(&a), system_event_register_setup(&b));
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&payload, 20),
        system_event_advice(),
        None,
        &setup,
    );
    assert!(
        matches!(result.unwrap_err(), ExecutionError::DeferredError { err: error, .. } if matches!(error.root(), PrecompileError::AssertionFailed))
    );
    assert_eq!(processor.deferred_state().get_node(&a.digest()), Some(&a));
    assert_eq!(processor.deferred_state().get_node(&b.digest()), Some(&b));
}

#[test]
fn system_event_physical_mutations_reject_access_alias_publication_and_permissions() {
    use crate::masm::Instruction;
    for (event, n) in system_event_cases() {
        let f = system_event_fixture(event, n);
        for instruction in [
            Instruction::Dup15,
            Instruction::Swap15,
            Instruction::Drop,
            Instruction::Locaddr(0.into()),
        ] {
            let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
            let index = body
                .ops()
                .iter()
                .position(|o| {
                    matches!(
                        o,
                        AnnotatedOp::Instruction {
                            instruction: Instruction::EmitImm(_),
                            ..
                        }
                    )
                })
                .unwrap();
            let unit = match &body.ops()[index] {
                AnnotatedOp::Instruction { unit, .. } => *unit,
                _ => unreachable!(),
            };
            body.ops_mut().insert(
                index,
                AnnotatedOp::Instruction {
                    instruction: instruction.clone(),
                    unit,
                },
            );
            assert!(verify::verify(&plan, &frame, body).is_err(), "{event:?} {instruction:?}");
        }
        let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
        body.ops_mut()
            .retain(|o| !matches!(o, AnnotatedOp::Boundary(Boundary::UnitEnd(_))));
        assert!(verify::verify(&plan, &frame, body).is_err(), "{event:?} omitted publication");
        let function = f.borrow();
        let env = SelectEnv::default();
        let mut selected = select_function(&function, &env).unwrap();
        selected.segments[0].units[0].effects.memory =
            if selected.segments[0].units[0].effects.memory == SourceMemoryEffects::READ {
                SourceMemoryEffects::default()
            } else {
                SourceMemoryEffects::READ
            };
        assert!(
            check_selection(
                selected,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err()
        );
    }
}

#[test]
fn system_event_selected_semantic_mutations_fail_exact_provider_oracles() {
    use miden_core::{Felt, Word, crypto::hash::Poseidon2, events::SystemEvent::*};

    use crate::masm::Instruction;
    let f = system_event_fixture(MapValueCountToStack, 4);
    let payload = [11, 23, 37, 53];
    let key = Word::new(payload.map(|v| Felt::from(v as u32)));
    let values = vec![Felt::ONE; 5];
    for mutation in 0..3 {
        let procedure = system_event_procedure(&f, false, |body| {
            for op in body.ops_mut() {
                if let AnnotatedOp::Instruction {
                    instruction: instruction @ Instruction::EmitImm(_),
                    ..
                } = op
                {
                    if mutation == 1 {
                        *instruction = Instruction::Nop;
                    }
                    if mutation == 2 {
                        *instruction = Instruction::EmitImm(
                            crate::masm::Immediate::Value(crate::masm::Span::unknown(
                                HasMapKey.event_id().as_felt(),
                            ))
                            .into(),
                        );
                    }
                }
            }
        });
        let (processor, result) = system_event_vm(
            &procedure,
            &system_event_input(&payload, 20),
            system_event_advice().with_map([(key, values.clone())]),
            None,
            "",
        );
        result.unwrap();
        let actual = processor.into_parts().0.stack();
        let oracle = || {
            if actual == [Felt::from(5u32), Felt::from(97u32)] {
                Ok(())
            } else {
                Err("MapValueCountToStack must push exact map length five")
            }
        };
        if mutation == 0 {
            oracle().unwrap();
        } else {
            assert_eq!(
                oracle().unwrap_err(),
                "MapValueCountToStack must push exact map length five"
            );
        }
    }
    let f = system_event_fixture(HqwordToMap, 16);
    let payload = (0..16).map(|i| 101 + i * 37).collect::<Vec<_>>();
    let values = payload.iter().map(|v| Felt::new_unchecked(*v as u64)).collect::<Vec<_>>();
    let key = Poseidon2::hash_elements(&values);
    let procedure = system_event_procedure(&f, false, |body| {
        let index = body
            .ops()
            .iter()
            .position(|o| {
                matches!(
                    o,
                    AnnotatedOp::Instruction {
                        instruction: Instruction::EmitImm(_),
                        ..
                    }
                )
            })
            .unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        // Both swaps restore every physical alias while changing the observed payload.
        body.ops_mut().insert(
            index + 1,
            AnnotatedOp::Instruction {
                instruction: Instruction::Swap1,
                unit,
            },
        );
        body.ops_mut().insert(
            index,
            AnnotatedOp::Instruction {
                instruction: Instruction::Swap1,
                unit,
            },
        );
    });
    let (processor, result) =
        system_event_vm(&procedure, &payload, system_event_advice(), None, "");
    result.unwrap();
    let (provider, _) = processor.into_parts();
    let oracle = || {
        if provider.get_mapped_values(&key) == Some(values.as_slice()) {
            Ok(())
        } else {
            Err("HqwordToMap must insert original sixteen lanes under their ordered hash")
        }
    };
    assert_eq!(
        oracle().unwrap_err(),
        "HqwordToMap must insert original sixteen lanes under their ordered hash"
    );
}

#[test]
fn system_event_selected_spills_and_live_original_inputs_retain_all_aliases() {
    use miden_core::{Felt, crypto::hash::Poseidon2, events::SystemEvent::*};
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    for (event, n) in [(HqwordToMap, 16), (HdwordToMapWithDomain, 9)] {
        let mut test = midenc_hir::testing::Test::new(
            "event_live",
            &vec![Type::Felt; 20 + n],
            &vec![Type::Felt; 20 + 2 * n],
        );
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
            let mut b = test.function_builder();
            let results = b
                .system_event(
                    args[20..].iter().copied(),
                    event.event_id().as_felt(),
                    SourceSpan::UNKNOWN,
                )
                .unwrap();
            b.ret(args.into_iter().chain(results), SourceSpan::UNKNOWN).unwrap();
        }
        let f = Parsed {
            context: test.context_rc(),
            function: test.function(),
        };
        let input_args = (0..20)
            .map(|i| 10001 + i)
            .chain((0..n).map(|i| 101 + i * 37))
            .collect::<Vec<_>>();
        let input = system_event_input(&input_args, 20);
        let expected = input_args
            .iter()
            .copied()
            .chain(input_args[20..].iter().copied())
            .chain(input[input_args.len()..].iter().copied())
            .collect::<Vec<_>>();
        let values = input_args[20..]
            .iter()
            .map(|v| Felt::new_unchecked(*v as u64))
            .collect::<Vec<_>>();
        let key = if n == 16 {
            Poseidon2::hash_elements(&values)
        } else {
            Poseidon2::merge_in_domain(
                &[
                    miden_core::Word::new(values[..4].try_into().unwrap()),
                    miden_core::Word::new(values[4..8].try_into().unwrap()),
                ],
                values[8],
            )
        };
        for force in [false, true] {
            let (_, frame, _) = fri_ready(&f, force);
            assert!(!frame.spills().is_empty());
            let procedure = system_event_procedure(&f, force, |_| {});
            let (processor, result) = system_event_vm_outputs(
                &procedure,
                &input,
                &expected,
                system_event_advice(),
                None,
                "",
            );
            result.unwrap();
            assert_eq!(
                processor.into_parts().0.get_mapped_values(&key),
                Some(&values[..if n == 16 { 16 } else { 8 }])
            );
        }
    }
}

#[test]
fn system_event_selected_advice_resource_errors_and_partial_inverse_state() {
    use miden_core::{
        Felt, Word,
        crypto::merkle::MerkleStore,
        events::SystemEvent::*,
        field::{BasedVectorSpace, Field, QuadFelt},
    };
    use miden_processor::{ExecutionError, advice::AdviceError};
    // Each stored Merkle node is three words; the initial advice sentinel is one Felt.
    let base = MerkleStore::default().num_internal_nodes() * 96 + 8;
    for (event, n, added) in [
        (HdwordToMap, 8, 96),
        (HdwordToMapWithDomain, 9, 96),
        (HqwordToMap, 16, 160),
        (HpermToMap, 12, 96),
        (MerkleNodeMerge, 8, 96),
    ] {
        let payload = (0..n).map(|i| 101 + i * 37).collect::<Vec<_>>();
        let procedure = system_event_procedure(&system_event_fixture(event, n), false, |_| {});
        let (processor, result) = system_event_vm(
            &procedure,
            &system_event_input(&payload, 20),
            system_event_advice(),
            Some(base),
            "",
        );
        assert!(
            matches!(result.unwrap_err(), ExecutionError::AdviceError { err: AdviceError::SizeBudgetExceeded { current, added: actual, max }, .. } if current == base && actual == added && max == base),
            "{event:?}"
        );
        let (provider, _) = processor.into_parts();
        assert_eq!(provider.stack(), [Felt::from(97u32)]);
        assert!(provider.map().is_empty());
    }
    let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
    let advice = system_event_advice().with_map([(key, vec![Felt::ONE; 2])]);
    let procedure =
        system_event_procedure(&system_event_fixture(MapValueToStack, 4), false, |_| {});
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&[11, 23, 37, 53], 20),
        advice,
        Some(base + 48),
        "",
    );
    assert!(
        matches!(result.unwrap_err(), ExecutionError::AdviceError { err: AdviceError::SizeBudgetExceeded { current, added: 16, max }, .. } if current == base+48 && max == base+48)
    );
    assert_eq!(processor.into_parts().0.stack(), [Felt::from(97u32)]);
    let procedure = system_event_procedure(&system_event_fixture(Ext2Inv, 2), false, |_| {});
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&[3, 5], 20),
        system_event_advice(),
        Some(base + 8),
        "",
    );
    assert!(
        matches!(result.unwrap_err(), ExecutionError::AdviceError { err: AdviceError::SizeBudgetExceeded { current, added: 8, max }, .. } if current == base+8 && max == base+8)
    );
    let inverse = QuadFelt::from_basis_coefficients_fn(|i| Felt::from([3u32, 5][i])).inverse();
    assert_eq!(
        processor.into_parts().0.stack(),
        [inverse.as_basis_coefficients_slice()[0], Felt::from(97u32)]
    );
}

#[test]
fn system_event_selected_deferred_state_resource_limit_preserves_registered_prefix() {
    use miden_core::{
        Felt,
        deferred::{DeferredError, MAX_DEFERRED_ELEMENTS, Node, PrecompileError},
        events::SystemEvent::DeferredRegister,
    };
    use miden_processor::{FastProcessor, StackInputs};
    let f = system_event_fixture(DeferredRegister, 12);
    let procedure = system_event_procedure(&f, false, |_| {});
    let payload = [1, 23, 37, 53, 71, 89, 107, 127, 2, 0, 0, 0];
    let input = system_event_input(&payload, 4);
    let felts = input.iter().map(|v| Felt::new_unchecked(*v as u64)).collect::<Vec<_>>();
    let mut processor = FastProcessor::new(StackInputs::new(&felts).unwrap());
    let initial = processor.deferred_state().num_elements();
    let capacity = (MAX_DEFERRED_ELEMENTS - initial) / 12;
    // Keep the counter below the exact twelve observed lanes at each selected call.
    // A runtime loop avoids unrolling roughly87k copies in the assembler.
    let code = format!(
        "{} begin push.{} dup neq.0 while.true movdn.12 exec.selected push.1 add movup.12 sub.1 \
         dup neq.0 end end",
        procedure.text,
        capacity + 1
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("deferred_limit", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host();
    let mut context = processor.get_initial_resume_context(&program).unwrap();
    let error = loop {
        match processor.step_sync(&mut host, context) {
            Ok(Some(next)) => context = next,
            Ok(None) => panic!("missing deferred resource failure"),
            Err(error) => break error,
        }
    };
    assert!(
        matches!(error, miden_processor::ExecutionError::DeferredError { err: ref error, .. } if matches!(error.root(), PrecompileError::Other(DeferredError::DeferredStateTooLarge { num_elements: 12, max }) if *max == MAX_DEFERRED_ELEMENTS - initial - 12*capacity)),
        "{error}"
    );
    assert_eq!(processor.deferred_state().num_elements(), initial + 12 * capacity);
    let node = |first| {
        Node::chunks(vec![[
            Felt::new_unchecked(first),
            Felt::from(23u32),
            Felt::from(37u32),
            Felt::from(53u32),
            Felt::from(71u32),
            Felt::from(89u32),
            Felt::from(107u32),
            Felt::from(127u32),
        ]])
        .unwrap()
    };
    for first in [1, capacity as u64] {
        let expected = node(first);
        assert_eq!(processor.deferred_state().get_node(&expected.digest()), Some(&expected));
    }
    assert!(
        processor
            .deferred_state()
            .get_node(&node(capacity as u64 + 1).digest())
            .is_none()
    );
}

#[test]
fn system_event_framework_and_registration_preserves_pinned_invalid_tag() {
    use miden_core::{
        Felt,
        deferred::{DeferredError, Node, PrecompileError, TRUE_DIGEST},
        events::SystemEvent::DeferredRegister,
    };
    let f = system_event_fixture(DeferredRegister, 12);
    let payload = [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0];
    let expected = Node::and(TRUE_DIGEST, TRUE_DIGEST);
    let initial_elements = processor_with_advice(&[]).deferred_state().num_elements();
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, |_| {});
        for suffix in [4, 20] {
            let input = system_event_input(&payload, suffix);
            let (processor, result) =
                system_event_vm(&procedure, &input, system_event_advice(), None, "");
            let error = result.unwrap_err();
            assert!(
                matches!(
                    error,
                    miden_processor::ExecutionError::DeferredError {
                        err: PrecompileError::Other(DeferredError::InvalidTag),
                        ..
                    }
                ),
                "exact DeferredRegister AND(TRUE,TRUE), id{}, payload{payload:?}: {error:?}",
                DeferredRegister.event_id().as_u64()
            );
            // The handler fails before the immediate-event ID is dropped. Every
            // original payload/suffix lane and legitimate floor zero remains.
            let padding = if force && suffix == 4 { 12 } else { 0 };
            let stack = [DeferredRegister.event_id().as_u64() as usize]
                .into_iter()
                .chain(input.iter().copied())
                .chain(core::iter::repeat_n(0, padding))
                .collect::<Vec<_>>();
            assert_eq!(processor.stack().len(), stack.len());
            assert_eq!(
                (0..stack.len())
                    .map(|i| processor.stack_get(i).as_canonical_u64() as usize)
                    .collect::<Vec<_>>(),
                stack
            );
            assert_eq!(processor.deferred_state().get_node(&expected.digest()), None);
            assert_eq!(processor.deferred_state().get_canonical_digest(expected.digest()), None);
            assert_eq!(processor.deferred_state().get_node(&TRUE_DIGEST), Some(&Node::TRUE));
            assert_eq!(processor.deferred_state().num_elements(), initial_elements);
            assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
            assert_eq!(processor.into_parts().0.stack(), [Felt::from(97u32)]);
        }
    }
}

#[test]
fn system_event_remaining_variants_observe_exact_minimum_floor_and_state() {
    use miden_core::{
        Felt,
        crypto::{
            hash::Poseidon2,
            merkle::{MerkleStore, MerkleTree},
        },
        deferred::{Node, TRUE_DIGEST},
        events::SystemEvent::*,
        field::{BasedVectorSpace, Field, QuadFelt},
    };
    let left = merkle_word(101);
    let right = merkle_word(701);
    let tree = MerkleTree::new(vec![left, right]).unwrap();
    let data = [11u32, 23, 37, 53, 71, 89, 107, 127].map(Felt::from);
    let node = Node::chunks(vec![data]).unwrap();
    for (event, n) in [
        (MerkleNodeToStack, 6),
        (MerkleNodeMerge, 8),
        (Ext2Inv, 2),
        (DeferredRegister, 12),
        (DeferredEvaluate, 4),
        (DeferredEvaluateTag, 4),
        (DeferredEvaluatePayload, 4),
    ] {
        let payload = match event {
            MerkleNodeToStack => [1, 1].into_iter().chain(merkle_lanes(tree.root())).collect(),
            MerkleNodeMerge => merkle_lanes(left).into_iter().chain(merkle_lanes(right)).collect(),
            Ext2Inv => vec![3, 5],
            DeferredRegister => {
                data.iter().map(|v| v.as_canonical_u64() as usize).chain([2, 0, 0, 0]).collect()
            }
            _ => vec![0; 4],
        };
        for force in [false, true] {
            let f = system_event_fixture(event, n);
            let procedure = system_event_procedure(&f, force, |_| {});
            let advice = system_event_advice().with_merkle_store(MerkleStore::from(&tree));
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&payload, 16 - n),
                advice,
                None,
                "",
            );
            result.unwrap();
            if event == DeferredRegister {
                assert_eq!(processor.deferred_state().get_node(&node.digest()), Some(&node));
            }
            assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
            let (provider, _) = processor.into_parts();
            let mut expected = match event {
                MerkleNodeToStack => right.as_elements().to_vec(),
                Ext2Inv => {
                    let inverse =
                        QuadFelt::from_basis_coefficients_fn(|i| Felt::from([3u32, 5][i]))
                            .inverse();
                    inverse.as_basis_coefficients_slice().iter().rev().copied().collect()
                }
                DeferredEvaluate | DeferredEvaluateTag => vec![Felt::ZERO; 4],
                _ => vec![],
            };
            expected.push(Felt::from(97u32));
            assert_eq!(provider.stack(), expected, "{event:?} force{force}");
            if event == MerkleNodeMerge {
                let root = Poseidon2::merge(&[left, right]);
                assert_eq!(provider.get_tree_node(root, Felt::ONE, Felt::ZERO).unwrap(), left);
                assert_eq!(provider.get_tree_node(root, Felt::ONE, Felt::ONE).unwrap(), right);
            }
        }
    }
}

#[test]
fn system_event_effect_order_preserves_advice_even_with_dead_result_aliases() {
    use miden_core::{Felt, Word, events::SystemEvent::*};
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    let mut test =
        midenc_hir::testing::Test::new("event_order", &vec![Type::Felt; 4], &vec![Type::Felt; 4]);
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
        let mut b = test.function_builder();
        b.system_event(
            args.iter().copied(),
            MapValueCountToStack.event_id().as_felt(),
            SourceSpan::UNKNOWN,
        )
        .unwrap();
        let results = b
            .system_event(args, HasMapKey.event_id().as_felt(), SourceSpan::UNKNOWN)
            .unwrap();
        b.ret(results, SourceSpan::UNKNOWN).unwrap();
    }
    let f = Parsed {
        context: test.context_rc(),
        function: test.function(),
    };
    let function = f.borrow();
    let env = SelectEnv::default();
    let mut selected = select_function(&function, &env).unwrap();
    assert_eq!(selected.segments[0].units.len(), 2);
    selected.segments[0].units.reverse();
    assert!(
        check_selection(
            selected,
            &SourceContext::Hir {
                function: &function,
                env: &env
            }
        )
        .is_err()
    );
    let procedure = system_event_procedure(&f, false, |_| {});
    let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&[11, 23, 37, 53], 20),
        system_event_advice().with_map([(key, vec![Felt::ONE; 5])]),
        None,
        "",
    );
    result.unwrap();
    assert_eq!(
        processor.into_parts().0.stack(),
        [Felt::ONE, Felt::from(5u32), Felt::from(97u32)]
    );
}

#[test]
fn memory_system_event_source_local_cast_add_boundary_drops_endpoint_with_spills() {
    use miden_core::{Felt, Word, events::SystemEvent::MemToMap};
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    for empty in [false, true] {
        let mut test = midenc_hir::testing::Test::new(
            "memory_boundary",
            &vec![Type::Felt; 28],
            &vec![Type::Felt; 32],
        );
        let args = test
            .function()
            .borrow()
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .map(|a| *a as midenc_hir::ValueRef)
            .collect::<Vec<_>>();
        let locals = (0..4)
            .map(|_| test.function().borrow_mut().alloc_local(Type::Felt))
            .collect::<Vec<_>>();
        {
            let span = SourceSpan::UNKNOWN;
            let mut b = test.function_builder();
            for (local, arg) in locals.iter().zip(&args[4..8]) {
                b.store_local(*local, *arg, span).unwrap();
            }
            let ptr = b.local_address(locals[0], span).unwrap();
            let bits = b.ptrtoint(ptr, Type::U32, span).unwrap();
            let base = b.bitcast(bits, Type::Felt, span).unwrap();
            let delta = b.felt(Felt::from(4u32), span);
            let end = b.add_unchecked(base, delta, span).unwrap();
            let start = if empty { end } else { base };
            let payload = args[..4].iter().copied().chain([start, end]);
            b.system_event(payload, MemToMap.event_id().as_felt(), span).unwrap();
            let loaded = locals
                .iter()
                .map(|local| b.load_local(*local, span).unwrap())
                .collect::<Vec<_>>();
            b.ret(args.iter().copied().chain(loaded), span).unwrap();
        }
        let f = Parsed {
            context: test.context_rc(),
            function: test.function(),
        };
        let arguments = [11, 23, 37, 53, 71, 89, 107, 127]
            .into_iter()
            .chain((0..20).map(|i| 900 + i))
            .collect::<Vec<_>>();
        let input = system_event_input(&arguments, 20);
        let expected = arguments
            .iter()
            .copied()
            .chain(arguments[4..8].iter().copied())
            .chain(input[28..].iter().copied())
            .collect::<Vec<_>>();
        for force in [false, true] {
            let (_, frame, _) = fri_ready(&f, force);
            assert_eq!(frame.source().extent, 4);
            assert!(!frame.spills().is_empty());
            let procedure = system_event_procedure(&f, force, |_| {});
            let (processor, result) = system_event_vm_outputs(
                &procedure,
                &input,
                &expected,
                system_event_advice(),
                None,
                "",
            );
            result.unwrap();
            let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
            let values = if empty {
                vec![]
            } else {
                [71u32, 89, 107, 127].map(Felt::from).to_vec()
            };
            let (provider, _) = processor.into_parts();
            assert_eq!(provider.get_mapped_values(&key), Some(values.as_slice()));
            assert_eq!(provider.stack(), [Felt::from(97u32)]);
        }
    }
}

#[test]
fn memory_system_event_mem_to_map_exact_order_holes_empty_terminal_budget_and_collision() {
    use miden_core::{Felt, Word, crypto::merkle::MerkleStore, events::SystemEvent::MemToMap};
    use miden_processor::{ExecutionError, advice::AdviceError};
    let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
    let base = MerkleStore::default().num_internal_nodes() * 96 + 8;
    for force in [false, true] {
        let f = system_event_fixture(MemToMap, 6);
        let (_, frame, _) = fri_ready(&f, force);
        let terminal_fmp = 127 + frame.aligned_size();
        let procedure = system_event_procedure(&f, force, |_| {});
        // U32_MAX-1 is the VM frame-pointer cell. Its in-call value includes this
        // verified procedure's actual frame allocation in either scheduling mode.
        for (start, end, setup, want) in [
            (401, 405, "push.71 mem_store.401 push.107 mem_store.403", vec![71u32, 0, 107, 0]),
            (405, 405, "", vec![]),
            (
                0xfffffffe,
                0xffffffff,
                "push.127 mem_store.4294967294 push.999 mem_store.4294967295",
                vec![terminal_fmp],
            ),
        ] {
            let payload = [11, 23, 37, 53, start, end];
            let values = want.into_iter().map(Felt::from).collect::<Vec<_>>();
            for prior in [None, Some(values.clone()), Some(vec![Felt::from(999u32)])] {
                let advice = prior.as_ref().map_or_else(system_event_advice, |old| {
                    system_event_advice().with_map([(key, old.clone())])
                });
                let (processor, result) = system_event_vm(
                    &procedure,
                    &system_event_input(&payload, 20),
                    advice,
                    None,
                    setup,
                );
                if prior.as_ref().is_some_and(|old| old != &values) {
                    assert!(matches!(
                        result.unwrap_err(),
                        ExecutionError::AdviceError {
                            err: AdviceError::MapKeyAlreadyPresent { .. },
                            ..
                        }
                    ));
                } else {
                    result.unwrap();
                }
                if start == 401 {
                    for (address, value) in
                        [(400, 0), (401, 71), (402, 0), (403, 107), (404, 0), (405, 0)]
                    {
                        assert_eq!(
                            processor
                                .memory()
                                .read_element(Default::default(), Felt::from(address as u32))
                                .unwrap(),
                            Felt::from(value as u32)
                        );
                    }
                }
                if end == 0xffffffff {
                    assert_eq!(
                        processor
                            .memory()
                            .read_element(Default::default(), Felt::new_unchecked(0xffffffff))
                            .unwrap(),
                        Felt::from(999u32)
                    );
                }
                let (provider, _) = processor.into_parts();
                assert_eq!(
                    provider.get_mapped_values(&key),
                    Some(prior.as_deref().unwrap_or(&values))
                );
                assert_eq!(provider.stack(), [Felt::from(97u32)]);
            }
        }
        // At a full budget, an identical value is free; a differing value is checked before insert.
        let values = [71u32, 89, 107, 127].map(Felt::from).to_vec();
        for identical in [false, true] {
            let setup = if identical {
                "push.71 mem_store.400 push.89 mem_store.401 push.107 mem_store.402 push.127 \
                 mem_store.403"
            } else {
                ""
            };
            let advice = system_event_advice().with_map([(key, values.clone())]);
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&[11, 23, 37, 53, 400, 404], 20),
                advice,
                Some(base + 64),
                setup,
            );
            if identical {
                result.unwrap();
            } else {
                assert!(matches!(
                    result.unwrap_err(),
                    ExecutionError::AdviceError {
                        err: AdviceError::SizeBudgetExceeded { .. },
                        ..
                    }
                ));
            }
            assert_eq!(processor.into_parts().0.get_mapped_values(&key), Some(values.as_slice()));
        }
    }
}

#[test]
fn memory_system_event_deferred_chunks_precompile_order_idempotence_and_guard_errors() {
    use miden_core::{
        Felt,
        deferred::{DeferredError, Node, PrecompileError, TRUE_DIGEST, Tag, precompile_id},
        events::SystemEvent::DeferredRegisterData,
    };
    use miden_processor::{ExecutionError, MemoryError};
    let values = [11u32, 23, 37, 53, 71, 89, 107, 127, 149, 173, 197, 223, 251, 281, 313, 347]
        .map(Felt::from);
    let chunks = vec![values[..8].try_into().unwrap(), values[8..].try_into().unwrap()];
    let setup = eval_circuit_initialize(
        400,
        &values.iter().map(|v| v.as_canonical_u64() as usize).collect::<Vec<_>>(),
    );
    let initial = processor_with_advice(&[]).deferred_state().num_elements();
    for force in [false, true] {
        let procedure =
            system_event_procedure(&system_event_fixture(DeferredRegisterData, 6), force, |_| {});
        for (tag, count, expected) in [
            (Tag::CHUNKS, 2, Node::chunks(chunks.clone()).unwrap()),
            (
                Tag::precompile(precompile_id("uint256"), [Felt::ZERO, Felt::ONE, Felt::ZERO])
                    .unwrap(),
                1,
                Node::value(
                    Tag::precompile(precompile_id("uint256"), [Felt::ZERO, Felt::ONE, Felt::ZERO])
                        .unwrap(),
                    chunks[0],
                )
                .unwrap(),
            ),
        ] {
            let payload = tag
                .as_word()
                .iter()
                .map(|v| v.as_canonical_u64() as usize)
                .chain([400, count])
                .collect::<Vec<_>>();
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&payload, 20),
                system_event_advice(),
                None,
                &setup,
            );
            result.unwrap();
            assert_eq!(processor.deferred_state().get_node(&expected.digest()), Some(&expected));
            assert_eq!(
                processor.deferred_state().get_canonical_digest(expected.digest()),
                Some(expected.digest())
            );
            assert_eq!(processor.deferred_state().num_elements(), initial + 4 + 8 * count);
            assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
            assert_eq!(processor.into_parts().0.stack(), [Felt::from(97u32)]);
        }
        // Handler ordering is pinned source evidence; errors are observable conformance,
        // not an untraced host-read counter. AND n1 reads then fails its constructor.
        for (payload, kind) in [
            ([2, 0, 0, 0, 5, 0], 0),
            ([2, 0, 0, 0, 5, 0x1_0000_0000], 0),
            ([0, 0, 0, 0, 5, 1], 0),
            ([0, 7, 0, 0, 5, 1], 0),
            ([1, 7, 0, 0, 5, 1], 0),
            ([2, 0, 7, 0, 5, 1], 0),
            ([1, 0, 0, 0, 5, 2], 0),
            ([1, 0, 0, 0, 400, 1], 1),
            ([999, 0, 0, 0, 400, 1], 0),
            ([2, 0, 0, 0, 5, 131072], 2),
            ([2, 0, 0, 0, 0x1_0000_0000, 1], 3),
            ([2, 0, 0, 0, 5, 1], 4),
            ([2, 0, 0, 0, 0xfffffff8, 1], 3),
        ] {
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&payload, 20),
                system_event_advice(),
                None,
                &setup,
            );
            let error = result.unwrap_err();
            match kind {
                0 => assert!(
                    matches!(error,ExecutionError::DeferredError {err:ref e,..} if matches!(e.root(),PrecompileError::InvalidNode)),
                    "{payload:?} {error}"
                ),
                1 => assert!(
                    matches!(error,ExecutionError::DeferredError {err:ref e,..} if matches!(e.root(),PrecompileError::Other(DeferredError::InvalidTag))),
                    "{payload:?} {error}"
                ),
                2 => assert!(
                    matches!(error,ExecutionError::DeferredError {err:ref e,..} if matches!(e.root(),PrecompileError::Other(DeferredError::DeferredStateTooLarge {..}))),
                    "{error}"
                ),
                3 => assert!(matches!(
                    error,
                    ExecutionError::MemoryError {
                        err: MemoryError::AddressOutOfBounds { .. },
                        ..
                    }
                )),
                _ => assert!(matches!(
                    error,
                    ExecutionError::MemoryError {
                        err: MemoryError::UnalignedWordAccess { .. },
                        ..
                    }
                )),
            }
            assert_eq!(processor.deferred_state().num_elements(), initial);
            assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
        }
        let (processor, result) = system_event_vm(
            &procedure,
            &system_event_input(&[2, 0, 0, 0, 0xfffffff4, 1], 20),
            system_event_advice(),
            None,
            "push.71 mem_store.4294967291",
        );
        result.unwrap();
        let mut zero = [Felt::ZERO; 8];
        zero[7] = Felt::from(71u32);
        let node = Node::chunks(vec![zero]).unwrap();
        assert_eq!(processor.deferred_state().get_node(&node.digest()), Some(&node));
    }
}

#[test]
fn memory_system_event_mem_to_map_vm_numeric_guards_and_semantic_mutants() {
    use miden_core::{
        Felt, Word,
        events::SystemEvent::{DeferredRegisterData, MemToMap},
    };
    use miden_processor::{ExecutionError, MemoryError};
    let f = system_event_fixture(MemToMap, 6);
    let procedure = system_event_procedure(&f, false, |_| {});
    for (start, end, address) in [
        (0x1_0000_0000, 0x1_0000_0001, Some(0x1_0000_0000)),
        (0, 0x1_0000_0000, Some(0x1_0000_0000)),
        (9, 2, None),
    ] {
        let (processor, result) = system_event_vm(
            &procedure,
            &system_event_input(&[11, 23, 37, 53, start, end], 20),
            system_event_advice(),
            None,
            "",
        );
        match (result.unwrap_err(), address) {
            (
                ExecutionError::MemoryError {
                    err: MemoryError::AddressOutOfBounds { addr },
                    ..
                },
                Some(expected),
            ) => assert_eq!(addr, expected),
            (
                ExecutionError::MemoryError {
                    err:
                        MemoryError::InvalidMemoryRange {
                            start_addr,
                            end_addr,
                        },
                    ..
                },
                None,
            ) => assert_eq!((start_addr, end_addr), (9, 2)),
            (error, _) => panic!("{error}"),
        }
        assert!(processor.into_parts().0.map().is_empty());
    }
    for mutation in 0..4 {
        let procedure = system_event_procedure(&f, false, |body| {
            let index = body
                .ops()
                .iter()
                .position(|o| {
                    matches!(
                        o,
                        AnnotatedOp::Instruction {
                            instruction: crate::masm::Instruction::EmitImm(_),
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
                    if let AnnotatedOp::Instruction { instruction, .. } = &mut body.ops_mut()[index]
                    {
                        *instruction = crate::masm::Instruction::Nop;
                    }
                }
                1 => {
                    if let AnnotatedOp::Instruction { instruction, .. } = &mut body.ops_mut()[index]
                    {
                        *instruction = crate::masm::Instruction::EmitImm(
                            crate::masm::Immediate::Value(crate::masm::Span::unknown(
                                DeferredRegisterData.event_id().as_felt(),
                            ))
                            .into(),
                        );
                    }
                }
                _ => {
                    let swap = if mutation == 2 {
                        crate::masm::Instruction::Swap1
                    } else {
                        crate::masm::Instruction::Swap5
                    };
                    body.ops_mut().insert(
                        index + 1,
                        AnnotatedOp::Instruction {
                            instruction: swap.clone(),
                            unit,
                        },
                    );
                    body.ops_mut().insert(
                        index,
                        AnnotatedOp::Instruction {
                            instruction: swap,
                            unit,
                        },
                    );
                }
            }
        });
        let (processor, result) = system_event_vm(
            &procedure,
            &system_event_input(&[11, 23, 37, 53, 400, 402], 20),
            system_event_advice(),
            None,
            "push.71 mem_store.400 push.89 mem_store.401",
        );
        let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
        let (provider, _) = processor.into_parts();
        assert!(
            result.is_err()
                || provider.get_mapped_values(&key)
                    != Some([Felt::from(71u32), Felt::from(89u32)].as_slice()),
            "mutant{mutation}"
        );
    }
}

#[test]
fn memory_system_event_deferred_repeated_chunks_and_failed_evaluation_preserve_partial_state() {
    use miden_core::{
        Felt,
        deferred::{Node, PrecompileError, TRUE_DIGEST, Tag, precompile_id},
        events::SystemEvent::DeferredRegisterData,
    };
    let data = [11u32, 23, 37, 53, 71, 89, 107, 127].map(Felt::from);
    let node = Node::chunks(vec![data]).unwrap();
    let procedure =
        system_event_procedure(&system_event_fixture(DeferredRegisterData, 6), false, |_| {});
    let setup = format!(
        "{} {}",
        eval_circuit_initialize(400, &data.map(|v| v.as_canonical_u64() as usize)),
        system_event_register_setup(&node)
    );
    let initial = processor_with_advice(&[]).deferred_state().num_elements();
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&[2, 0, 0, 0, 400, 1], 20),
        system_event_advice(),
        None,
        &setup,
    );
    result.unwrap();
    assert_eq!(processor.deferred_state().num_elements(), initial + 12);
    assert_eq!(
        processor.deferred_state().get_canonical_digest(node.digest()),
        Some(node.digest())
    );
    let id = precompile_id("uint256");
    let value_tag = Tag::precompile(id, [Felt::ZERO, Felt::ONE, Felt::ZERO]).unwrap();
    let a = Node::value(value_tag, data).unwrap();
    let b = Node::value(value_tag, [1u32, 2, 3, 4, 5, 6, 7, 8].map(Felt::from)).unwrap();
    let eq = Tag::precompile(id, [Felt::from(4u32), Felt::ZERO, Felt::ZERO]).unwrap();
    let predicate = Node::join(eq, a.digest(), b.digest()).unwrap();
    let children = merkle_lanes(a.digest())
        .into_iter()
        .chain(merkle_lanes(b.digest()))
        .collect::<Vec<_>>();
    let setup = format!(
        "{} {} {}",
        system_event_register_setup(&a),
        system_event_register_setup(&b),
        eval_circuit_initialize(400, &children)
    );
    let payload = eq
        .as_word()
        .iter()
        .map(|v| v.as_canonical_u64() as usize)
        .chain([400, 1])
        .collect::<Vec<_>>();
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&payload, 20),
        system_event_advice(),
        None,
        &setup,
    );
    assert!(
        matches!(result.unwrap_err(),miden_processor::ExecutionError::DeferredError {err:ref error,..} if matches!(error.root(),PrecompileError::AssertionFailed))
    );
    assert_eq!(processor.deferred_state().get_node(&a.digest()), Some(&a));
    assert_eq!(processor.deferred_state().get_node(&b.digest()), Some(&b));
    assert_eq!(processor.deferred_state().get_node(&predicate.digest()), Some(&predicate));
    assert_eq!(processor.deferred_state().get_canonical_digest(predicate.digest()), None);
    assert_eq!(processor.deferred_state().num_elements(), initial + 36);
    assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
}

#[test]
fn memory_system_event_deferred_semantic_omission_wrong_id_and_payload_mutants() {
    use miden_core::{
        Felt,
        deferred::Node,
        events::SystemEvent::{DeferredRegisterData, MemToMap},
    };
    let f = system_event_fixture(DeferredRegisterData, 6);
    let values = [11u32, 23, 37, 53, 71, 89, 107, 127].map(Felt::from);
    let expected = Node::chunks(vec![values]).unwrap();
    for mutation in 0..4 {
        let procedure = system_event_procedure(&f, false, |body| {
            let index = body
                .ops()
                .iter()
                .position(|o| {
                    matches!(
                        o,
                        AnnotatedOp::Instruction {
                            instruction: crate::masm::Instruction::EmitImm(_),
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
                    if let AnnotatedOp::Instruction { instruction, .. } = &mut body.ops_mut()[index]
                    {
                        *instruction = crate::masm::Instruction::Nop;
                    }
                }
                1 => {
                    if let AnnotatedOp::Instruction { instruction, .. } = &mut body.ops_mut()[index]
                    {
                        *instruction = crate::masm::Instruction::EmitImm(
                            crate::masm::Immediate::Value(crate::masm::Span::unknown(
                                MemToMap.event_id().as_felt(),
                            ))
                            .into(),
                        );
                    }
                }
                _ => {
                    let swap = if mutation == 2 {
                        crate::masm::Instruction::Swap1
                    } else {
                        crate::masm::Instruction::Swap5
                    };
                    body.ops_mut().insert(
                        index + 1,
                        AnnotatedOp::Instruction {
                            instruction: swap.clone(),
                            unit,
                        },
                    );
                    body.ops_mut().insert(
                        index,
                        AnnotatedOp::Instruction {
                            instruction: swap,
                            unit,
                        },
                    );
                }
            }
        });
        let (processor, _) = system_event_vm(
            &procedure,
            &system_event_input(&[2, 0, 0, 0, 400, 1], 20),
            system_event_advice(),
            None,
            &eval_circuit_initialize(400, &values.map(|v| v.as_canonical_u64() as usize)),
        );
        assert_eq!(
            processor.deferred_state().get_node(&expected.digest()),
            None,
            "mutant{mutation}"
        );
    }
}

#[test]
fn memory_system_event_deferred_maximum_payload_budget_keeps_registered_prefix() {
    use miden_core::{
        Felt,
        deferred::{DeferredError, MAX_DEFERRED_ELEMENTS, Node, PrecompileError, TRUE_DIGEST},
        events::SystemEvent::DeferredRegisterData,
    };
    let prefix = Node::chunks(vec![[Felt::ONE; 8]]).unwrap();
    let initial = processor_with_advice(&[]).deferred_state().num_elements();
    let setup = system_event_register_setup(&prefix);
    let procedure =
        system_event_procedure(&system_event_fixture(DeferredRegisterData, 6), false, |_| {});
    let (processor, result) = system_event_vm(
        &procedure,
        &system_event_input(&[2, 0, 0, 0, 400, 131071], 20),
        system_event_advice(),
        None,
        &setup,
    );
    assert!(
        matches!(result.unwrap_err(),miden_processor::ExecutionError::DeferredError {err:ref error,..} if matches!(error.root(),PrecompileError::Other(DeferredError::DeferredStateTooLarge {num_elements:1048572,max}) if *max==MAX_DEFERRED_ELEMENTS-initial-12))
    );
    assert_eq!(processor.deferred_state().get_node(&prefix.digest()), Some(&prefix));
    assert_eq!(
        processor.deferred_state().get_canonical_digest(prefix.digest()),
        Some(prefix.digest())
    );
    assert_eq!(processor.deferred_state().num_elements(), initial + 12);
    assert_eq!(processor.deferred_state().root(), TRUE_DIGEST);
}

#[test]
fn memory_system_event_selected_minimum_physical_floor_preserves_provider_and_memory() {
    use miden_core::{
        Felt, Word,
        deferred::Node,
        events::SystemEvent::{DeferredRegisterData, MemToMap},
    };
    let data = [11u32, 23, 37, 53, 71, 89, 107, 127].map(Felt::from);
    let setup = eval_circuit_initialize(400, &data.map(|v| v.as_canonical_u64() as usize));
    let node = Node::chunks(vec![data]).unwrap();
    let key = Word::new([11u32, 23, 37, 53].map(Felt::from));
    for (event, payload) in [
        (MemToMap, [11, 23, 37, 53, 400, 408]),
        (DeferredRegisterData, [2, 0, 0, 0, 400, 1]),
    ] {
        for force in [false, true] {
            let procedure = system_event_procedure(&system_event_fixture(event, 6), force, |_| {});
            let (processor, result) = system_event_vm(
                &procedure,
                &system_event_input(&payload, 10),
                system_event_advice(),
                None,
                &setup,
            );
            result.unwrap();
            for (i, value) in data.into_iter().enumerate() {
                assert_eq!(
                    processor
                        .memory()
                        .read_element(Default::default(), Felt::from(400 + i as u32))
                        .unwrap(),
                    value
                );
            }
            if event == DeferredRegisterData {
                assert_eq!(processor.deferred_state().get_node(&node.digest()), Some(&node));
            }
            let (provider, _) = processor.into_parts();
            if event == MemToMap {
                assert_eq!(provider.get_mapped_values(&key), Some(data.as_slice()));
            }
            assert_eq!(provider.stack(), [Felt::from(97u32)]);
        }
    }
}

#[test]
fn println_actual_host_matrix() {
    const SENTINEL: &str = "MIDENC_TASK10_PRINTLN_CHILD";
    const COMPLETE: &str = "TASK10_PRINTLN_CHILD_CASES=42";
    if std::env::var_os(SENTINEL).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "select::tests::intrinsics::println_actual_host_matrix",
                "--nocapture",
            ])
            .env(SENTINEL, "1")
            .output()
            .unwrap();
        let stdout = alloc::string::String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "child failed: {stdout}\n{}",
            alloc::string::String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.lines().any(|line| line == COMPLETE), "child did not complete: {stdout}");
        return;
    }
    static LOGGER: PrintLnLogger = PrintLnLogger;
    log::set_logger(&LOGGER).expect("isolated child owns the logger before fixture setup");
    log::set_max_level(log::LevelFilter::Info);
    let cases = println_host_cases() + println_selected_host_cases() + println_ordered_host_cases();
    assert_eq!(cases, 42);
    std::println!("{COMPLETE}");
}

pub(super) fn println_fixture(live: bool) -> Parsed {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    let types = vec![midenc_hir::PointerType::new(Type::U8).into(), Type::U32];
    let outputs = if live { types.clone() } else { vec![] };
    let mut test = midenc_hir::testing::Test::new("print", &types, &outputs);
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
        let mut b = test.function_builder();
        b.println(args[0], args[1], SourceSpan::UNKNOWN).unwrap();
        b.ret(if live { args } else { vec![] }, SourceSpan::UNKNOWN).unwrap();
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn println_selects_source_owned_consuming_read_unit() {
    let f = println_fixture(false);
    let selected = checked(&f, &SelectEnv::default());
    let unit = &selected.segments[0].units[0];
    assert_eq!(unit.stack_inputs.len(), 2);
    assert!(unit.outputs.is_empty());
    assert!(unit.stored_inputs.is_empty());
    assert!(unit.scratch.is_empty());
    assert_eq!(unit.effects.memory, SourceMemoryEffects::READ);
    assert!(unit.effects.may_trap);
    ready(&f, &SelectEnv::default());
}

fn println_host_cases() -> usize {
    let id = crate::Event::PrintLn.as_event_id().as_u64();
    let mut count = 0;
    let mut raw = |initialize: &str, pointer: u64, length: u64, want: &str, warning: bool| {
        let records = debug_println_code(&format!(
            "begin {initialize} push.{length} push.{pointer} push.{id} emit drop drop drop \
             push.73 push.73 assert_eq end"
        ));
        assert_eq!(records.len(), 1, "ptr{pointer} len{length}: {records:?}");
        assert_eq!(
            records[0].0,
            if warning {
                log::Level::Warn
            } else {
                log::Level::Info
            }
        );
        if warning {
            assert!(records[0].1.contains(want), "{:?}", records[0]);
        } else {
            assert_eq!(records[0].1, want);
        }
        count += 1;
    };
    // Distinct little-endian cells; the nonzero high bits must not reach the decoder.
    let initialize = "push.22620095041 mem_store.100 push.1212630597 mem_store.101";
    for (residue, length, want) in [
        (0, 1, "A"),
        (1, 1, "B"),
        (2, 1, "C"),
        (3, 1, "D"),
        (0, 4, "ABCD"),
        (1, 4, "BCDE"),
        (2, 4, "CDEF"),
        (3, 4, "DEFG"),
    ] {
        raw(initialize, 400 + residue, length, want, false);
    }
    // Euro sign begins at byte 3 and crosses the cell boundary.
    raw("push.3796387654 mem_store.100 push.2206850 mem_store.101", 403, 3, "€", false);
    raw("", 400, 0, "", false);
    raw("", 4294967296, 0, "address should fit in u32", true);
    raw("", 4294967296, 524289, "address should fit in u32", true);
    raw("", 400, 524289, "string length 524289 exceeds maximum 524288", true);
    raw("", 400, 1, "memory is not initialized", true);
    raw("push.1094795585 mem_store.100", 415, 2, "memory is not initialized", true);
    raw("push.65 mem_store.100", 404, 1, "\0", false);
    raw("push.255 mem_store.100", 400, 1, "invalid UTF-8", true);
    raw(
        "push.1090519040 mem_store.1073741823 push.66 mem_store.1073741824",
        4294967295,
        2,
        "AB",
        false,
    );
    // All successful max-residue cells are initialized, including the final extra cell.
    raw(
        "push.100.131073 push.1 while.true swap.1 dup.0 push.1094795585 swap.1 mem_store add.1 \
         swap.1 sub.1 dup.0 neq.0 end drop drop",
        403,
        524288,
        &"A".repeat(524288),
        false,
    );

    // Enter the raw immediate event at exactly sixteen lanes: ptr/len plus fourteen
    // distinct protected values. Cleanup leaves the VM's two bottom padding zeros.
    let suffix = (0..14).map(|n| 901 + n).collect::<Vec<_>>();
    let mut inputs = vec![400, 2];
    inputs.extend(&suffix);
    let code = format!(
        "begin push.16961 mem_store.100 push.{id} emit drop drop drop {} assertz assertz end",
        crate::plan::tests::lowering_units::assert_top(&suffix)
    );
    assert_eq!(
        debug_println_inputs(&code, &[], 1, 0, &inputs),
        [PrintLnRecord(log::Level::Info, "AB".into())]
    );
    count += 1;
    // DefaultHost has no builtin handler for this ordinary event.
    let program = miden_assembly::Assembler::default()
        .assemble_program(
            "absent_println",
            format!("begin push.0.400 push.{id} emit drop drop drop end"),
        )
        .unwrap()
        .try_into_program()
        .unwrap();
    let error = miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut miden_processor::DefaultHost::default())
        .unwrap_err();
    match error {
        miden_processor::ExecutionError::EventError {
            event_id, error, ..
        } => {
            assert_eq!(event_id, crate::Event::PrintLn.as_event_id());
            assert_eq!(error.to_string(), "no event handler registered");
        }
        error => panic!("expected unhandled PrintLn dispatch: {error}"),
    }
    count += 1;
    // A panicking synchronous execution cannot leave a capture active for the next case.
    assert!(
        std::panic::catch_unwind(|| capture_println(|| panic!("capture cleanup probe"))).is_err()
    );
    assert!(capture_println(|| ()).1.is_empty());
    count += 1;
    count
}

#[test]
fn println_exact_tuples_effects_coverage_and_physical_violations() {
    use midenc_hir::{SourceSpan, Type};
    for mutation in 0..7 {
        let f = println_fixture(false);
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
            0 => op.borrow().operands().all()[0]
                .borrow()
                .as_value_ref()
                .borrow_mut()
                .set_type(Type::Felt),
            1 => op.borrow().operands().all()[0]
                .borrow()
                .as_value_ref()
                .borrow_mut()
                .set_type(midenc_hir::PointerType::new(Type::U32).into()),
            2 => op.borrow().operands().all()[0].borrow().as_value_ref().borrow_mut().set_type(
                midenc_hir::PointerType::new_with_address_space(
                    Type::U8,
                    midenc_hir::AddressSpace::Element,
                )
                .into(),
            ),
            3 => op.borrow().operands().all()[1]
                .borrow()
                .as_value_ref()
                .borrow_mut()
                .set_type(Type::Felt),
            4 => {
                op.borrow_mut().operands_mut().group_mut(0).erase(0);
            }
            5 => {
                let value = op.borrow().operands().all()[0].borrow().as_value_ref();
                let operand = f.context.make_operand(value, op, 0);
                op.borrow_mut().operands_mut().push(operand);
            }
            _ => {
                let result = f.context.make_result(SourceSpan::UNKNOWN, Type::Felt, op, 0);
                op.borrow_mut().results_mut().push(result);
            }
        }
        assert!(
            select_function(&f.borrow(), &SelectEnv::default()).is_err(),
            "tuple mutant{mutation}"
        );
    }
    let f = println_fixture(false);
    let function = f.borrow();
    let env = SelectEnv::default();
    let selected = select_function(&function, &env).unwrap();
    for mutation in 0..6 {
        let mut bad = selected.clone();
        match mutation {
            0 => bad.segments[0].units.clear(),
            1 => {
                let unit = bad.segments[0].units[0].clone();
                bad.segments[0].units.push(unit);
            }
            2 => bad.segments[0].units[0].stack_inputs.reverse(),
            3 => bad.segments[0].units[0].effects.memory = SourceMemoryEffects::default(),
            4 => bad.segments[0].units[0].effects.may_trap = false,
            _ => bad.source_map.clear(),
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
            "source mutant{mutation}"
        );
    }
    for mutation in 0..3 {
        let (plan, frame, mut body) = ready(&f, &env);
        let index = body.ops().iter().position(|op| matches!(op, AnnotatedOp::Instruction { instruction: crate::masm::Instruction::EmitImm(event), .. } if *event == crate::Event::PrintLn.into())).unwrap();
        let unit = match body.ops()[index] {
            AnnotatedOp::Instruction { unit, .. } => unit,
            _ => unreachable!(),
        };
        match mutation {
            0 => body.ops_mut().insert(
                index,
                AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Swap2,
                    unit,
                },
            ),
            1 => {
                body.ops_mut().remove(index + 1);
            }
            _ => body.ops_mut().insert(
                index,
                AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Locaddr(0.into()),
                    unit,
                },
            ),
        }
        assert!(verify::verify(&plan, &frame, body).is_err(), "physical mutant{mutation}");
    }
}

fn println_selected_host_cases() -> usize {
    let mut count = 0;
    for live in [false, true] {
        let f = println_fixture(live);
        let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
        let intrinsics = crate::intrinsics::load(f.context.session()).unwrap();
        for force in [false, true] {
            let (_, frame, _) = fri_ready(&f, force);
            if force {
                assert!(!frame.spills().is_empty());
            }
            let procedure = system_event_procedure(&f, force, |_| {});
            for (initialize, expected, warning) in [
                ("push.16961 mem_store.100", "AB", false),
                ("push.255 mem_store.100", "invalid UTF-8", true),
            ] {
                let suffix = (0..20).map(|n| 901 + n).collect::<Vec<_>>();
                let mut output = if live { vec![400, 2] } else { vec![] };
                output.extend(&suffix);
                // Forced normalization may create physical floor padding below this suffix;
                // the complete twenty meaningful suffix lanes are still asserted in order.
                let code = format!(
                    "{} begin {initialize} {} push.2.400 exec.selected {} push.73 push.73 \
                     assert_eq exec.::miden::core::sys::truncate_stack end",
                    procedure.text,
                    crate::plan::tests::lowering_units::push_top(&suffix),
                    crate::plan::tests::lowering_units::assert_top(&output)
                );
                let records =
                    debug_println_packages(&code, &[core.clone(), intrinsics.clone()], 1, 0);
                assert_eq!(records.len(), 1);
                assert_eq!(
                    records[0].0,
                    if warning {
                        log::Level::Warn
                    } else {
                        log::Level::Info
                    }
                );
                assert!(records[0].1.contains(expected), "{:?}", records[0]);
                count += 1;
            }
            if !live {
                let suffix = (0..14).map(|n| 901 + n).collect::<Vec<_>>();
                let mut inputs = vec![400, 2];
                inputs.extend(&suffix);
                let code = format!(
                    "{} begin push.16961 mem_store.100 exec.selected {} assertz assertz \
                     exec.::miden::core::sys::truncate_stack end",
                    procedure.text,
                    crate::plan::tests::lowering_units::assert_top(&suffix)
                );
                assert_eq!(
                    debug_println_inputs(&code, &[core.clone(), intrinsics.clone()], 1, 0, &inputs),
                    [PrintLnRecord(log::Level::Info, "AB".into())]
                );
                count += 1;
                // These substitutions remain physically legal; only actual logs catch them.
                for mutation in 0..4 {
                    let procedure = system_event_procedure(&f, force, |body| {
                        let index = body.ops().iter().position(|op| matches!(op, AnnotatedOp::Instruction { instruction: crate::masm::Instruction::EmitImm(event), .. } if *event == crate::Event::PrintLn.into())).unwrap();
                        let unit = match body.ops()[index] {
                            AnnotatedOp::Instruction { unit, .. } => unit,
                            _ => unreachable!(),
                        };
                        match mutation {
                            0 => {
                                body.ops_mut().remove(index);
                            }
                            1 => {
                                body.ops_mut()[index] = AnnotatedOp::Instruction {
                                    instruction: crate::masm::Instruction::EmitImm(
                                        crate::Event::FrameStart.into(),
                                    ),
                                    unit,
                                }
                            }
                            2 => body.ops_mut().insert(
                                index,
                                AnnotatedOp::Instruction {
                                    instruction: crate::masm::Instruction::AddImm(
                                        miden_core::Felt::ONE.into(),
                                    ),
                                    unit,
                                },
                            ),
                            _ => {
                                body.ops_mut().insert(
                                    index,
                                    AnnotatedOp::Instruction {
                                        instruction: crate::masm::Instruction::Swap1,
                                        unit,
                                    },
                                );
                                body.ops_mut().insert(
                                    index + 1,
                                    AnnotatedOp::Instruction {
                                        instruction: crate::masm::Instruction::AddImm(
                                            miden_core::Felt::ONE.into(),
                                        ),
                                        unit,
                                    },
                                );
                                body.ops_mut().insert(
                                    index + 2,
                                    AnnotatedOp::Instruction {
                                        instruction: crate::masm::Instruction::Swap1,
                                        unit,
                                    },
                                );
                            }
                        }
                    });
                    let code = format!(
                        "{} begin push.1145258561 mem_store.100 push.2.400 exec.selected \
                         exec.::miden::core::sys::truncate_stack end",
                        procedure.text
                    );
                    let records = debug_println_packages(
                        &code,
                        &[core.clone(), intrinsics.clone()],
                        usize::from(mutation >= 2),
                        usize::from(mutation == 1),
                    );
                    assert_ne!(
                        records,
                        [PrintLnRecord(log::Level::Info, "AB".into())],
                        "semantic mutant{mutation}"
                    );
                    count += 1;
                }
            }
        }
    }
    count
}

#[test]
fn println_real_native_address_casts_retain_opaque_source_memory_abi() {
    let mut f = parse(
        "builtin.function public extern(\"C\") @print_local() { %n = arith.constant 524288 : u32; \
         %native = hir.local_address <{ local = #builtin.local_variable<3,felt> }>; %bits = \
         hir.ptr_to_int %native <{ ty = #builtin.type<u32> }>; %bytes = hir.int_to_ptr %bits <{ \
         ty = #builtin.type<ptr<u8,byte>> }>; hir.print_ln %bytes,%n : (ptr<u8,byte>,u32); \
         builtin.ret; };",
    );
    for _ in 0..4 {
        f.function.borrow_mut().alloc_local(midenc_hir::Type::Felt);
    }
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.source().extent, 4);
    verify::verify(&plan, &frame, body).unwrap();
}

fn println_ordered_host_cases() -> usize {
    let f = parse(
        "builtin.function public extern(\"C\") @two_prints(%p: ptr<u8,byte>,%n: u32,%q: \
         ptr<u8,byte>,%m: u32) { hir.print_ln %p,%n : (ptr<u8,byte>,u32); hir.print_ln %q,%m : \
         (ptr<u8,byte>,u32); builtin.ret; };",
    );
    let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
    let intrinsics = crate::intrinsics::load(f.context.session()).unwrap();
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, |_| {});
        let code = format!(
            "{} begin push.16961 mem_store.100 push.17475 mem_store.101 push.2.404.2.400 \
             exec.selected exec.::miden::core::sys::truncate_stack end",
            procedure.text
        );
        assert_eq!(
            debug_println_packages(&code, &[core.clone(), intrinsics.clone()], 2, 0),
            [
                PrintLnRecord(log::Level::Info, "AB".into()),
                PrintLnRecord(log::Level::Info, "CD".into())
            ]
        );
    }
    2
}

pub(crate) fn generic_event_fixture(immediate: bool) -> Parsed {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    let types = vec![Type::Felt];
    let mut test = midenc_hir::testing::Test::new("generic", &types, &types);
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
        let mut b = test.function_builder();
        if immediate {
            b.emit_event_imm(miden_core::Felt::from(700001u32), SourceSpan::UNKNOWN)
                .unwrap();
            b.ret(args, SourceSpan::UNKNOWN).unwrap();
        } else {
            let result = b.emit_event(args[0], SourceSpan::UNKNOWN).unwrap();
            b.ret([result], SourceSpan::UNKNOWN).unwrap();
        }
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn generic_event_real_hir_selection_retains_only_dynamic_id() {
    for immediate in [false, true] {
        let f = generic_event_fixture(immediate);
        let selected = checked(&f, &SelectEnv::default());
        let unit = &selected.segments[0].units[0];
        assert_eq!(unit.stack_inputs.len(), usize::from(!immediate));
        assert_eq!(unit.outputs.len(), usize::from(!immediate));
        assert!(unit.effects.may_trap);
        assert_eq!(unit.effects.memory, SourceMemoryEffects::default());
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        verify::verify(&plan, &frame, body).unwrap();
    }
}

#[test]
fn generic_event_exact_tuples_diagnose_without_panicking() {
    use midenc_hir::{Immediate, SourceSpan, Type, Value};
    for immediate in [false, true] {
        for mutation in 0..5 {
            let f = generic_event_fixture(immediate);
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
                0 if immediate => {
                    *op.borrow_mut()
                        .downcast_mut::<midenc_dialect_hir::EmitEventImm>()
                        .unwrap()
                        .get_event_id_mut() = Immediate::U32(7)
                }
                0 => op.borrow().operands().all()[0]
                    .borrow()
                    .as_value_ref()
                    .borrow_mut()
                    .set_type(Type::U32),
                1 if !immediate => {
                    let mut result = op.borrow().results().all()[0];
                    result.borrow_mut().set_type(Type::U32);
                }
                1 | 2 if immediate => {
                    let arg =
                        f.borrow().entry_block().borrow().arguments()[0] as midenc_hir::ValueRef;
                    let operand = f.context.make_operand(arg, op, 0);
                    op.borrow_mut().operands_mut().push(operand);
                }
                2 => {
                    op.borrow_mut().operands_mut().group_mut(0).erase(0);
                }
                3 if !immediate => {
                    op.borrow_mut().results_mut().group_mut(0).erase(0);
                }
                _ => {
                    let result = f.context.make_result(SourceSpan::UNKNOWN, Type::Felt, op, 1);
                    op.borrow_mut().results_mut().push(result);
                }
            }
            assert!(
                select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                "immediate{immediate} mutation{mutation}"
            );
        }
    }
}

#[test]
fn generic_event_actual_host_generated_state_suffix_spills_and_advice() {
    use miden_processor::event::EventName;

    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let name = EventName::new("test::generic_generated");
    let id = name.to_event_id().as_u64() as usize;
    for immediate in [false, true] {
        let f = generic_event_fixture(immediate);
        if immediate {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            *op.borrow_mut()
                .downcast_mut::<midenc_dialect_hir::EmitEventImm>()
                .unwrap()
                .get_event_id_mut() = midenc_hir::Immediate::Felt(name.to_event_id().as_felt());
        }
        for force in [false, true] {
            let (_, frame, _) = fri_ready(&f, force);
            assert_eq!(!frame.spills().is_empty(), force);
            let procedure = system_event_procedure(&f, force, |_| {});
            let core =
                midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
            for suffix_len in [0, 15, 36] {
                let suffix = (0..suffix_len).map(|n| 901 + n).collect::<Vec<_>>();
                let mut observed = vec![id];
                if immediate && !force {
                    observed.push(id);
                }
                observed.extend(&suffix);
                observed.extend([0; 16]);
                let response = observed.get(17).copied().unwrap_or(0);
                let exported_address = if force {
                    miden_core::FMP_INIT_VALUE.as_canonical_u64()
                } else {
                    0
                };
                let mut expected = vec![id];
                expected.extend(&suffix);
                let code = format!(
                    "{} begin push.4242 mem_store.100 {} push.{id} exec.selected adv_push \
                     push.{response} assert_eq adv_push push.4242 assert_eq adv_push \
                     push.{exported_address} assert_eq {} mem_load.100 push.4242 assert_eq \
                     push.73 push.73 assert_eq exec.::miden::core::sys::truncate_stack end",
                    procedure.text,
                    push_top(&suffix),
                    assert_top(&expected)
                );
                let mut assembler = miden_assembly::Assembler::default();
                assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
                let program = assembler
                    .assemble_program("generic_capture", code)
                    .unwrap()
                    .try_into_program()
                    .unwrap();
                let (mut host, records) = generic_event_host(core::slice::from_ref(&name));
                host.load_library(miden_processor::HostLibrary::from(core.clone())).unwrap();
                miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                    .execute_sync(&program, &mut host)
                    .unwrap();
                let records = records.lock().unwrap();
                assert_eq!(records.len(), 1);
                let record = &records[0];
                assert_eq!(record.id, id as u64);
                assert_eq!(record.context, 0);
                assert!(record.clock > 0);
                assert_eq!(record.stack, observed.iter().map(|n| *n as u64).collect::<Vec<_>>());
                assert!(record.memory.contains(&(100, 4242)));
                // The assembler initializes its reserved frame-pointer word in root memory.
                let fmp_address = miden_core::FMP_ADDR.as_canonical_u64() as u32;
                let base = miden_core::FMP_INIT_VALUE.as_canonical_u64() as u32;
                let mut expected_memory = vec![(100, 4242), (101, 0), (102, 0), (103, 0)];
                if force {
                    assert_eq!(frame.aligned_size(), 4);
                    expected_memory.extend([
                        (base, id as u64),
                        (base + 1, 0),
                        (base + 2, 0),
                        (base + 3, 0),
                    ]);
                }
                expected_memory.extend([
                    (fmp_address - 2, 0),
                    (fmp_address - 1, 0),
                    (fmp_address, u64::from(base + frame.aligned_size())),
                    (fmp_address + 1, 0),
                ]);
                assert_eq!(record.memory, expected_memory);
            }
        }
    }
}

#[test]
fn generic_event_vm_owns_known_dispatch_and_unknown_failure() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let id = miden_core::events::SystemEvent::HasMapKey.event_id().as_felt();
    for immediate in [false, true] {
        let f = generic_event_fixture(immediate);
        if immediate {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            *op.borrow_mut()
                .downcast_mut::<midenc_dialect_hir::EmitEventImm>()
                .unwrap()
                .get_event_id_mut() = midenc_hir::Immediate::Felt(id);
        }
        for force in [false, true] {
            // Force spilling is observable: immediate payloads are generated state, so use
            // dynamic dispatch for that mode and leave the unchanged key in caller overflow.
            if immediate && force {
                continue;
            }
            let procedure = system_event_procedure(&f, force, |_| {});
            let suffix = (11..31).collect::<Vec<_>>();
            let argument = if immediate {
                10
            } else {
                id.as_canonical_u64() as usize
            };
            let mut expected = vec![argument];
            expected.extend(&suffix);
            let core =
                midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
            let mut assembler = miden_assembly::Assembler::default();
            assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
            let program = assembler
                .assemble_program(
                    "generic_known",
                    format!(
                        "{} begin {} push.{argument} exec.selected adv_push assertz {} \
                         exec.::miden::core::sys::truncate_stack end",
                        procedure.text,
                        push_top(&suffix),
                        assert_top(&expected)
                    ),
                )
                .unwrap()
                .try_into_program()
                .unwrap();
            let mut host = miden_processor::DefaultHost::default();
            host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
            miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                .execute_sync(&program, &mut host)
                .unwrap();
        }
    }
    let f = generic_event_fixture(true);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    let program = miden_assembly::Assembler::default()
        .assemble_program("generic_unknown", format!("{procedure} begin push.9 exec.selected end"))
        .unwrap()
        .try_into_program()
        .unwrap();
    let error = miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut miden_processor::DefaultHost::default())
        .unwrap_err();
    assert!(
        matches!(error, miden_processor::ExecutionError::EventError { event_id, .. } if event_id.as_u64() == 700001)
    );
}

#[test]
fn generic_event_semantic_omission_duplication_and_wrong_immediate_are_vm_obligations() {
    use miden_processor::event::EventName;
    let name = EventName::new("test::generic_semantic");
    let other = EventName::new("test::generic_wrong");
    let id = name.to_event_id().as_u64();
    for immediate in [false, true] {
        let f = generic_event_fixture(immediate);
        if immediate {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            *op.borrow_mut()
                .downcast_mut::<midenc_dialect_hir::EmitEventImm>()
                .unwrap()
                .get_event_id_mut() = midenc_hir::Immediate::Felt(name.to_event_id().as_felt());
        }
        for force in [false, true] {
            for mutation in 0..if immediate { 4 } else { 3 } {
                let procedure = system_event_procedure(&f, force, |body| {
                    let index = body
                        .ops()
                        .iter()
                        .position(|op| {
                            matches!(
                                op,
                                AnnotatedOp::Instruction {
                                    instruction: crate::masm::Instruction::Emit
                                        | crate::masm::Instruction::EmitImm(_),
                                    ..
                                }
                            )
                        })
                        .unwrap();
                    let original = body.ops()[index].clone();
                    match mutation {
                        0 => {}
                        1 => {
                            // Retain the required executable anchor for an otherwise empty procedure.
                            let unit = match original {
                                AnnotatedOp::Instruction { unit, .. } => unit,
                                _ => unreachable!(),
                            };
                            body.ops_mut()[index] = AnnotatedOp::Instruction {
                                instruction: crate::masm::Instruction::Nop,
                                unit,
                            };
                        }
                        2 => {
                            body.ops_mut().insert(index, original);
                        }
                        _ => {
                            let unit = match original {
                                AnnotatedOp::Instruction { unit, .. } => unit,
                                _ => unreachable!(),
                            };
                            body.ops_mut()[index] = AnnotatedOp::Instruction {
                                instruction: crate::masm::Instruction::EmitImm(
                                    crate::masm::EventImmediate::Immediate(
                                        other.to_event_id().as_felt().into(),
                                    ),
                                ),
                                unit,
                            };
                        }
                    }
                }); // All mutations intentionally pass independent physical verification.
                let core =
                    midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
                let mut assembler = miden_assembly::Assembler::default();
                assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
                let program = assembler
                    .assemble_program(
                        "generic_semantic",
                        format!(
                            "{} begin push.4242 mem_store.100 push.{id} exec.selected push.{id} \
                             assert_eq mem_load.100 push.4242 assert_eq \
                             exec.::miden::core::sys::truncate_stack end",
                            procedure.text
                        ),
                    )
                    .unwrap()
                    .try_into_program()
                    .unwrap();
                let (mut host, records) = generic_event_host(&[name.clone(), other.clone()]);
                host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
                miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                    .execute_sync(&program, &mut host)
                    .unwrap();
                let ids = records.lock().unwrap().iter().map(|r| r.id).collect::<Vec<_>>();
                let expected = match mutation {
                    0 => vec![id],
                    1 => vec![],
                    2 => vec![id, id],
                    _ => vec![other.to_event_id().as_u64()],
                };
                assert_eq!(ids, expected);
                assert_eq!(ids == [id], mutation == 0, "semantic mutation{mutation}");
            }
        }
    }
}

#[test]
fn generic_event_effect_order_across_advice_memory_and_structured_control() {
    use miden_processor::event::EventName;

    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let name = EventName::new("test::generic_order");
    let id = name.to_event_id().as_u64() as usize;
    let f = parse(
        "builtin.function public extern(\"C\") @generic_order(%id: felt,%p: \
         ptr<felt,element>,%flag: i1,%next: felt) -> felt { %first=hir.emit_event %id; \
         %ignored=hir.advice_pop; %value=hir.advice_pop; hir.store %p,%next : \
         (ptr<felt,element>,felt); scf.if %flag then { %inside=hir.emit_event %id; scf.yield : \
         (); } else { scf.yield : (); } : (i1) -> (); %last=hir.emit_event %id; builtin.ret \
         %value : (felt); };",
    );
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, crate::opt::peephole::simplify_annotated);
        let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
        for flag in [0, 1] {
            let suffix = (901..921).collect::<Vec<_>>();
            let mut expected = vec![4242];
            expected.extend(&suffix);
            let mut assembler = miden_assembly::Assembler::default();
            assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
            let program = assembler
                .assemble_program(
                    "generic_order",
                    format!(
                        "{} begin push.4242 mem_store.100 {} push.5151.{flag}.100.{id} \
                         exec.selected {} mem_load.100 push.5151 assert_eq \
                         exec.::miden::core::sys::truncate_stack end",
                        procedure.text,
                        push_top(&suffix),
                        assert_top(&expected)
                    ),
                )
                .unwrap()
                .try_into_program()
                .unwrap();
            let (mut host, records) = generic_event_host(core::slice::from_ref(&name));
            host.load_library(miden_processor::HostLibrary::from(core.clone())).unwrap();
            miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                .execute_sync(&program, &mut host)
                .unwrap();
            let records = records.lock().unwrap();
            assert_eq!(records.len(), 2 + flag);
            assert!(records.iter().all(|r| r.id == id as u64));
            assert!(records.windows(2).all(|pair| pair[0].clock < pair[1].clock));
            let memory = records
                .iter()
                .map(|r| r.memory.iter().find(|(a, _)| *a == 100).unwrap().1)
                .collect::<Vec<_>>();
            assert_eq!(
                memory,
                if flag == 0 {
                    vec![4242, 5151]
                } else {
                    vec![4242, 5151, 5151]
                }
            );
        }
    }
}

#[test]
fn generic_event_minimum_floor_observes_actual_transient_id_and_padding() {
    use miden_processor::event::EventName;
    let name = EventName::new("test::generic_floor");
    let id = name.to_event_id().as_u64();
    let input = core::iter::once(id).chain(901..916).collect::<Vec<_>>();
    for immediate in [false, true] {
        let f = generic_event_fixture(immediate);
        if immediate {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            *op.borrow_mut()
                .downcast_mut::<midenc_dialect_hir::EmitEventImm>()
                .unwrap()
                .get_event_id_mut() = midenc_hir::Immediate::Felt(name.to_event_id().as_felt());
        }
        for force in [false, true] {
            let procedure = system_event_procedure(&f, force, |_| {});
            let core =
                midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
            let mut assembler = miden_assembly::Assembler::default();
            assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
            let checks = input
                .iter()
                .map(|v| format!("push.{v} assert_eq"))
                .collect::<Vec<_>>()
                .join(" ");
            let program = assembler
                .assemble_program(
                    "generic_floor",
                    format!(
                        "{} begin push.4242 mem_store.100 exec.selected sdepth push.{} assert_eq \
                         adv_push assertz adv_push push.4242 assert_eq {checks} {} \
                         exec.::miden::core::sys::truncate_stack end",
                        procedure.text,
                        16 + usize::from(force),
                        if force { "assertz" } else { "" }
                    ),
                )
                .unwrap()
                .try_into_program()
                .unwrap();
            let (mut host, records) = generic_event_host(core::slice::from_ref(&name));
            host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
            let inputs =
                input.iter().map(|v| miden_core::Felt::new_unchecked(*v)).collect::<Vec<_>>();
            miden_processor::FastProcessor::new(
                miden_processor::StackInputs::new(&inputs).unwrap(),
            )
            .execute_sync(&program, &mut host)
            .unwrap();
            let mut expected = if immediate && !force {
                core::iter::once(id).chain(input.clone()).collect::<Vec<_>>()
            } else {
                input.clone()
            };
            if force {
                expected.push(0);
            }
            assert_eq!(records.lock().unwrap()[0].stack, expected);
        }
    }
}

#[test]
fn generic_event_loop_occurrence_order_and_continuation() {
    use miden_processor::event::EventName;

    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let names = [
        EventName::new("test::generic_loop_first"),
        EventName::new("test::generic_loop_second"),
    ];
    let [first, second] = names.each_ref().map(|name| name.to_event_id().as_u64());
    let f = parse(
        "builtin.function public extern(\"C\") @generic_loop(%a: felt,%b: felt,%n: u32,%live: \
         felt) -> (felt,felt,felt,u32) { %entry=hir.emit_event %a; %done=scf.while %n before { \
         ^before(%remaining: u32): %zero=arith.constant 0 : u32; %more=arith.neq \
         %remaining,%zero; scf.condition %more,%remaining : (i1,u32); } after { ^after(%counter: \
         u32): %first=hir.emit_event %b; %second=hir.emit_event %a; %one=arith.constant 1 : u32; \
         %next=arith.sub %counter,%one <{ overflow=#builtin.overflow<wrapping> }>; scf.yield \
         %next : (u32); } : (u32) -> u32; %exit=hir.emit_event %b; builtin.ret %a,%b,%live,%done \
         : (felt,felt,felt,u32); };",
    );
    for force in [false, true] {
        let procedure = system_event_procedure(&f, force, crate::opt::peephole::simplify_annotated);
        let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
        for iterations in [0, 3] {
            let suffix = (901..921).collect::<Vec<_>>();
            let mut expected_state = vec![first as usize, second as usize, 7373, 0];
            expected_state.extend(&suffix);
            let mut assembler = miden_assembly::Assembler::default();
            assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
            let program = assembler
                .assemble_program(
                    "generic_loop",
                    format!(
                        "{} begin push.4242 mem_store.100 {} \
                         push.7373.{iterations}.{second}.{first} exec.selected {} mem_load.100 \
                         push.4242 assert_eq push.73 push.73 assert_eq \
                         exec.::miden::core::sys::truncate_stack end",
                        procedure.text,
                        push_top(&suffix),
                        assert_top(&expected_state)
                    ),
                )
                .unwrap()
                .try_into_program()
                .unwrap();
            let (mut host, records) = generic_event_host(&names);
            host.load_library(miden_processor::HostLibrary::from(core.clone())).unwrap();
            miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                .execute_sync(&program, &mut host)
                .unwrap();
            let records = records.lock().unwrap();
            let mut expected_ids = vec![first];
            for _ in 0..iterations {
                expected_ids.extend([second, first]);
            }
            expected_ids.push(second);
            assert_eq!(records.iter().map(|record| record.id).collect::<Vec<_>>(), expected_ids);
            assert_eq!(records.len(), 2 + 2 * iterations);
            assert!(records.windows(2).all(|pair| pair[0].clock < pair[1].clock));
            assert!(records.iter().all(|record| record.memory.contains(&(100, 4242))));
        }
    }
}
