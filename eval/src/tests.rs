use alloc::format;
use core::ops::{Deref, DerefMut};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    Builder, Immediate, Op, PointerType, Report, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder},
    testing::Test,
};

use crate::*;

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
    use midenc_dialect_wasm::WasmOpBuilder;

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

/// The byte address of the first of the slots used by the `mem_cpy` tests.
const MEM_CPY_BASE_ADDR: u32 = 64;

/// Evaluates a function which stores `values` into consecutive slots of their type and then
/// performs `hir.mem_cpy` on pointers to that type pointing at slots `src_slot` and `dst_slot`,
/// returning the contents of the slots afterwards.
fn eval_mem_cpy<const N: usize>(
    name: &'static str,
    values: [Immediate; N],
    src_slot: u32,
    dst_slot: u32,
    count: u32,
) -> Result<[Value; N], Report> {
    let value_size = values[0].ty().size_in_bytes() as u32;
    let slot_addr = |slot: u32| MEM_CPY_BASE_ADDR + slot * value_size;
    eval_mem_cpy_at(name, values, slot_addr(src_slot), slot_addr(dst_slot), count)
}

/// Evaluates a function which stores `values` into consecutive slots of their type and then
/// performs `hir.mem_cpy` on pointers to that type with the raw byte addresses `src_addr` and
/// `dst_addr`, returning the contents of the slots afterwards.
fn eval_mem_cpy_at<const N: usize>(
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
            let ptr = ptr_at(&mut builder, MEM_CPY_BASE_ADDR + slot * value_size)?;
            let value = builder.imm(value, span);
            builder.store(ptr, value, span)?;
        }

        let src = ptr_at(&mut builder, src_addr)?;
        let dst = ptr_at(&mut builder, dst_addr)?;
        let count = builder.u32(count, span);
        builder.memcpy(src, dst, count, span)?;
        builder.ret(None, span)?;
    }

    let callable = test.function().borrow();
    let results = test.evaluator.eval_callable(&*callable, [])?;
    assert!(results.is_empty());

    let mut slots = values.map(Value::Immediate);
    for (slot, value) in (0u32..).zip(slots.iter_mut()) {
        *value = test.evaluator.read_memory(MEM_CPY_BASE_ADDR + slot * value_size, &value_ty)?;
    }
    Ok(slots)
}

/// Evaluates [eval_mem_cpy] on four u32 slots holding `[1, 2, 3, 4]`.
fn eval_mem_cpy_u32(
    name: &'static str,
    src_slot: u32,
    dst_slot: u32,
    count: u32,
) -> Result<[Value; 4], Report> {
    eval_mem_cpy(name, [1u32, 2, 3, 4].map(Immediate::U32), src_slot, dst_slot, count)
}

/// Returns the expected contents of the four u32 slots of the `mem_cpy` tests.
fn u32_slots(values: [u32; 4]) -> [Value; 4] {
    values.map(|value| Value::Immediate(value.into()))
}

/// Checks that `hir.mem_cpy` with a destination one value above an overlapping source copies the
/// original source values.
///
/// Regression test for #1418.
#[test]
fn mem_cpy_overlapping_destination_above_source() -> Result<(), Report> {
    let slots = eval_mem_cpy_u32("mem_cpy_overlapping_destination_above_source", 0, 1, 3)?;
    assert_eq!(slots, u32_slots([1, 1, 2, 3]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a destination one value below an overlapping source copies the
/// original source values.
///
/// Regression test for #1418.
#[test]
fn mem_cpy_overlapping_destination_below_source() -> Result<(), Report> {
    let slots = eval_mem_cpy_u32("mem_cpy_overlapping_destination_below_source", 1, 0, 3)?;
    assert_eq!(slots, u32_slots([2, 3, 4, 4]));
    Ok(())
}

/// Checks that `hir.mem_cpy` with a zero count leaves memory unchanged.
#[test]
fn mem_cpy_zero_count_is_a_noop() -> Result<(), Report> {
    let slots = eval_mem_cpy_u32("mem_cpy_zero_count_is_a_noop", 0, 1, 0)?;
    assert_eq!(slots, u32_slots([1, 2, 3, 4]));
    Ok(())
}

/// Checks that `hir.mem_cpy` on `ptr<u8>` operands with a destination one byte above an
/// overlapping source copies the original source bytes.
#[test]
fn mem_cpy_bytes_overlapping() -> Result<(), Report> {
    let bytes = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let slots = eval_mem_cpy("mem_cpy_bytes_overlapping", bytes.map(Immediate::U8), 0, 1, 7)?;
    let mut expected = bytes;
    expected.copy_within(0..7, 1);
    assert_eq!(slots, expected.map(|byte| Value::Immediate(byte.into())));
    Ok(())
}

/// Checks that `hir.mem_cpy` whose byte length `count * size_of(pointee)` does not fit in `u32`
/// is an error.
#[test]
fn mem_cpy_byte_length_overflow_is_an_error() {
    let err = eval_mem_cpy_u32("mem_cpy_byte_length_overflow_is_an_error", 0, 1, 0x4000_0000)
        .expect_err("expected the byte length overflow to be an error");
    let overflow_label = err
        .labels()
        .into_iter()
        .flatten()
        .any(|label| label.label().is_some_and(|label| label.contains("overflows")));
    assert!(overflow_label, "unexpected error: {err:?}");
}

/// A byte address whose 16-byte range ends past the evaluator's addressable heap.
const MEM_CPY_OUT_OF_BOUNDS_ADDR: u32 = u32::MAX - 7;

/// Checks that a 16-byte `hir.mem_cpy` whose destination range ends past the addressable heap is
/// an invalid memory write.
#[test]
fn mem_cpy_out_of_bounds_destination_is_an_error() {
    let err = eval_mem_cpy_at(
        "mem_cpy_out_of_bounds_destination_is_an_error",
        [0u8; 16].map(Immediate::U8),
        MEM_CPY_BASE_ADDR,
        MEM_CPY_OUT_OF_BOUNDS_ADDR,
        16,
    )
    .expect_err("expected the out-of-bounds destination to be an error");
    assert!(format!("{err}").contains("invalid memory write"), "unexpected error: {err:?}");
}

/// Checks that a 16-byte `hir.mem_cpy` whose source range ends past the addressable heap is an
/// invalid memory read.
#[test]
fn mem_cpy_out_of_bounds_source_is_an_error() {
    let err = eval_mem_cpy_at(
        "mem_cpy_out_of_bounds_source_is_an_error",
        [0u8; 16].map(Immediate::U8),
        MEM_CPY_OUT_OF_BOUNDS_ADDR,
        MEM_CPY_BASE_ADDR,
        16,
    )
    .expect_err("expected the out-of-bounds source to be an error");
    assert!(format!("{err}").contains("invalid memory read"), "unexpected error: {err:?}");
}
