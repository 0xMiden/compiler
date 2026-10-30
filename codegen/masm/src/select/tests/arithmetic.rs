use alloc::string::String;

use midenc_hir::Value;

use super::*;

fn signed_minmax_cases(ty: &str, width: usize, op: &str) {
    let (min, max) = if width == 1 {
        (i32::MIN as i64, i32::MAX as i64)
    } else {
        (i64::MIN, i64::MAX)
    };
    let limbs = |value: i64| {
        (0..width)
            .map(|lane| (((value as u64) >> (lane * 32)) as u32) as usize)
            .collect::<Vec<_>>()
    };
    let mut cases = vec![(min, max), (min, -1), (-7, -3), (-1, 0), (0, 0), (7, 7), (3, 5)];
    if width == 2 {
        cases.extend([
            (0x12345678_abcdef01, 0x12345678_abcdef02),
            (-0x12345678_abcdef01, -0x12345679_01234567),
        ]);
    }
    for reverse in [false, true] {
        let args = if reverse { "%b,%a" } else { "%a,%b" };
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> ({ty},{ty}) {{              %r=arith.{op} {args}; builtin.ret %r,%a : ({ty},{ty}); }};"
        ));
        for &(a, b) in &cases {
            for (a, b) in [(a, b), (b, a)] {
                let mut inputs = limbs(a);
                inputs.extend(limbs(b));
                let mut expected = limbs(if op == "min" { a.min(b) } else { a.max(b) });
                expected.extend(limbs(a));
                execute_linked(&f, &inputs, &expected);
            }
        }
    }
}

#[test]
fn signed_i32_min_preserves_values_and_suffix() {
    signed_minmax_cases("i32", 1, "min");
}

#[test]
fn signed_i32_max_preserves_values_and_suffix() {
    signed_minmax_cases("i32", 1, "max");
}

#[test]
fn signed_i64_min_preserves_values_and_suffix() {
    signed_minmax_cases("i64", 2, "min");
}

#[test]
fn signed_i64_max_preserves_values_and_suffix() {
    signed_minmax_cases("i64", 2, "max");
}

/// Execute only the generated arithmetic and stack cleanup, so expected source traps cannot
/// be confused with assertions made by the result oracle.
fn arithmetic_runtime_result(
    f: &Parsed,
    inputs: &[usize],
) -> Result<(), miden_processor::ExecutionError> {
    let procedure = selected_procedure(f, &SelectEnv::default());
    let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
    let intrinsics = crate::intrinsics::load(f.context.session()).unwrap();
    let suffix = (900..920).collect::<Vec<_>>();
    let code = format!(
        "{procedure} begin {} {} exec.selected exec.::miden::core::sys::truncate_stack end",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::push_top(inputs)
    );
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    assembler
        .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
        .unwrap();
    let program = assembler
        .assemble_program("arithmetic_trap_probe", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host();
    host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
    host.load_library(miden_processor::HostLibrary::from(intrinsics)).unwrap();
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host)
        .map(|_| ())
}

#[test]
fn signed_minmax_rejects_each_invalid_limb() {
    for (ty, width) in [("i32", 1), ("i64", 2)] {
        for op in ["min", "max"] {
            let f = parse(&format!(
                "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> {ty} {{                  %r=arith.{op} %a,%b; builtin.ret %r : ({ty}); }};"
            ));
            for lane in 0..2 * width {
                let mut inputs = vec![0; 2 * width];
                inputs[lane] = u32::MAX as usize + 1;
                let error = arithmetic_runtime_result(&f, &inputs).unwrap_err();
                assert!(
                    matches!(
                        error,
                        miden_processor::ExecutionError::OperationError {
                            err: miden_processor::operation::OperationError::NotU32Values { .. },
                            ..
                        }
                    ),
                    "{ty}/{op}/lane {lane}: {error}"
                );
            }
        }
    }
}

#[test]
fn narrow_bnot_preserves_declared_width_and_live_input() {
    for (ty, bits) in [("u8", 8), ("i8", 8), ("u16", 16), ("i16", 16)] {
        let mask = (1usize << bits) - 1;
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {ty}) -> ({ty}, {ty}) {{ %r = \
             arith.bnot %a; builtin.ret %r, %a : ({ty}, {ty}); }};"
        ));
        for input in [0, 1, 0x0f, 1 << (bits - 1), mask - 1, mask] {
            execute_linked(&f, &[input], &[mask ^ input, input]);
        }
    }
}

#[test]
fn full_limb_bnot_preserves_limb_order_and_live_input() {
    for (ty, input) in [
        ("u32", vec![0x1234_5678]),
        ("u64", vec![0x1234_5678, 0x9abc_def0]),
        ("u128", vec![0x1234_5678, 0x9abc_def0, 0, 0xffff_ffff]),
    ] {
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {ty}) -> ({ty}, {ty}) {{ %r = \
             arith.bnot %a; builtin.ret %r, %a : ({ty}, {ty}); }};"
        ));
        let mut expected = input.iter().map(|limb| 0xffff_ffff ^ limb).collect::<Vec<_>>();
        expected.extend_from_slice(&input);
        execute_linked(&f, &input, &expected);
    }
}

#[test]
fn narrow_bnot_stores_preserve_neighboring_bytes() {
    let sentinel = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let initialize = format!(
        "push.{} mem_store.64 push.{} mem_store.65",
        u32::from_le_bytes(sentinel[..4].try_into().unwrap()),
        u32::from_le_bytes(sentinel[4..].try_into().unwrap()),
    );
    for (ty, bits) in [("u8", 8), ("i8", 8), ("u16", 16), ("i16", 16)] {
        let mask = (1usize << bits) - 1;
        for offset in 0..4 {
            for constant_pointer in [false, true] {
                let (args, pointer) = if constant_pointer {
                    (
                        format!("%a: {ty}"),
                        format!("%p = hir.constant_pointer {} : ptr<{ty}, byte>;", 256 + offset),
                    )
                } else {
                    (format!("%p: ptr<{ty}, byte>, %a: {ty}"), String::new())
                };
                let f = parse(&format!(
                    "builtin.function public extern(\"C\") @f({args}) -> {ty} {{ {pointer} %r = \
                     arith.bnot %a; hir.store %p, %r : (ptr<{ty}, byte>, {ty}); builtin.ret %r : \
                     ({ty}); }};"
                ));
                for input in [0, 0x0f, 1 << (bits - 1), mask] {
                    let result = mask ^ input;
                    let mut bytes = sentinel;
                    bytes[offset..offset + bits / 8]
                        .copy_from_slice(&result.to_le_bytes()[..bits / 8]);
                    let observe = format!(
                        "mem_load.64 push.{} assert_eq mem_load.65 push.{} assert_eq",
                        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                        u32::from_le_bytes(bytes[4..].try_into().unwrap()),
                    );
                    let inputs = if constant_pointer {
                        vec![input]
                    } else {
                        vec![256 + offset, input]
                    };
                    execute_linked_memory(&f, &inputs, &[result], &initialize, &observe).unwrap();
                }
            }
        }
    }
}

