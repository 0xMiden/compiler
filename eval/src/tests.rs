use alloc::format;
use core::ops::{Deref, DerefMut};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_dialect_wasm::WasmOpBuilder;
use midenc_hir::{
    Builder, Immediate, Op, PointerType, Report, SourceSpan, SymbolName, SymbolTable, Type,
    UnsafeIntrusiveEntityRef, ValueRef,
    diagnostics::Uri,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, Module},
    parse::{ParserConfig, parse},
    testing::Test,
};

use crate::*;

mod memory;

struct EvalTest {
    test: Test,
    evaluator: HirEvaluator,
}

impl Default for EvalTest {
    fn default() -> Self {
        let test = Test::default();
        let evaluator = HirEvaluator::new(test.context_rc());
        Self { test, evaluator }
    }
}

impl EvalTest {
    pub fn named(name: &'static str) -> Self {
        let test = Test::named(name);
        let evaluator = HirEvaluator::new(test.context_rc());
        Self { test, evaluator }
    }

    pub fn with_function(&mut self, params: &[Type], results: &[Type]) {
        let name = self.test.name();
        self.test.with_function(name, params, results);
    }
}

impl Deref for EvalTest {
    type Target = Test;

    fn deref(&self) -> &Self::Target {
        &self.test
    }
}

impl DerefMut for EvalTest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.test
    }
}

const ALIAS_TEST_SOURCE: &str = r#"
builtin.module public @test {
    builtin.function private extern("C") @body(%x: u32) -> u32 { builtin.ret %x : (u32); };
    builtin.function_alias private @first -> @body;
    builtin.function_alias public @api -> @first;
    builtin.function public extern("C") @caller(%x: u32) -> u32 {
        %result = hir.exec @api(%x) : extern("C") (u32) -> u32;
        builtin.ret %result : (u32);
    };
};
"#;

fn parse_alias_test_source() -> Result<(HirEvaluator, UnsafeIntrusiveEntityRef<Module>), Report> {
    let test = Test::default();
    test.context().get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
    let evaluator = HirEvaluator::new(test.context_rc());
    let module = parse::<Module>(
        ParserConfig::new(test.context_rc()),
        Uri::new("eval_alias.hir"),
        ALIAS_TEST_SOURCE,
    )?;
    Ok((evaluator, module))
}

