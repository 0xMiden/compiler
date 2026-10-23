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

#[test]
fn signed_minmax_rejects_each_invalid_limb() {
    for (ty, width) in [("i32", 1), ("i64", 2)] {
        for op in ["min", "max"] {
            let f = parse(&format!(
                "builtin.function public extern(\"C\") @f(%a:{ty},%b:{ty}) -> {ty} {{                  %r=arith.{op} %a,%b; builtin.ret %r : ({ty}); }};"
            ));
            let procedure = selected_procedure(&f, &SelectEnv::default());
            let core =
                midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
            let intrinsics = crate::intrinsics::load(f.context.session()).unwrap();
            for lane in 0..2 * width {
                let mut inputs = vec![0; 2 * width];
                inputs[lane] = u32::MAX as usize + 1;
                let code = format!(
                    "{procedure} begin {} exec.selected exec.::miden::core::sys::truncate_stack \
                     end",
                    crate::plan::tests::lowering_units::push_top(&inputs)
                );
                let mut assembler = miden_assembly::Assembler::default();
                assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
                assembler
                    .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
                    .unwrap();
                let program = assembler
                    .assemble_program("invalid_signed_minmax", code)
                    .unwrap()
                    .try_into_program()
                    .unwrap();
                let mut host = crate::plan::tests::lowering_units::host();
                host.load_library(miden_processor::HostLibrary::from(core.clone())).unwrap();
                host.load_library(miden_processor::HostLibrary::from(intrinsics.clone()))
                    .unwrap();
                let error =
                    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                        .execute_sync(&program, &mut host)
                        .unwrap_err();
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