#[test]
fn registered_arithmetic_selects_real_source_and_preserves_suffix() {
    let f = parse(
        "builtin.function public extern(\"C\") @add(%a: u32, %b: u32) -> u32 {\n%r = arith.add \
         %a, %b <{ overflow = #builtin.overflow<wrapping> }>;\nbuiltin.ret %r : (u32); };",
    );
    execute(&f, &SelectEnv::default(), &[17, 29], &[46]);
}

#[test]
fn commutative_whole_values_use_resident_operand_order() {
    for ty in ["felt", "u32", "u64", "u128"] {
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {ty}, %b: {ty}) -> {ty} {{ %r = \
             arith.add %a, %b <{{ overflow = #builtin.overflow<wrapping> }}>; builtin.ret %r : \
             ({ty}); }};"
        ));
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        let actions = plan.segment_actions.values().next().unwrap();
        assert_eq!(plan.lowering_bindings[&SelectedStepId(0)].input_order, InputOrder::Reversed);
        assert!(
            matches!(actions.first(), Some(crate::plan::actions::PlanAction::RunLowering { .. })),
            "{ty}: resident complete operands need no preparation: {actions:?}"
        );
        verify::verify(&plan, &frame, body).unwrap();
    }
}

#[test]
fn commutative_unsigned_arithmetic_preserves_all_modes_and_live_sources() {
    fn limbs(value: u128, width: usize) -> Vec<usize> {
        (0..width).map(|lane| ((value >> (32 * lane)) as u32) as usize).collect()
    }
    for (ty, width) in [("u32", 1), ("u64", 2), ("u128", 4)] {
        let mask = if width == 4 {
            u128::MAX
        } else {
            (1u128 << (width * 32)) - 1
        };
        let distinct = 0x12345678_9abcdef0_10203040_50607080u128 & mask;
        for op in ["add", "mul"] {
            for mode in ["unchecked", "wrapping", "checked", "overflowing"] {
                for reverse in [false, true] {
                    let args = if reverse { "%b,%a" } else { "%a,%b" };
                    let (operation, property, result, tuple) = if mode == "overflowing" {
                        (format!("{op}_overflowing"), String::new(), "%flag,%r", format!("i1,{ty}"))
                    } else {
                        (
                            op.into(),
                            format!(" <{{ overflow=#builtin.overflow<{mode}> }}>"),
                            "%r",
                            ty.into(),
                        )
                    };
                    let f = parse(&format!(
                        "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> \
                         ({tuple},{ty}) {{ {result}=arith.{operation} {args}{property}; \
                         builtin.ret {result},%a : ({tuple},{ty}); }};"
                    ));
                    for (a, b) in [(3, distinct), (mask, 1), (mask, mask), (u32::MAX as u128, 2)] {
                        let (raw, carry) = if op == "add" {
                            a.overflowing_add(b)
                        } else {
                            a.overflowing_mul(b)
                        };
                        let overflow = carry || raw > mask;
                        // Unchecked arithmetic requires the caller to keep the result in range.
                        if mode == "unchecked" && overflow {
                            continue;
                        }
                        let mut inputs = limbs(a, width);
                        inputs.extend(limbs(b, width));
                        let mut expected = Vec::new();
                        if mode == "overflowing" {
                            expected.push(usize::from(overflow));
                        }
                        expected.extend(limbs(raw & mask, width));
                        expected.extend(limbs(a, width));
                        if mode == "checked" && overflow {
                            let error = arithmetic_runtime_result(&f, &inputs).unwrap_err();
                            let expected_trap = match &error {
                                miden_processor::ExecutionError::OperationError { err, .. } => match err {
                                    miden_processor::operation::OperationError::NotU32Values { values } if width == 1 => {
                                        values.as_slice() == [miden_core::Felt::new_unchecked(raw as u64)]
                                    }
                                    miden_processor::operation::OperationError::FailedAssertion { .. } => width > 1,
                                    _ => false,
                                },
                                _ => false,
                            };
                            assert!(expected_trap, "{ty}/{op}/{mode}/{reverse}: {error}");
                        } else {
                            execute_linked_memory(&f, &inputs, &expected, "", "").unwrap_or_else(
                                |error| panic!("{ty}/{op}/{mode}/{reverse}: {error}"),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn commutative_comparisons_and_bitwise_helpers_preserve_each_limb() {
    for (ty, width) in [("u32", 1), ("u64", 2), ("u128", 4)] {
        let mask = if width == 4 {
            u128::MAX
        } else {
            (1u128 << (width * 32)) - 1
        };
        let a = 0x01234567_89abcdef_fedcba98_76543210u128 & mask;
        let b = 0x89abcdef_10203040_abcdef01_20304050u128 & mask;
        for op in ["band", "bor", "bxor", "eq", "neq", "min", "max"] {
            if width == 4 && matches!(op, "min" | "max") {
                continue;
            }
            let comparison = matches!(op, "eq" | "neq");
            let result_ty = if comparison { "i1" } else { ty };
            for reverse in [false, true] {
                let args = if reverse { "%b,%a" } else { "%a,%b" };
                let f = parse(&format!(
                    "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> {result_ty} {{ \
                     %r=arith.{op} {args}; builtin.ret %r : ({result_ty}); }};"
                ));
                for b in [a, b] {
                    let expected = match op {
                        "band" => a & b,
                        "bor" => a | b,
                        "bxor" => a ^ b,
                        "eq" => u128::from(a == b),
                        "neq" => u128::from(a != b),
                        "min" => a.min(b),
                        "max" => a.max(b),
                        _ => unreachable!(),
                    };
                    let inputs = [a, b]
                        .into_iter()
                        .flat_map(|value| {
                            (0..width).map(move |lane| ((value >> (32 * lane)) as u32) as usize)
                        })
                        .collect::<Vec<_>>();
                    let output = (0..if comparison { 1 } else { width })
                        .map(|lane| ((expected >> (32 * lane)) as u32) as usize)
                        .collect::<Vec<_>>();
                    execute_linked(&f, &inputs, &output);
                }
            }
        }
    }
}

#[test]
fn commutative_narrow_signed_and_boolean_sources_are_explicit() {
    for (ty, operations, a, b) in [
        ("i1", &["and", "or", "xor", "eq", "neq"][..], 0, 1),
        ("i8", &["band", "bor", "bxor", "eq", "neq"][..], 3, 5),
        ("u8", &["band", "bor", "bxor", "min", "max"][..], 3, 5),
        ("i16", &["band", "bor", "bxor", "eq", "neq"][..], 3, 5),
        ("u16", &["band", "bor", "bxor", "min", "max"][..], 3, 5),
        ("i32", &["band", "bor", "bxor", "min", "max"][..], 3, 5),
    ] {
        for op in operations {
            let result_ty = if matches!(*op, "eq" | "neq") {
                "i1"
            } else {
                ty
            };
            let f = parse(&format!(
                "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> {result_ty} {{ \
                 %r=arith.{op} %a,%b; builtin.ret %r : ({result_ty}); }};"
            ));
            assert_eq!(
                checked(&f, &SelectEnv::default()).input_symmetry(SelectedStepId(0)).unwrap(),
                InputSymmetry::CommutativePair {
                    lanes_per_operand: 1
                }
            );
            let expected = match *op {
                "and" | "band" => a & b,
                "or" | "bor" => a | b,
                "xor" | "bxor" => a ^ b,
                "eq" => usize::from(a == b),
                "neq" => usize::from(a != b),
                "min" => a.min(b),
                "max" => a.max(b),
                _ => unreachable!(),
            };
            execute_linked(&f, &[a, b], &[expected]);
        }
    }
    for (ty, width) in [("i32", 1), ("i64", 2), ("i128", 4)] {
        let negative = (0..width)
            .map(|lane| {
                if lane == 0 {
                    u32::MAX as usize - 2
                } else {
                    u32::MAX as usize
                }
            })
            .collect::<Vec<_>>();
        for (op, expected_low) in [("add", 4), ("mul", u32::MAX as usize - 20)] {
            let f = parse(&format!(
                "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> {ty} {{ \
                 %r=arith.{op} %a,%b <{{ overflow=#builtin.overflow<checked> }}>; builtin.ret %r \
                 : ({ty}); }};"
            ));
            let mut inputs = negative.clone();
            inputs.push(7);
            inputs.extend(core::iter::repeat_n(0, width - 1));
            let mut expected = vec![expected_low];
            expected.extend(core::iter::repeat_n(
                if op == "add" { 0 } else { u32::MAX as usize },
                width - 1,
            ));
            execute_linked(&f, &inputs, &expected);
        }
    }
}

#[test]
fn commutative_helper_projection_rejects_each_corrupted_u128_call() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a:u128,%b:u128) -> u128 { %r=arith.bxor %a,%b; \
         builtin.ret %r : (u128); };",
    );
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body.clone()).unwrap();
    let calls = body
        .ops()
        .iter()
        .enumerate()
        .filter_map(|(index, op)| match op {
            AnnotatedOp::Instruction {
                instruction: crate::masm::Instruction::Exec(_),
                unit,
            } => Some((index, *unit)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    for (site, unit) in calls {
        let mut wrong = body.clone();
        wrong.ops_mut().insert(
            site,
            AnnotatedOp::Instruction {
                instruction: crate::masm::Instruction::Swap1,
                unit,
            },
        );
        assert_eq!(
            verify::verify(&plan, &frame, wrong).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );
    }
    let mut wrong = plan.clone();
    wrong.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap().input_order =
        InputOrder::Canonical;
    assert!(verify::verify(&wrong, &frame, body).is_err());
}

#[test]
fn exact_arithmetic_result_types_and_properties_are_checked() {
    let f = parse(
        "builtin.function public extern(\"C\") @add(%a: u32, %b: u32) {\n%r = arith.sub %a, %b <{ \
         overflow = #builtin.overflow<wrapping> }>;\nbuiltin.ret; };",
    );
    checked(&f, &SelectEnv::default());
    let mut result =
        f.borrow().entry_block().borrow().body().iter().next().unwrap().results().all()[0];
    result.borrow_mut().set_type(midenc_hir::Type::I32);
    assert_malformed_source_rejected(&f, &SelectEnv::default());
}

/// Independent source fixtures enumerate the public registrations, not the selection enum.
pub(super) fn fixtures() -> Vec<(Parsed, bool)> {
    let mut fixtures = Vec::new();
    for (name, ty) in [
        ("add", "u32"),
        ("sub", "u32"),
        ("mul", "u32"),
        ("exp", "felt"),
        ("div", "u32"),
        ("mod", "u32"),
        ("and", "i1"),
        ("or", "i1"),
        ("xor", "i1"),
        ("band", "u32"),
        ("bor", "u32"),
        ("bxor", "u32"),
        ("shl", "u32"),
        ("shr", "u32"),
        ("rotl", "u32"),
        ("rotr", "u32"),
        ("eq", "u32"),
        ("neq", "u32"),
        ("gt", "u32"),
        ("gte", "u32"),
        ("lt", "u32"),
        ("lte", "u32"),
        ("min", "u32"),
        ("max", "u32"),
    ] {
        let properties = if matches!(name, "add" | "sub" | "mul") {
            " <{ overflow = #builtin.overflow<wrapping> }>"
        } else if name == "exp" {
            " <{ exponent_must_be_u32 = false }>"
        } else {
            ""
        };
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: {ty}, %b: {ty}) {{ %r = \
                 arith.{name} %a, %b{properties}; builtin.ret; }};"
            )),
            false,
        ));
    }
    for name in ["add_overflowing", "sub_overflowing", "mul_overflowing", "divmod"] {
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: u32, %b: u32) {{ %r, %s = \
                 arith.{name} %a, %b; builtin.ret; }};"
            )),
            false,
        ));
    }
    for (name, ty) in [
        ("incr", "u32"),
        ("neg", "felt"),
        ("inv", "felt"),
        ("ilog_2", "felt"),
        ("pow_2", "u32"),
        ("not", "i1"),
        ("bnot", "u32"),
        ("is_odd", "u32"),
        ("popcnt", "u32"),
        ("clz", "u32"),
        ("ctz", "u32"),
        ("clo", "u32"),
        ("cto", "u32"),
    ] {
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: {ty}) {{ %r = arith.{name} %a; \
                 builtin.ret; }};"
            )),
            false,
        ));
    }
    for (name, src, dst) in
        [("trunc", "u64", "u32"), ("zext", "u32", "u64"), ("sext", "i32", "i64")]
    {
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: {src}) {{ %r = arith.{name} %a <{{ \
                 ty = #builtin.type<{dst}> }}>; builtin.ret; }};"
            )),
            false,
        ));
    }
    for name in ["add", "sub", "mul", "div"] {
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: felt, %b: felt, %c: felt, %d: felt) \
                 {{ %r, %s = arith.ext_2_{name} %a, %b, %c, %d; builtin.ret; }};"
            )),
            false,
        ));
    }
    for name in ["neg", "inv"] {
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: felt, %b: felt) {{ %r, %s = \
                 arith.ext_2_{name} %a, %b; builtin.ret; }};"
            )),
            false,
        ));
    }
    for name in ["assert", "assertz", "assert_u32"] {
        fixtures.push((
            parse(&format!(
                "builtin.function public extern(\"C\") @f(%a: u32) {{ %r = hir.{name} %a <{{ code \
                 = #builtin.u32<0>, message = \"\" }}>; builtin.ret; }};"
            )),
            false,
        ));
    }
    for text in [
        "builtin.function public extern(\"C\") @f(%a: u32, %b: u32) { hir.assert_eq %a, %b <{ \
         code = #builtin.u32<0>, message = \"\" }> : (u32,u32); builtin.ret; };",
        "builtin.function public extern(\"C\") @f(%a: u32) { %r = hir.cast %a <{ ty = \
         #builtin.type<u64> }>; builtin.ret; };",
        "builtin.function public extern(\"C\") @f(%p: i1, %a: u32, %b: u32) { %r = cf.select %p, \
         %a, %b; builtin.ret; };",
        "builtin.function public extern(\"C\") @f(%a: i32) { %r = wasm.sign_extend %a <{ src_ty = \
         #builtin.type<i8>, dst_ty = #builtin.type<i32> }>; builtin.ret; };",
        "builtin.function public extern(\"C\") @f(%a: i32, %b: i32) { %r = wasm.i32_rem_s %a, %b; \
         builtin.ret; };",
        "builtin.function public extern(\"C\") @f(%a: i64, %b: i64) { %r = wasm.i64_rem_s %a, %b; \
         builtin.ret; };",
        "builtin.function public extern(\"C\") @f() { %r = ub.poison <{ value = #ub.poison<u32> \
         }>; builtin.ret; };",
    ] {
        fixtures.push((parse(text), false));
    }
    fixtures.push((constant_pointer_fixture(), false));
    fixtures.push((
        parse_module_function(
            "builtin.module public @globals { builtin.global_variable private @g : u32 { \
             builtin.ret_imm 0 : u32; }; builtin.function public extern(\"C\") @f() { %r = \
             builtin.global_symbol @g : ptr<u8, byte>; builtin.ret; }; };",
        ),
        true,
    ));
    fixtures
}