/// Test that we can evaluate a standalone operation, not just callables
///
/// This verifies ControlFlowEffect::None and ControlFlowEffect::Yield.
#[test]
fn eval_test() -> Result<(), Report> {
    let mut test = EvalTest::default();

    let op = {
        let builder = test.builder_mut();
        let block = builder.context_rc().create_block_with_params([Type::I1]);
        let cond = block.borrow().arguments()[0] as ValueRef;
        let conditional = builder.r#if(cond, &[Type::U32], SourceSpan::default())?;

        let then_region = conditional.borrow().then_body().as_region_ref();
        builder.create_block(then_region, None, &[]);
        let is_true = builder.u32(1, SourceSpan::default());
        builder.r#yield([is_true], SourceSpan::default())?;

        let else_region = conditional.borrow().else_body().as_region_ref();
        builder.create_block(else_region, None, &[]);
        let is_false = builder.u32(0, SourceSpan::default());
        builder.r#yield([is_false], SourceSpan::default())?;
        conditional.as_operation_ref()
    };

    let op = op.borrow();
    let results = test.evaluator.eval(&op, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    let results = test.evaluator.eval(&op, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    Ok(())
}

/// Test evaluation of a callable operation
///
/// This verifies the interaction between ControlFlowEffect::Yield and ControlFlowEffect::Return
#[test]
fn eval_callable_test() -> Result<(), Report> {
    let mut test = EvalTest::named("callable");
    test.with_function(&[Type::I1], &[Type::U32]);

    {
        let mut builder = test.function_builder();
        let cond = builder.current_block().borrow().arguments()[0] as ValueRef;
        let conditional = builder.r#if(cond, &[Type::U32], SourceSpan::default())?;
        let result = conditional.borrow().results()[0] as ValueRef;
        builder.ret(Some(result), SourceSpan::default())?;

        let then_region = conditional.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        let is_true = builder.u32(1, SourceSpan::default());
        builder.r#yield([is_true], SourceSpan::default())?;

        let else_region = conditional.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let is_false = builder.u32(0, SourceSpan::default());
        builder.r#yield([is_false], SourceSpan::default())?;
    }

    let function = test.function();
    let callable = function.borrow();
    let results = test.evaluator.eval_callable(&*callable, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    let results = test.evaluator.eval_callable(&*callable, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    Ok(())
}

#[test]
fn alias_entrypoint_and_nested_call_execute_the_canonical_body() -> Result<(), Report> {
    let (mut evaluator, module) = parse_alias_test_source()?;
    let module = module.borrow();

    for name in ["api", "caller"] {
        let path = module.get(SymbolName::intern(name)).unwrap().borrow().path();
        let results = evaluator.call(module.as_operation(), &path, [42u32.into()])?;
        assert_eq!(results.as_slice(), &[Value::Immediate(42u32.into())]);
    }

    Ok(())
}

#[test]
fn alias_call_with_wrong_signature_fails() -> Result<(), Report> {
    let (mut evaluator, module) = parse_alias_test_source()?;
    let module = module.borrow();

    let alias = module.get(SymbolName::intern("api")).unwrap();
    let path = alias.borrow().path();
    let err = evaluator
        .call(module.as_operation(), &path, [])
        .expect_err("calling the alias without its required argument should fail");
    let expected = alloc::format!("entrypoint '{path}' expects 1 arguments, but 0 were given");
    assert!(
        err.labels()
            .expect("argument-count mismatch should have a diagnostic label")
            .any(|label| label.label() == Some(expected.as_str())),
        "unexpected diagnostic: {err:?}"
    );
    Ok(())
}

/// Test evaluation of a callable that calls another callable.
///
/// This verifies the handling of ControlFlowEffect::Call and ControlFlowEffect::Return, and their
/// interaction with ControlFlowEffect::Yield
#[test]
fn call_handling_test() -> Result<(), Report> {
    let test = Test::named("call_handling").in_module("test");
    let evaluator = HirEvaluator::new(test.context_rc());
    let mut test = EvalTest { test, evaluator };

    test.with_function(&[Type::I1], &[Type::U32]);

    // Define callee
    let callee = test.define_function("callee", &[Type::I1], &[Type::I1]);

    {
        let callee_signature = callee.borrow().get_signature().clone();
        let mut builder = test.function_builder();
        let input = builder.current_block().borrow().arguments()[0] as ValueRef;
        let call = builder.exec(callee, callee_signature, [input], SourceSpan::default())?;
        let cond = call.borrow().results()[0] as ValueRef;
        {
            let call = call.borrow();
            let callee = call.callee();
            assert_eq!(callee.path().name().as_str(), "callee");
        }
        let conditional = builder.r#if(cond, &[Type::U32], SourceSpan::default())?;
        let result = conditional.borrow().results()[0] as ValueRef;
        builder.ret(Some(result), SourceSpan::default())?;

        let then_region = conditional.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        let is_true = builder.u32(1, SourceSpan::default());
        builder.r#yield([is_true], SourceSpan::default())?;

        let else_region = conditional.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let is_false = builder.u32(0, SourceSpan::default());
        builder.r#yield([is_false], SourceSpan::default())?;
    }

    // This function inverts the boolean value it receives and returns it
    {
        let mut builder = FunctionBuilder::new(callee, test.builder_mut());
        let cond = builder.current_block().borrow().arguments()[0] as ValueRef;
        let truthy = builder.i1(true, SourceSpan::default());
        let falsey = builder.i1(false, SourceSpan::default());
        let result = builder.select(cond, falsey, truthy, SourceSpan::default())?;
        builder.ret(Some(result), SourceSpan::default())?;
    }

    let callable = test.function().borrow();
    let results = test.evaluator.eval_callable(&*callable, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    let results = test.evaluator.eval_callable(&*callable, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    Ok(())
}

#[test]
fn inv_zero_reports_error() -> Result<(), Report> {
    let mut test = EvalTest::named("inv_zero");
    test.with_function(&[], &[Type::Felt]);

    {
        let mut builder = test.function_builder();
        let zero = builder.felt(midenc_hir::Felt::ZERO, SourceSpan::default());
        let inverse = builder.inv(zero, SourceSpan::default())?;
        builder.ret(Some(inverse), SourceSpan::default())?;
    }

    let callable = test.function().borrow();
    let _err = test
        .evaluator
        .eval_callable(&*callable, [])
        .expect_err("zero inverse should produce an evaluation error");

    Ok(())
}

#[test]
fn println_collects_printed_lines() -> Result<(), Report> {
    let mut test = EvalTest::named("println_collects_printed_lines");
    test.with_function(&[], &[]);

    {
        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let ptr_ty = Type::from(PointerType::new(Type::U8));
        let base_addr = 64u32;

        for (offset, byte) in b"hello".iter().enumerate() {
            let addr = builder.u32(base_addr + offset as u32, span);
            let ptr = builder.inttoptr(addr, ptr_ty.clone(), span)?;
            let value = builder.u8(*byte, span);
            builder.store(ptr, value, span)?;
        }

        let addr = builder.u32(base_addr, span);
        let ptr = builder.inttoptr(addr, ptr_ty, span)?;
        let len = builder.u32(5, span);
        builder.println(ptr, len, span)?;
        builder.ret(None, span)?;
    }

    let callable = test.function().borrow();
    let results = test.evaluator.eval_callable(&*callable, [])?;
    assert!(results.is_empty());
    assert_eq!(test.evaluator.printed_lines(), ["hello"]);

    Ok(())
}

#[test]
fn println_reports_invalid_utf8() -> Result<(), Report> {
    let mut test = EvalTest::named("println_reports_invalid_utf8");
    test.with_function(&[], &[]);

    {
        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let ptr_ty = Type::from(PointerType::new(Type::U8));
        let addr = builder.u32(96, span);
        let ptr = builder.inttoptr(addr, ptr_ty.clone(), span)?;
        let invalid_utf8 = builder.u8(0xff, span);
        builder.store(ptr, invalid_utf8, span)?;

        let ptr = builder.inttoptr(addr, ptr_ty, span)?;
        let len = builder.u32(1, span);
        builder.println(ptr, len, span)?;
        builder.ret(None, span)?;
    }

    let callable = test.function().borrow();
    test.evaluator
        .eval_callable(&*callable, [])
        .expect_err("invalid UTF-8 should produce an evaluation error");
    assert!(test.evaluator.printed_lines().is_empty());

    Ok(())
}

#[test]
fn wasm_i64_remainder() -> Result<(), Report> {
    let mut test = EvalTest::named("wasm_i64_remainder");
    test.with_function(&[Type::I64, Type::I64], &[Type::I64]);
    {
        let mut builder = test.function_builder();
        let block = builder.current_block();
        let lhs = block.borrow().arguments()[0] as ValueRef;
        let rhs = block.borrow().arguments()[1] as ValueRef;
        let result = builder.i64_rem_s(lhs, rhs, SourceSpan::default())?;
        builder.ret(Some(result), SourceSpan::default())?;
    }
    let function = test.function();
    let callable = function.borrow();
    for (lhs, rhs, expected) in [
        (-7i64, 3i64, -1i64),
        (7, -3, 1),
        (-7, -3, -1),
        (i64::MIN, -1, 0),
        (i64::MAX, i64::MIN, i64::MAX),
    ] {
        let results = test.evaluator.eval_callable(&*callable, [lhs.into(), rhs.into()])?;
        assert_eq!(results.as_slice(), &[Value::Immediate(expected.into())]);
    }
    assert!(test.evaluator.eval_callable(&*callable, [1i64.into(), 0i64.into()]).is_err());
    Ok(())
}

/// Builds and evaluates `fn(value: param) -> result`, whose body `build` computes from `value`.
fn eval_limb_fn(
    param: Type,
    result: Type,
    arg: Immediate,
    build: impl FnOnce(&mut FunctionBuilder<'_, midenc_hir::OpBuilder>, ValueRef) -> ValueRef,
) -> Result<Value, Report> {
    let mut test = EvalTest::named("limbs");
    test.with_function(&[param], &[result]);
    {
        let mut builder = test.function_builder();
        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
        let output = build(&mut builder, value);
        builder.ret(Some(output), SourceSpan::default())?;
    }
    let function = test.function();
    let callable = function.borrow();
    let results = test.evaluator.eval_callable(&*callable, [arg.into()])?;
    Ok(results[0])
}

/// `arith.split` returns limbs most-significant first and `arith.join` takes them in that order:
/// a join of the high felt limb under a zero limb is `x >> 32`, of the low limb above a zero limb
/// is `x << 32`, four `u32` limbs can be reordered, and the two `u64` halves of a `u128` can be
/// swapped.
#[test]
fn split_join_limb_order() -> Result<(), Report> {
    let span = SourceSpan::default();
    let x = 0x1111_1111_2222_2222u64;

    let shr = eval_limb_fn(Type::U64, Type::U64, x.into(), |b, value| {
        let (high, _) = b.split2(value, Type::Felt, span).unwrap();
        let zero = b.felt(midenc_hir::Felt::ZERO, span);
        b.join2(zero, high, Type::U64, span).unwrap()
    })?;
    assert_eq!(shr, Value::Immediate((x >> 32).into()));

    let shl = eval_limb_fn(Type::U64, Type::U64, x.into(), |b, value| {
        let (_, low) = b.split2(value, Type::Felt, span).unwrap();
        let zero = b.felt(midenc_hir::Felt::ZERO, span);
        b.join2(low, zero, Type::U64, span).unwrap()
    })?;
    assert_eq!(shl, Value::Immediate((x << 32).into()));

    let wide = 0x1111_1111_2222_2222_3333_3333_4444_4444u128;
    let reordered = eval_limb_fn(Type::U128, Type::U128, wide.into(), |b, value| {
        let [l0, l1, l2, l3] = b.split4(value, Type::U32, span).unwrap();
        b.join4([l3, l1, l0, l2], Type::U128, span).unwrap()
    })?;
    assert_eq!(
        reordered,
        Value::Immediate(0x4444_4444_2222_2222_1111_1111_3333_3333u128.into())
    );

    let swapped_halves = eval_limb_fn(Type::U128, Type::U128, wide.into(), |b, value| {
        let (high, low) = b.split2(value, Type::U64, span).unwrap();
        b.join2(low, high, Type::U128, span).unwrap()
    })?;
    assert_eq!(
        swapped_halves,
        Value::Immediate(0x3333_3333_4444_4444_1111_1111_2222_2222u128.into())
    );
    Ok(())
}

/// `arith.join` of a felt limb that does not fit in 32 bits is an evaluation error.
#[test]
fn join_of_wide_felt_limb_is_an_error() {
    let span = SourceSpan::default();
    let wide = midenc_hir::Felt::new_unchecked(1 << 40);
    let result = eval_limb_fn(Type::Felt, Type::U64, Immediate::Felt(wide), |b, value| {
        b.join2(value, value, Type::U64, span).unwrap()
    });
    let err = result.expect_err("a felt limb >= 2^32 must not be joined");
    assert!(has_label(&err, "does not fit in 32 bits"), "{err:?}");
}

/// A memory copy operation exercised by the `mem_cpy` and `mem_move` tests.
#[derive(Debug, Clone, Copy)]
enum CopyOp {
    /// `hir.mem_cpy`, whose source and destination ranges must be disjoint.
    MemCpy,
    /// `hir.mem_move`, whose source and destination ranges may overlap.
    MemMove,
}

/// The byte address of the first of the slots used by the `mem_cpy` and `mem_move` tests.
const COPY_BASE_ADDR: u32 = 64;

/// Evaluates a function which stores `values` into consecutive slots of their type and then
/// performs `op` on pointers to that type pointing at slots `src_slot` and `dst_slot`, returning
/// the contents of the slots afterwards.
fn eval_copy<const N: usize>(
    op: CopyOp,
    name: &'static str,
    values: [Immediate; N],
    src_slot: u32,
    dst_slot: u32,
    count: u32,
) -> Result<[Value; N], Report> {
    let value_size = values[0].ty().size_in_bytes() as u32;
    let slot_addr = |slot: u32| COPY_BASE_ADDR + slot * value_size;
    eval_copy_at(op, name, values, slot_addr(src_slot), slot_addr(dst_slot), count)
}

/// Evaluates a function which stores `values` into consecutive slots of their type and then
/// performs `op` on pointers to that type with the raw byte addresses `src_addr` and `dst_addr`,
/// returning the contents of the slots afterwards.
fn eval_copy_at<const N: usize>(
    op: CopyOp,
    name: &'static str,
    values: [Immediate; N],
    src_addr: u32,
    dst_addr: u32,
    count: u32,
) -> Result<[Value; N], Report> {
    let value_ty = values[0].ty();
    let value_size = value_ty.size_in_bytes() as u32;
    let mut test = EvalTest::named(name);
    test.with_function(&[], &[]);

    {
        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let ptr_ty = Type::from(PointerType::new(value_ty.clone()));
        let ptr_at = |builder: &mut FunctionBuilder<'_, _>, addr: u32| {
            let addr = builder.u32(addr, span);
            builder.inttoptr(addr, ptr_ty.clone(), span)
        };

        for (slot, value) in (0u32..).zip(values) {
            let ptr = ptr_at(&mut builder, COPY_BASE_ADDR + slot * value_size)?;
            let value = builder.imm(value, span);
            builder.store(ptr, value, span)?;
        }

        let src = ptr_at(&mut builder, src_addr)?;
        let dst = ptr_at(&mut builder, dst_addr)?;
        let count = builder.u32(count, span);
        match op {
            CopyOp::MemCpy => {
                builder.memcpy(src, dst, count, span)?;
            }
            CopyOp::MemMove => {
                builder.memmove(src, dst, count, span)?;
            }
        }
        builder.ret(None, span)?;
    }

    let callable = test.function().borrow();
    let results = test.evaluator.eval_callable(&*callable, [])?;
    assert!(results.is_empty());

    let mut slots = values.map(Value::Immediate);
    for (slot, value) in (0u32..).zip(slots.iter_mut()) {
        *value = test.evaluator.read_memory(COPY_BASE_ADDR + slot * value_size, &value_ty)?;
    }
    Ok(slots)
}

/// Evaluates [eval_copy] on four u32 slots holding `[1, 2, 3, 4]`.
fn eval_copy_u32(
    op: CopyOp,
    name: &'static str,
    src_slot: u32,
    dst_slot: u32,
    count: u32,
) -> Result<[Value; 4], Report> {
    eval_copy(op, name, [1u32, 2, 3, 4].map(Immediate::U32), src_slot, dst_slot, count)
}

/// Returns the expected contents of the four u32 slots of the `mem_cpy` and `mem_move` tests.
fn u32_slots(values: [u32; 4]) -> [Value; 4] {
    values.map(|value| Value::Immediate(value.into()))
}

/// Returns true if one of the labels of `err` contains `text`.
fn has_label(err: &Report, text: &str) -> bool {
    err.labels()
        .into_iter()
        .flatten()
        .any(|label| label.label().is_some_and(|label| label.contains(text)))
}

/// Checks that `hir.mem_move` with a destination one value above an overlapping source copies the
/// original source values.
///
/// Regression test for #1418.
#[test]
fn mem_move_overlapping_destination_above_source() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemMove, "mem_move_overlapping_destination_above_source", 0, 1, 3)?;
    assert_eq!(slots, u32_slots([1, 1, 2, 3]));
    Ok(())
}

/// Checks that `hir.mem_move` with a destination one value below an overlapping source copies the
/// original source values.
///
/// Regression test for #1418.
#[test]
fn mem_move_overlapping_destination_below_source() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemMove, "mem_move_overlapping_destination_below_source", 1, 0, 3)?;
    assert_eq!(slots, u32_slots([2, 3, 4, 4]));
    Ok(())
}

/// Checks that `hir.mem_move` with a zero count leaves memory unchanged.
#[test]
fn mem_move_zero_count_is_a_noop() -> Result<(), Report> {
    let slots = eval_copy_u32(CopyOp::MemMove, "mem_move_zero_count_is_a_noop", 0, 1, 0)?;
    assert_eq!(slots, u32_slots([1, 2, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_move` with identical source and destination ranges and a non-zero count
/// leaves memory unchanged.
#[test]
fn mem_move_identical_ranges_is_a_noop() -> Result<(), Report> {
    let slots = eval_copy_u32(CopyOp::MemMove, "mem_move_identical_ranges_is_a_noop", 1, 1, 2)?;
    assert_eq!(slots, u32_slots([1, 2, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_move` on `ptr<u8>` operands with a destination one byte above an
/// overlapping source copies the original source bytes.
#[test]
fn mem_move_bytes_overlapping() -> Result<(), Report> {
    let bytes = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let slots = eval_copy(
        CopyOp::MemMove,
        "mem_move_bytes_overlapping",
        bytes.map(Immediate::U8),
        0,
        1,
        7,
    )?;
    let mut expected = bytes;
    expected.copy_within(0..7, 1);
    assert_eq!(slots, expected.map(|byte| Value::Immediate(byte.into())));
    Ok(())
}

/// Checks that `hir.mem_move` whose byte length `count * size_of(pointee)` does not fit in `u32`
/// is an error.
#[test]
fn mem_move_byte_length_overflow_is_an_error() {
    let err = eval_copy_u32(
        CopyOp::MemMove,
        "mem_move_byte_length_overflow_is_an_error",
        0,
        1,
        0x4000_0000,
    )
    .expect_err("expected the byte length overflow to be an error");
    assert!(has_label(&err, "invalid memmove"), "unexpected error: {err:?}");
    assert!(has_label(&err, "overflows"), "unexpected error: {err:?}");
}

/// A byte address whose 16-byte range ends past the evaluator's addressable heap.
const COPY_OUT_OF_BOUNDS_ADDR: u32 = u32::MAX - 7;

/// Checks that a 16-byte `hir.mem_move` whose destination range ends past the addressable heap is
/// an invalid memory write.
#[test]
fn mem_move_out_of_bounds_destination_is_an_error() {
    let err = eval_copy_at(
        CopyOp::MemMove,
        "mem_move_out_of_bounds_destination_is_an_error",
        [0u8; 16].map(Immediate::U8),
        COPY_BASE_ADDR,
        COPY_OUT_OF_BOUNDS_ADDR,
        16,
    )
    .expect_err("expected the out-of-bounds destination to be an error");
    assert!(format!("{err}").contains("invalid memory write"), "unexpected error: {err:?}");
}

/// Checks that a 16-byte `hir.mem_move` whose source range ends past the addressable heap is an
/// invalid memory read.
#[test]
fn mem_move_out_of_bounds_source_is_an_error() {
    let err = eval_copy_at(
        CopyOp::MemMove,
        "mem_move_out_of_bounds_source_is_an_error",
        [0u8; 16].map(Immediate::U8),
        COPY_OUT_OF_BOUNDS_ADDR,
        COPY_BASE_ADDR,
        16,
    )
    .expect_err("expected the out-of-bounds source to be an error");
    assert!(format!("{err}").contains("invalid memory read"), "unexpected error: {err:?}");
}

/// Checks that `hir.mem_cpy` with a destination above a disjoint source copies the source values.
#[test]
fn mem_cpy_disjoint_destination_above_source() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemCpy, "mem_cpy_disjoint_destination_above_source", 0, 3, 1)?;
    assert_eq!(slots, u32_slots([1, 2, 3, 1]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a destination below a disjoint source copies the source values.
#[test]
fn mem_cpy_disjoint_destination_below_source() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemCpy, "mem_cpy_disjoint_destination_below_source", 3, 0, 1)?;
    assert_eq!(slots, u32_slots([4, 2, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a destination range starting right where the source range ends
/// copies the source values.
#[test]
fn mem_cpy_adjacent_destination_above_source() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemCpy, "mem_cpy_adjacent_destination_above_source", 0, 2, 2)?;
    assert_eq!(slots, u32_slots([1, 2, 1, 2]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a source range starting right where the destination range ends
/// copies the source values.
#[test]
fn mem_cpy_adjacent_destination_below_source() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemCpy, "mem_cpy_adjacent_destination_below_source", 2, 0, 2)?;
    assert_eq!(slots, u32_slots([3, 4, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_cpy` on `ptr<u8>` operands with a destination above a disjoint source
/// copies the source bytes.
#[test]
fn mem_cpy_bytes_disjoint() -> Result<(), Report> {
    let bytes = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let slots =
        eval_copy(CopyOp::MemCpy, "mem_cpy_bytes_disjoint", bytes.map(Immediate::U8), 0, 5, 3)?;
    let mut expected = bytes;
    expected.copy_within(0..3, 5);
    assert_eq!(slots, expected.map(|byte| Value::Immediate(byte.into())));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a zero count and identical source and destination pointers
/// leaves memory unchanged.
#[test]
fn mem_cpy_zero_count_identical_pointers_is_a_noop() -> Result<(), Report> {
    let slots =
        eval_copy_u32(CopyOp::MemCpy, "mem_cpy_zero_count_identical_pointers_is_a_noop", 1, 1, 0)?;
    assert_eq!(slots, u32_slots([1, 2, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a zero count and a destination pointer one value above the
/// source pointer leaves memory unchanged.
#[test]
fn mem_cpy_zero_count_overlapping_pointers_is_a_noop() -> Result<(), Report> {
    let slots = eval_copy_u32(
        CopyOp::MemCpy,
        "mem_cpy_zero_count_overlapping_pointers_is_a_noop",
        0,
        1,
        0,
    )?;
    assert_eq!(slots, u32_slots([1, 2, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a destination one value above an overlapping source is an error.
#[test]
fn mem_cpy_overlapping_destination_above_source_is_an_error() {
    let err = eval_copy_u32(
        CopyOp::MemCpy,
        "mem_cpy_overlapping_destination_above_source_is_an_error",
        0,
        1,
        3,
    )
    .expect_err("expected the overlapping ranges to be an error");
    assert!(
        has_label(&err, "invalid memcpy: source and destination ranges must not overlap"),
        "unexpected error: {err:?}"
    );
}

/// Checks that `hir.mem_cpy` with a destination one value below an overlapping source is an error.
#[test]
fn mem_cpy_overlapping_destination_below_source_is_an_error() {
    let err = eval_copy_u32(
        CopyOp::MemCpy,
        "mem_cpy_overlapping_destination_below_source_is_an_error",
        1,
        0,
        3,
    )
    .expect_err("expected the overlapping ranges to be an error");
    assert!(
        has_label(&err, "invalid memcpy: source and destination ranges must not overlap"),
        "unexpected error: {err:?}"
    );
}

/// Checks that `hir.mem_cpy` with identical source and destination ranges and a non-zero count is
/// an error.
#[test]
fn mem_cpy_identical_ranges_is_an_error() {
    let err = eval_copy_u32(CopyOp::MemCpy, "mem_cpy_identical_ranges_is_an_error", 1, 1, 2)
        .expect_err("expected the identical ranges to be an error");
    assert!(
        has_label(&err, "invalid memcpy: source and destination ranges must not overlap"),
        "unexpected error: {err:?}"
    );
}

/// Checks that `hir.mem_cpy` whose byte length `count * size_of(pointee)` does not fit in `u32`
/// is an error.
#[test]
fn mem_cpy_byte_length_overflow_is_an_error() {
    let err = eval_copy_u32(
        CopyOp::MemCpy,
        "mem_cpy_byte_length_overflow_is_an_error",
        0,
        1,
        0x4000_0000,
    )
    .expect_err("expected the byte length overflow to be an error");
    assert!(has_label(&err, "invalid memcpy"), "unexpected error: {err:?}");
    assert!(has_label(&err, "overflows"), "unexpected error: {err:?}");
}

/// Checks that a 16-byte `hir.mem_cpy` whose destination range, disjoint from the source range,
/// ends past the addressable heap is an invalid memory write.
#[test]
fn mem_cpy_out_of_bounds_destination_is_an_error() {
    let err = eval_copy_at(
        CopyOp::MemCpy,
        "mem_cpy_out_of_bounds_destination_is_an_error",
        [0u8; 16].map(Immediate::U8),
        COPY_BASE_ADDR,
        COPY_OUT_OF_BOUNDS_ADDR,
        16,
    )
    .expect_err("expected the out-of-bounds destination to be an error");
    assert!(format!("{err}").contains("invalid memory write"), "unexpected error: {err:?}");
}

/// Checks that a 16-byte `hir.mem_cpy` whose source range, disjoint from the destination range,
/// ends past the addressable heap is an invalid memory read.
#[test]
fn mem_cpy_out_of_bounds_source_is_an_error() {
    let err = eval_copy_at(
        CopyOp::MemCpy,
        "mem_cpy_out_of_bounds_source_is_an_error",
        [0u8; 16].map(Immediate::U8),
        COPY_OUT_OF_BOUNDS_ADDR,
        COPY_BASE_ADDR,
        16,
    )
    .expect_err("expected the out-of-bounds source to be an error");
    assert!(format!("{err}").contains("invalid memory read"), "unexpected error: {err:?}");
}
