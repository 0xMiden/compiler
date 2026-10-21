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
            matches!(name, "horner_base" | "horner_ext" | "mem_stream" | "crypto_stream")
        );
        assert_eq!(
            source_unit.effects.memory.write,
            matches!(name, "advice_pipe" | "crypto_stream")
        );
        let aliases = match n {
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