fn constant_pointer_fixture() -> Parsed {
    use midenc_hir::{Builder, BuilderExt, OpBuilder, SourceSpan, Type};
    let f = parse("builtin.function public extern(\"C\") @f() { builtin.ret; };");
    let terminator = f.borrow().entry_block().borrow().terminator().unwrap();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(terminator);
    let pointer = crate::hir::Pointer::new(42, midenc_hir::PointerType::new(Type::U32));
    builder.create::<crate::hir::ConstantPointer, _>(SourceSpan::UNKNOWN)(pointer).unwrap();
    f
}

#[test]
fn arithmetic_fixtures_expand_through_final_verifier() {
    for (f, needs_link) in fixtures() {
        let info = needs_link.then(|| linked(&f));
        let env = SelectEnv {
            link_info: info.as_ref(),
            ..Default::default()
        };
        let (plan, frame, body) = ready(&f, &env);
        verify::verify(&plan, &frame, body)
            .unwrap_or_else(|e| panic!("{}: {e}", f.borrow().as_operation().to_pretty_string()));
    }
}

#[test]
fn selected_arithmetic_executes_distinct_lanes_flags_calls_and_suffix() {
    for (op, ty, inputs, expected) in [
        ("sub", "u32", vec![17, 5], vec![12]),
        ("sub", "u64", vec![7, 4, 3, 1], vec![4, 3]),
        ("add", "u128", vec![u32::MAX as usize, 0, 5, 7, 1, 0, 2, 3], vec![0, 1, 7, 10]),
        ("mul", "i128", vec![3, 0, 0, 0, 7, 0, 0, 0], vec![21, 0, 0, 0]),
        ("band", "u128", vec![7, 15, 31, 63, 3, 5, 17, 33], vec![3, 5, 17, 33]),
    ] {
        let f = parse(
            &format!(
                "builtin.function public extern(\"C\") @f(%a: {ty}, %b: {ty}) -> {ty} {{ %r = \
                 arith.{op} %a, %b <{{ overflow = #builtin.overflow<wrapping> }}>; builtin.ret %r \
                 : ({ty}); }};"
            )
            .replace(
                "arith.band %a, %b <{ overflow = #builtin.overflow<wrapping> }>",
                "arith.band %a, %b",
            ),
        );
        execute_linked(&f, &inputs, &expected);
    }
    for (op, ty, inputs, expected) in [
        (
            "add",
            "u128",
            vec![u32::MAX as usize; 8],
            vec![
                1,
                u32::MAX as usize - 1,
                u32::MAX as usize,
                u32::MAX as usize,
                u32::MAX as usize,
            ],
        ),
        (
            "mul",
            "i128",
            vec![
                0,
                0,
                0,
                1usize << 31,
                u32::MAX as usize,
                u32::MAX as usize,
                u32::MAX as usize,
                u32::MAX as usize,
            ],
            vec![1, 0, 0, 0, 1usize << 31],
        ),
        (
            "mul",
            "felt",
            vec![0xffff_ffff_0000_0000usize, 0xffff_ffff_0000_0000usize],
            vec![1, 1],
        ),
    ] {
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {ty}, %b: {ty}) -> (i1,{ty}) {{ \
             %flag,%r = arith.{op}_overflowing %a, %b; builtin.ret %flag,%r : (i1,{ty}); }};"
        ));
        execute_linked(&f, &inputs, &expected);
    }
}

#[test]
fn selected_conversion_aliases_and_repeats_preserve_distinct_limb_order() {
    for (op, src, dst, inputs, expected) in [
        ("arith.trunc", "u128", "u64", vec![11, 22, 33, 44], vec![11, 22]),
        ("arith.zext", "u64", "u128", vec![11, 22], vec![11, 22, 0, 0]),
        (
            "arith.sext",
            "i64",
            "i128",
            vec![11, 0x80000000],
            vec![11, 0x80000000, u32::MAX as usize, u32::MAX as usize],
        ),
        ("hir.cast", "u64", "i64", vec![11, 22], vec![11, 22]),
    ] {
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {src}) -> {dst} {{ %r = {op} %a <{{ ty \
             = #builtin.type<{dst}> }}>; builtin.ret %r : ({dst}); }};"
        ));
        execute_linked(&f, &inputs, &expected);
    }
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a: u64) -> u64 { %r = arith.bnot %a; \
         builtin.ret %r : (u64); };",
    );
    execute_linked(&f, &[0x11223344, 0x55667788], &[0xeeddccbb, 0xaa998877]);
}

#[test]
fn arithmetic_intrinsic_argument_and_result_permutations_are_rejected() {
    for (op, ty) in [("sub", "u128"), ("sub", "i128"), ("sub", "u64"), ("band", "u128")] {
        let props = if op == "band" {
            ""
        } else {
            " <{ overflow = #builtin.overflow<checked> }>"
        };
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {ty},%b: {ty}) -> {ty} {{ %r=arith.{op} \
             %a,%b{props}; builtin.ret %r : ({ty}); }};"
        ));
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        verify::verify(&plan, &frame, body.clone()).unwrap();
        for result in [false, true] {
            let mut bad = body.clone();
            let (index, unit) = bad
                .ops()
                .iter()
                .enumerate()
                .find_map(|(i, op)| match op {
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Exec(_),
                        unit,
                    } => Some((i, *unit)),
                    _ => None,
                })
                .unwrap();
            bad.ops_mut().insert(
                index + usize::from(result),
                AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Swap1,
                    unit,
                },
            );
            assert!(verify::verify(&plan, &frame, bad).is_err(), "{op}/{ty} result={result}");
        }
    }
}

#[test]
fn arithmetic_overflow_properties_and_pointer_pointees_are_exact_source_authority() {
    use midenc_hir::{Overflow, dialects::builtin::attributes::OverflowAttr};
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a: u32,%b: u32) { %r=arith.add %a,%b <{ \
         overflow=#builtin.overflow<wrapping> }>; builtin.ret; };",
    );
    let mut op = f
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .front()
        .get()
        .unwrap()
        .as_operation_ref();
    let attr = f.context.create_attribute::<OverflowAttr, _>(Overflow::Overflowing);
    op.borrow_mut().set_property("overflow", attr.as_attribute_ref()).unwrap();
    assert_malformed_source_rejected(&f, &SelectEnv::default());
    let f = constant_pointer_fixture();
    checked(&f, &SelectEnv::default());
    let op = f
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .front()
        .get()
        .unwrap()
        .as_operation_ref();
    let mut result = op.borrow().results().all()[0];
    result
        .borrow_mut()
        .set_type(midenc_hir::PointerType::new(midenc_hir::Type::U8).into());
    assert_malformed_source_rejected(&f, &SelectEnv::default());
}

#[test]
fn pointer_select_is_outside_the_final_hir_source_domain() {
    let result = midenc_hir::parse::parse_any(
        midenc_hir::parse::ParserConfig {
            context: Rc::new(Context::default()),
            verify: false,
        },
        Uri::new("pointer-select.hir"),
        "builtin.function public extern(\"C\") @f(%p: i1,%a: ptr<u32, byte>,%b: ptr<u32, byte>) \
         -> ptr<u32, byte> { %r=cf.select %p,%a,%b; builtin.ret %r : (ptr<u32, byte>); };",
    );
    assert!(result.is_err(), "pointer select is not legal final HIR");
}

#[test]
fn selected_extension_field_subtraction_preserves_component_order() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a: felt,%b: felt,%c: felt,%d: felt) -> \
         (felt,felt) { %x,%y=arith.ext_2_sub %a,%b,%c,%d; builtin.ret %x,%y : (felt,felt); };",
    );
    execute_linked(&f, &[11, 7, 3, 2], &[8, 5]);
}

#[test]
fn arithmetic_widths_and_overflow_modes_have_complete_selected_contracts() {
    for ty in ["i1", "u8", "u16", "u32", "i32", "u64", "i64", "u128", "i128", "felt"] {
        for op in ["add", "sub", "mul"] {
            if ty == "i1" && op == "mul" {
                continue;
            }
            for mode in ["unchecked", "wrapping", "checked", "overflowing"] {
                let (operation, property, results) = if mode == "overflowing" {
                    (format!("arith.{op}_overflowing"), String::new(), "%flag,%r")
                } else {
                    (
                        format!("arith.{op}"),
                        format!(" <{{ overflow=#builtin.overflow<{mode}> }}>"),
                        "%r",
                    )
                };
                let f = parse(&format!(
                    "builtin.function public extern(\"C\") @f(%a: {ty},%b: {ty}) {{ \
                     {results}={operation} %a,%b{property}; builtin.ret; }};"
                ));
                let (plan, frame, body) = ready(&f, &SelectEnv::default());
                let symmetry = plan.selected.input_symmetry(SelectedStepId(0)).unwrap();
                assert_eq!(symmetry == InputSymmetry::Ordered, op == "sub", "{ty}/{op}/{mode}");
                verify::verify(&plan, &frame, body)
                    .unwrap_or_else(|e| panic!("{ty}/{op}/{mode}: {e}"));
            }
        }
    }
}

fn check_bool_cast(dst: &str) {
    let f = parse(&format!(
        "builtin.function public extern(\"C\") @f(%a: i1) -> {dst} {{ %r = hir.cast %a <{{ ty = \
         #builtin.type<{dst}> }}>; builtin.ret %r : ({dst}); }};"
    ));
    for value in [0, 1] {
        execute_linked(&f, &[value], &[value]);
    }
}

#[test]
fn unsigned_scalar_casts_to_felt_preserve_raw_lane_and_live_input() {
    for (ty, maximum) in [("i1", 1), ("u8", 255), ("u16", 65535), ("u32", u32::MAX as usize)] {
        let f = parse(&format!(
            "builtin.function public extern(\"C\") @f(%a: {ty}) -> (felt, {ty}) {{ %r = hir.cast \
             %a <{{ ty = #builtin.type<felt> }}>; builtin.ret %r, %a : (felt, {ty}); }};"
        ));
        let evidence = crate::select::evidence(&f.borrow(), &SelectEnv::default()).unwrap();
        let cast = &evidence.source.program.segments[0].units[0];
        assert_eq!(cast.stack_inputs.len(), 1);
        assert_eq!(cast.outputs.len(), 1);
        assert_eq!(
            evidence.source.units[&cast.id].aliases.pairs(),
            &[(cast.outputs[0], cast.stack_inputs[0])],
            "{ty} widening must retain the exact input token"
        );
        for value in [0, maximum] {
            execute_linked(&f, &[value], &[value, value]);
        }
    }
}

#[test]
fn bool_cast_to_felt_preserves_single_lane() {
    check_bool_cast("felt");
}

#[test]
fn valid_bool_cast_to_i8_preserves_value() {
    check_bool_cast("i8");
}

#[test]
fn valid_bool_cast_to_u8_preserves_value() {
    check_bool_cast("u8");
}

#[test]
fn valid_bool_cast_to_i16_preserves_value() {
    check_bool_cast("i16");
}

#[test]
fn valid_bool_cast_to_u16_preserves_value() {
    check_bool_cast("u16");
}

#[test]
fn valid_bool_cast_controls_preserve_value() {
    for dst in ["i1", "i32", "felt"] {
        check_bool_cast(dst);
    }
}

#[test]
fn exact_arithmetic_mode_result_shapes_are_checked() {
    use midenc_hir::{SourceSpan, Type};
    for mutation in 0..3 {
        let f = parse(
            "builtin.function public extern(\"C\") @f(%a: i128,%b: i128) { \
             %flag,%r=arith.sub_overflowing %a,%b; builtin.ret; };",
        );
        let candidate = select_function(&f.borrow(), &SelectEnv::default()).unwrap();
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
                let mut flag = op.borrow().results().all()[0];
                flag.borrow_mut().set_type(Type::U32);
            }
            1 => {
                let last = op.borrow().results().groups().count() - 1;
                op.borrow_mut().results_mut().group_mut(last).pop().unwrap();
            }
            _ => {
                let extra = f.context.make_result(SourceSpan::UNKNOWN, Type::U32, op, 2);
                op.borrow_mut().results_mut().push(extra);
            }
        }
        let function = f.borrow();
        let before = function.as_operation().to_pretty_string();
        let env = SelectEnv::default();
        assert!(select_function(&function, &env).is_err(), "mutation {mutation}");
        assert!(
            check_selection(
                candidate,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(before, function.as_operation().to_pretty_string());
    }
}

#[test]
fn wide_arithmetic_call_shape_target_publication_and_suffix_mutations_are_rejected() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a: i128,%b: i128,%live: felt) -> \
         (i1,i128,felt) { %flag,%r=arith.sub_overflowing %a,%b; builtin.ret %flag,%r,%live : \
         (i1,i128,felt); };",
    );
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body.clone()).unwrap();
    let (site, unit) = body
        .ops()
        .iter()
        .enumerate()
        .find_map(|(i, op)| match op {
            AnnotatedOp::Instruction {
                instruction: crate::masm::Instruction::Exec(_),
                unit,
            } => Some((i, *unit)),
            _ => None,
        })
        .unwrap();
    for mutation in 0..7 {
        let mut bad = body.clone();
        match mutation {
            0 => {
                bad.ops_mut()[site] = AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Exec(crate::lower::raw::target(
                        "::intrinsics::i128::overflowing_add",
                    )),
                    unit,
                };
            }
            1 => {
                bad.ops_mut().remove(site);
            }
            2 => {
                bad.ops_mut().insert(
                    site + 1,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Swap1,
                        unit,
                    },
                );
            }
            3 => {
                bad.ops_mut().insert(
                    site + 1,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::MovUp5,
                        unit,
                    },
                );
                bad.ops_mut().insert(
                    site + 2,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Drop,
                        unit,
                    },
                );
            }
            4 => {
                bad.ops_mut().insert(
                    site + 1,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Drop,
                        unit,
                    },
                );
            }
            5 => {
                bad.ops_mut().insert(
                    site + 1,
                    AnnotatedOp::Instruction {
                        instruction: Primitive::Push(miden_core::Felt::ONE).instruction().unwrap(),
                        unit,
                    },
                );
                bad.ops_mut().insert(
                    site + 2,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::MovDn5,
                        unit,
                    },
                );
            }
            _ => {
                let end = bad
                    .ops()
                    .iter()
                    .position(
                        |op| matches!(op,AnnotatedOp::Boundary(Boundary::UnitEnd(id)) if *id==unit),
                    )
                    .unwrap();
                let publication = bad.ops_mut().remove(end);
                bad.ops_mut().insert(site, publication);
            }
        }
        assert!(verify::verify(&plan, &frame, bad).is_err(), "physical mutation {mutation}");
    }
    let function = f.borrow();
    let env = SelectEnv::default();
    let candidate = select_function(&function, &env).unwrap();
    let mut bad = candidate.clone();
    bad.segments[0].units[0].effects.may_trap = false;
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
fn conversion_alias_lane_permutation_is_rejected() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a: u128) -> u64 { %r=arith.trunc %a <{ \
         ty=#builtin.type<u64> }>; builtin.ret %r : (u64); };",
    );
    let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body.clone()).unwrap();
    let (end, unit) = body
        .ops()
        .iter()
        .enumerate()
        .find_map(|(i, op)| match op {
            AnnotatedOp::Boundary(Boundary::UnitEnd(unit)) => Some((i, *unit)),
            _ => None,
        })
        .unwrap();
    body.ops_mut().insert(
        end,
        AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::Swap1,
            unit,
        },
    );
    assert!(verify::verify(&plan, &frame, body).is_err());
}

#[test]
fn selected_pointer_constants_preserve_complete_types_addresses_and_suffix() {
    use midenc_hir::{
        AddressSpace, Builder, BuilderExt, OpBuilder, PointerType, SourceSpan, Type,
        attributes::InferAttributeValueType,
    };
    for space in [AddressSpace::Byte, AddressSpace::Element] {
        for pointee in [
            Type::U32,
            PointerType::new(Type::U8).into(),
            PointerType::new_with_address_space(Type::U8, AddressSpace::Element).into(),
        ] {
            let pointer_type = PointerType::new_with_address_space(pointee, space);
            let ty = Type::from(pointer_type.clone());
            // Exercise the declared custom syntax as well as the builder API.
            let space_text = match space {
                AddressSpace::Byte => "byte",
                AddressSpace::Element => "element",
            };
            let pointee_text = match pointer_type.pointee() {
                Type::U32 => "u32",
                Type::Ptr(inner) if inner.addrspace() == AddressSpace::Byte => "ptr<u8, byte>",
                Type::Ptr(_) => "ptr<u8, element>",
                _ => unreachable!(),
            };
            let text_type = format!("ptr<{pointee_text}, {space_text}>");
            let f = parse(&format!(
                "builtin.function public extern(\"C\") @f() -> {text_type} {{ %p = \
                 hir.constant_pointer 4294967295 : {text_type}; builtin.ret %p : ({text_type}); \
                 }};"
            ));
            execute(&f, &SelectEnv::default(), &[], &[u32::MAX as usize]);

            for folded in [false, true] {
                let f = parse(&format!(
                    "builtin.function public extern(\"C\") @f() -> {text_type} {{ %p = \
                     hir.constant_pointer 0 : {text_type}; builtin.ret %p : ({text_type}); }};"
                ));
                let mut old = f
                    .borrow()
                    .entry_block()
                    .borrow()
                    .body()
                    .front()
                    .get()
                    .unwrap()
                    .as_operation_ref();
                let mut builder = OpBuilder::new(f.context.clone());
                builder.set_insertion_point_before(old);
                let new = if folded {
                    use midenc_dialect_arith::ArithOpBuilder;
                    use midenc_hir::{OpFoldResult, SmallVec, traits::Foldable};
                    let integer = builder.u32(u32::MAX, SourceSpan::UNKNOWN);
                    let mut cast = builder.create::<crate::hir::IntToPtr, _>(SourceSpan::UNKNOWN)(
                        integer,
                        ty.clone(),
                    )
                    .unwrap();
                    let mut results = SmallVec::new();
                    assert!(cast.borrow().fold(&mut results).is_ok());
                    let OpFoldResult::Attribute(attr) = results[0] else {
                        panic!("fold must produce an attribute")
                    };
                    let dialect = f.context.get_or_register_dialect::<crate::hir::HirDialect>();
                    let new = dialect
                        .materialize_constant(&mut builder, attr, &ty, SourceSpan::UNKNOWN)
                        .unwrap();
                    cast.borrow_mut().as_operation_mut().erase();
                    new
                } else {
                    let pointer = crate::hir::Pointer::new(u32::MAX, pointer_type.clone());
                    assert_eq!(pointer.infer_type_from_value(), ty);
                    builder.create::<crate::hir::ConstantPointer, _>(SourceSpan::UNKNOWN)(pointer)
                        .unwrap()
                        .as_operation_ref()
                };
                let mut old_result = old.borrow().results().all()[0];
                let new_result = new.borrow().results().all()[0];
                old_result.borrow_mut().replace_all_uses_with(new_result);
                old.borrow_mut().erase();
                execute(&f, &SelectEnv::default(), &[], &[u32::MAX as usize]);
            }
        }
    }
}

#[test]
fn pointer_constant_outer_space_mismatch_is_rejected() {
    let f = parse(
        "builtin.function public extern(\"C\") @f() { %p = hir.constant_pointer 42 : ptr<u32, \
         element>; builtin.ret; };",
    );
    checked(&f, &SelectEnv::default());
    let op = f
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .front()
        .get()
        .unwrap()
        .as_operation_ref();
    let mut result = op.borrow().results().all()[0];
    result
        .borrow_mut()
        .set_type(midenc_hir::PointerType::new(midenc_hir::Type::U32).into());
    assert_malformed_source_rejected(&f, &SelectEnv::default());
}

#[test]
fn commutative_repeated_operands_and_downstream_names_execute() {
    for (ty, width) in [("u32", 1), ("u64", 2), ("u128", 4)] {
        for repeated in [false, true] {
            let rhs = if repeated { "%a" } else { "%b" };
            let f = parse(&format!(
                        "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> ({ty},{ty}) \
                         {{                  %sum=arith.add %a,{rhs} <{{ \
                         overflow=#builtin.overflow<wrapping> }}>;                  \
                         %difference=arith.sub %sum,%a <{{ overflow=#builtin.overflow<wrapping> \
                         }}>;                  builtin.ret %difference,%a : ({ty},{ty}); }};"
                    ));
            let a = [0x12345678, 0xabcdef01, 0x10203040, 0x55667788];
            let b = [0x98765432, 0x13579bdf, 0x90807060, 0x11223344];
            let mut inputs = a[..width].to_vec();
            inputs.extend(&b[..width]);
            let mut expected = if repeated {
                a[..width].to_vec()
            } else {
                b[..width].to_vec()
            };
            expected.extend(&a[..width]);
            execute_linked(&f, &inputs, &expected);
        }
    }
}

#[test]
fn pointer_equality_remains_outside_the_hir_operand_domain() {
    for ty in ["ptr<u8, byte>", "ptr<u8, element>"] {
        for op in ["eq", "neq"] {
            let text = format!(
                "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> i1 {{                  %r=arith.{op} %a,%b; builtin.ret %r : (i1); }};"
            );
            let result = midenc_hir::parse::parse_any(
                midenc_hir::parse::ParserConfig {
                    context: Rc::new(Context::default()),
                    verify: true,
                },
                Uri::new("pointer_equality.hir"),
                &text,
            );
            let error = result.unwrap_err();
            assert!(error.to_string().contains("expected an integral type"), "{error}");
        }
    }
}

/// A one-shift function whose count is the constant `count`.
fn shift_by_constant(op: &str, ty: &str, count: u32) -> Parsed {
    parse(&format!(
        "builtin.function public extern(\"C\") @f(%a:{ty}) -> {ty} {{ %c = arith.constant {count} \
         : u32; %r = arith.{op} %a, %c; builtin.ret %r : ({ty}); }};"
    ))
}

#[test]
fn constant_movement_counts_are_immediates() {
    for (op, ty, instruction, a, expected) in [
        ("shl", "u32", "u32shl.3", 5usize, 40usize),
        ("shl", "i32", "u32shl.3", 5, 40),
        ("shl", "u16", "u32shl.3", 0x2345, 0x1a28),
        ("shl", "u8", "u32shl.3", 200, 64),
        ("shr", "u32", "u32shr.3", 40, 5),
        ("shr", "u16", "u32shr.3", 0x1234, 0x246),
        ("shr", "u8", "u32shr.3", 200, 25),
        ("rotl", "u32", "u32rotl.3", 0xa000_0001, 0x0000_000d),
        ("rotl", "i32", "u32rotl.3", 0xa000_0001, 0x0000_000d),
        ("rotr", "u32", "u32rotr.3", 0x0000_000d, 0xa000_0001),
        ("rotr", "i32", "u32rotr.3", 0x0000_000d, 0xa000_0001),
    ] {
        let f = shift_by_constant(op, ty, 3);
        let procedure = selected_procedure(&f, &SelectEnv::default());
        assert!(procedure.contains(instruction), "{op} {ty}: {procedure}");
        assert!(!procedure.contains("push.3"), "{op} {ty}: {procedure}");
        execute(&f, &SelectEnv::default(), &[a], &[expected]);
        let selected = checked(&f, &SelectEnv::default());
        let units = &selected.segments[0].units;
        assert_eq!(units[0].publication, OutputPublication::Literal, "{op} {ty}");
        assert_eq!(units[0].outputs.len(), 1);
        assert_eq!(units[1].stack_inputs.len(), 1, "{op} {ty}: only the shifted value");
        assert_eq!(units[1].semantic_inputs.len(), 2, "{op} {ty}: the count stays a dependency");
    }
}

#[test]
fn signed_and_wide_shifts_keep_the_count_on_the_stack() {
    for (op, ty, a, expected) in [
        ("shr", "i32", 40usize, 5usize),
        ("shl", "u64", 5, 40),
        ("rotl", "u64", 5, 40),
        ("rotr", "i64", 40, 5),
    ] {
        let f = shift_by_constant(op, ty, 3);
        let procedure = selected_procedure(&f, &SelectEnv::default());
        assert!(procedure.contains("push.3"), "{op} {ty}: {procedure}");
        assert!(
            !["u32shr.", "u32shl.", "u32rotl.", "u32rotr."]
                .iter()
                .any(|i| procedure.contains(i)),
            "{procedure}"
        );
        let wide = ty.ends_with("64");
        let inputs = if wide { vec![a, 0] } else { vec![a] };
        let outputs = if wide {
            vec![expected, 0]
        } else {
            vec![expected]
        };
        execute_linked(&f, &inputs, &outputs);
    }
}

#[test]
fn shift_counts_at_or_above_the_width_are_not_immediates() {
    let f = shift_by_constant("shl", "u32", 32);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(procedure.contains("push.32"), "{procedure}");
    assert!(!procedure.contains("u32shl."), "{procedure}");
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments[0].units[0].publication, OutputPublication::Stack);
}

#[test]
fn a_zero_shift_count_is_an_immediate() {
    let f = shift_by_constant("shr", "u32", 0);
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(procedure.contains("u32shr.0"), "{procedure}");
    assert!(!procedure.contains("push.0"), "{procedure}");
    execute(&f, &SelectEnv::default(), &[7], &[7]);
}

#[test]
fn a_count_shared_with_another_user_is_pushed_for_that_user_only() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%a:u32) -> u32 { %c = arith.constant 3 : u32; \
         %r = arith.shl %a, %c; %s = arith.add %r, %c <{ overflow = #builtin.overflow<wrapping> \
         }>; builtin.ret %s : (u32); };",
    );
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(procedure.contains("u32shl.3"), "{procedure}");
    assert_eq!(procedure.matches("push.3").count(), 1, "{procedure}");
    execute(&f, &SelectEnv::default(), &[5], &[43]);
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments[0].units[0].publication, OutputPublication::Stack);
}

#[test]
fn a_constant_shifted_by_itself_keeps_the_stack_form() {
    // One value cannot be both the shifted lane and the immediate of the same instruction.
    let f = parse(
        "builtin.function public extern(\"C\") @f() -> u32 { %c = arith.constant 3 : u32; %r = \
         arith.shl %c, %c; builtin.ret %r : (u32); };",
    );
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(!procedure.contains("u32shl."), "{procedure}");
    assert!(procedure.contains("push.3"), "{procedure}");
    execute(&f, &SelectEnv::default(), &[], &[24]);
}

#[test]
fn a_dominating_count_folds_into_a_nested_shift() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%p: i1, %a: u32) -> u32 { %c = arith.constant 3 \
         : u32; %x = scf.if %p then { %r = arith.shl %a, %c; scf.yield %r : (u32); } else { \
         scf.yield %a : (u32); } : (i1) -> u32; builtin.ret %x : (u32); };",
    );
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(procedure.contains("u32shl.3"), "{procedure}");
    assert!(!procedure.contains("push.3"), "{procedure}");
    execute(&f, &SelectEnv::default(), &[1, 5], &[40]);
    execute(&f, &SelectEnv::default(), &[0, 5], &[5]);
}

#[test]
fn a_folded_count_is_observable_as_a_constant() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
        interner::Symbol,
    };
    let f = shift_by_constant("shl", "u32", 3);
    let entry = f.borrow().entry_block();
    let ops = entry.borrow().body().iter().map(|op| op.as_operation_ref()).collect::<Vec<_>>();
    let count = ops[0].borrow().results().all()[0].borrow().as_value_ref();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(ops[1]);
    let variable = Variable::new(Symbol::intern("count"), Symbol::intern("test.rs"), 1, None);
    builder.debug_value(count, variable, SourceSpan::UNKNOWN).unwrap();
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    let record = plan.debug_locations.values().find(|r| !r.locations.is_empty()).unwrap();
    assert_eq!(
        record.locations,
        vec![crate::lower::debug::Location::Literal(miden_core::Felt::from(3u32))]
    );
    let mut expanded = ExpansionEnv::default();
    expanded.resolve_debug(&plan, &frame);
    let variable = expanded
        .debug
        .values()
        .find_map(|decorator| match decorator {
            DebugDecorator::Variable(info) => Some(info),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        *variable.value_location(),
        crate::masm::DebugVarLocation::Const(miden_core::Felt::from(3u32))
    );
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    assert!(procedure.contains("u32shl.3") && !procedure.contains("push.3"), "{procedure}");
}

#[test]
fn an_immediate_shift_body_must_carry_the_registered_count() {
    let f = shift_by_constant("shl", "u32", 3);
    let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
    let mut rewritten = 0;
    for op in body.ops_mut() {
        if let AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::U32ShlImm(value),
            ..
        } = op
        {
            *value = 4u8.into();
            rewritten += 1;
        }
    }
    assert_eq!(rewritten, 1);
    let error = verify::verify(&plan, &frame, body).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSelectedLowering);
}

/// A folded count defined before a loop is never pushed for the loop header: the header
/// layout leaves a literal without a copy to reconstruction, and the shift inside folds it.
#[test]
fn a_folded_count_live_across_a_loop_is_not_carried_on_the_stack() {
    let f = parse(
        "builtin.function public extern(\"C\") @f(%n: u32, %a: u32) -> u32 { %c = arith.constant \
         3 : u32; %i, %r = scf.while %n, %a before { ^before(%i0: u32, %x: u32): %z = \
         arith.constant 0 : u32; %go = arith.neq %i0, %z; scf.condition %go, %i0, %x : (i1, u32, \
         u32); } after { ^after(%j: u32, %y: u32): %one = arith.constant 1 : u32; %j2 = arith.sub \
         %j, %one <{ overflow = #builtin.overflow<wrapping> }>; %y2 = arith.shl %y, %c; scf.yield \
         %j2, %y2 : (u32, u32); } : (u32, u32) -> (u32, u32); builtin.ret %r : (u32); };",
    );
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(procedure.contains("u32shl.3"), "{procedure}");
    assert!(!procedure.contains("push.3"), "{procedure}");
    execute(&f, &SelectEnv::default(), &[4, 1], &[4096]);
    execute(&f, &SelectEnv::default(), &[0, 5], &[5]);
}
