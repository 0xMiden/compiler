//! A 128-bit integer has its least significant limb on top of the operand stack, as in the core
//! library's `u128`. The values below have four different limbs, so an operation that took or
//! checked the wrong ones returns a different value, or traps. One test runs the 64-bit cast to
//! `i32`, which shares its range check with the 128-bit casts.

use miden_debug::ToMidenRepr;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Felt, OpBuilder, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder},
};

use super::support::{UnaryOp, assert_traps, cast, compile, run, run_args, trunc};
use crate::testing::compile_test_module;

/// A `u128` whose limbs, least significant first, are `0x11111111`, `0x22222222`, `0x33333333`
/// and `0x44444444`.
const LIMBS: u128 = 0x4444_4444_3333_3333_2222_2222_1111_1111;

/// The low half of [LIMBS], which fits in a felt.
const LOW_HALF: u64 = LIMBS as u64;

#[test]
fn trunc_of_a_u128_to_u64_keeps_the_low_half() {
    let (package, context) = compile(Type::U128, Type::U64, trunc);
    assert_eq!(run::<u64>(&package, &context, LIMBS), LOW_HALF);
}

#[test]
fn trunc_of_a_u128_to_u32_keeps_the_least_significant_limb() {
    let (package, context) = compile(Type::U128, Type::U32, trunc);
    assert_eq!(run::<u32>(&package, &context, LIMBS), 0x1111_1111);
}

#[test]
fn trunc_of_a_u128_to_felt_keeps_the_low_half() {
    let (package, context) = compile(Type::U128, Type::Felt, trunc);
    assert_eq!(run::<Felt>(&package, &context, LIMBS), Felt::new_unchecked(LOW_HALF));
}

#[test]
fn trunc_of_an_i128_to_i64_keeps_the_low_half() {
    let (package, context) = compile(Type::I128, Type::I64, trunc);
    let value = -(LIMBS as i128);
    assert_eq!(run::<i64>(&package, &context, value), value as i64);
}

/// A `u128` whose high half is zero casts to its low half; any other traps.
#[test]
fn cast_of_a_u128_to_u64_checks_the_high_half_is_zero() {
    let (package, context) = compile(Type::U128, Type::U64, cast);
    assert_eq!(run::<u64>(&package, &context, LOW_HALF as u128), LOW_HALF);
    for value in [LIMBS, 1 << 64, 1 << 96] {
        assert_traps(&package, &context, value, "128-bit value does not fit in u64");
    }
}

/// A `u128` whose three high limbs are zero casts to its least significant limb; any other traps.
#[test]
fn cast_of_a_u128_to_u32_checks_the_high_limbs_are_zero() {
    let (package, context) = compile(Type::U128, Type::U32, cast);
    assert_eq!(run::<u32>(&package, &context, 0x1111_1111u128), 0x1111_1111);
    for value in [LIMBS, 1 << 32, 1 << 64, 1 << 96] {
        assert_traps(&package, &context, value, "128-bit value does not fit in u32");
    }
}

/// A `u128` whose high half is zero casts to the felt of its low half; any other traps.
#[test]
fn cast_of_a_u128_to_felt_checks_the_high_half_is_zero() {
    let (package, context) = compile(Type::U128, Type::Felt, cast);
    assert_eq!(run::<Felt>(&package, &context, LOW_HALF as u128), Felt::new_unchecked(LOW_HALF));
    assert_traps(&package, &context, LIMBS, "128-bit value does not fit in u64");
}

/// An `i128` whose high half extends the sign of its low half casts to its low half; any other
/// traps. `-(1 << 96)` fails only the check of `x3`: its other limbs are those of 0.
#[test]
fn cast_of_an_i128_to_i64_checks_the_high_half_extends_the_sign() {
    let (package, context) = compile(Type::I128, Type::I64, cast);
    for value in [LOW_HALF as i64 as i128, -(LOW_HALF as i64 as i128), i64::MIN as i128] {
        assert_eq!(run::<i64>(&package, &context, value), value as i64);
    }
    for value in [
        LIMBS as i128,
        -(LIMBS as i128),
        i64::MAX as i128 + 1,
        i64::MIN as i128 - 1,
        -(1 << 96),
    ] {
        assert_traps(&package, &context, value, "128-bit value does not fit in i64");
    }
}

/// An `i128` in the `i32` range casts to its least significant limb; any other traps.
#[test]
fn cast_of_an_i128_to_i32_checks_the_value_is_in_range() {
    let (package, context) = compile(Type::I128, Type::I32, cast);
    for value in [0x1111_1111, -0x1111_1111, i32::MAX as i128, i32::MIN as i128] {
        assert_eq!(run::<i32>(&package, &context, value), value as i32);
    }
    for value in [i32::MAX as i128 + 1, i32::MIN as i128 - 1, -(1 << 32)] {
        assert_traps(&package, &context, value, "i64 value does not fit in signed 32-bit range");
    }
    assert_traps(&package, &context, LIMBS as i128, "128-bit value does not fit in i64");
}

/// An `i128` in the `i16` range casts to its 16-bit pattern, with nothing above bit 15 even when
/// it is negative; one past either bound traps.
#[test]
fn cast_of_an_i128_to_i16_checks_the_value_is_in_range() {
    let (package, context) = compile(Type::I128, Type::I16, cast);
    for value in [i16::MIN, -1, i16::MAX] {
        let pattern = Felt::new_unchecked(value as u16 as u64);
        assert_eq!(run::<Felt>(&package, &context, value as i128), pattern, "{value}");
    }
    for value in [i16::MIN as i128 - 1, i16::MAX as i128 + 1] {
        assert_traps(&package, &context, value, "i64 value does not fit in signed 16-bit range");
    }
}

/// An `i128` in the `i8` range casts to its 8-bit pattern, with nothing above bit 7 even when it
/// is negative; one past either bound traps.
#[test]
fn cast_of_an_i128_to_i8_checks_the_value_is_in_range() {
    let (package, context) = compile(Type::I128, Type::I8, cast);
    for value in [i8::MIN, -1, i8::MAX] {
        let pattern = Felt::new_unchecked(value as u8 as u64);
        assert_eq!(run::<Felt>(&package, &context, value as i128), pattern, "{value}");
    }
    for value in [i8::MIN as i128 - 1, i8::MAX as i128 + 1] {
        assert_traps(&package, &context, value, "i64 value does not fit in signed 8-bit range");
    }
}

/// The 64-bit cast to `i32` shares its range check with the 128-bit one: an `i64` in the `i32`
/// range casts to its low limb, and `-2^32` traps.
#[test]
fn cast_of_an_i64_to_i32_checks_the_value_is_in_range() {
    let (package, context) = compile(Type::I64, Type::I32, cast);
    for value in [-1, i32::MIN as i64] {
        assert_eq!(run::<i32>(&package, &context, value), value as i32);
    }
    assert_traps(
        &package,
        &context,
        -(1i64 << 32),
        "i64 value does not fit in signed 32-bit range",
    );
}

/// A felt cast to a 128-bit integer keeps its value: the split felt is the low half.
#[test]
fn cast_of_a_felt_to_128_bits_keeps_its_value() {
    let felt = Felt::new_unchecked(LOW_HALF);
    let (package, context) = compile(Type::Felt, Type::U128, cast);
    assert_eq!(run::<u128>(&package, &context, felt), LOW_HALF as u128);
    let (package, context) = compile(Type::Felt, Type::I128, cast);
    assert_eq!(run::<i128>(&package, &context, felt), LOW_HALF as i128);
}

fn is_odd(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.is_odd(value, SourceSpan::default()).unwrap()
}

fn clz(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.clz(value, SourceSpan::default()).unwrap()
}

fn clo(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.clo(value, SourceSpan::default()).unwrap()
}

fn ctz(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.ctz(value, SourceSpan::default()).unwrap()
}

fn cto(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.cto(value, SourceSpan::default()).unwrap()
}

fn bnot(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.bnot(value, SourceSpan::default()).unwrap()
}

fn assert(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.assert(value, SourceSpan::default()).unwrap()
}

/// A `u128` is odd when its least significant limb is: `LIMBS` has an odd low limb and an even high
/// one, and the other value the reverse.
#[test]
fn is_odd_of_a_u128_tests_its_least_significant_limb() {
    let (package, context) = compile(Type::U128, Type::I1, is_odd);
    assert!(run::<bool>(&package, &context, LIMBS));
    assert!(!run::<bool>(&package, &context, 0x4444_4445_3333_3333_2222_2222_1111_1110u128));
}

/// Values that separate the limbs, the halves, and the point where one half's count carries into
/// the other's.
const COUNT_INPUTS: [u128; 8] = [0, 1, 1 << 31, 1 << 63, 1 << 64, 1 << 100, 1 << 127, u128::MAX];

/// Run the count `op` of a `u128` on each of `inputs`, and compare it with `expected`.
fn check_count(op: UnaryOp, inputs: &[u128], expected: fn(u128) -> u32) {
    let (package, context) = compile(Type::U128, Type::U32, op);
    for &value in inputs {
        assert_eq!(run::<u32>(&package, &context, value), expected(value), "input {value:#x}");
    }
}

/// `COUNT_INPUTS` and their complements, for the ones-counting forms.
fn with_complements() -> Vec<u128> {
    COUNT_INPUTS.iter().flat_map(|&value| [value, !value]).collect()
}

#[test]
fn clz_of_a_u128_counts_from_its_most_significant_limb() {
    check_count(clz, &COUNT_INPUTS, u128::leading_zeros);
}

#[test]
fn clo_of_a_u128_counts_from_its_most_significant_limb() {
    check_count(clo, &with_complements(), u128::leading_ones);
}

#[test]
fn ctz_of_a_u128_counts_from_its_least_significant_limb() {
    check_count(ctz, &COUNT_INPUTS, u128::trailing_zeros);
}

#[test]
fn cto_of_a_u128_counts_from_its_least_significant_limb() {
    check_count(cto, &with_complements(), u128::trailing_ones);
}

/// `u128` wrapping addition carries out of each limb and wraps at the top.
#[test]
fn wrapping_add_of_u128s_carries_between_the_halves_and_wraps() {
    let (package, context) =
        compile_test_module([Type::U128, Type::U128], [Type::U128], |builder| {
            let args = builder.current_block().borrow().arguments().to_vec();
            let (a, b) = (args[0] as ValueRef, args[1] as ValueRef);
            let sum = builder.add_wrapping(a, b, SourceSpan::default()).unwrap();
            builder.ret(Some(sum), SourceSpan::default()).unwrap();
        });
    for (a, b) in [(u64::MAX as u128, 1), (u128::MAX, 2), (LIMBS, LIMBS)] {
        let mut args = Vec::new();
        a.push_to_operand_stack(&mut args);
        b.push_to_operand_stack(&mut args);
        assert_eq!(
            run_args::<u128>(&package, &context, &args),
            a.wrapping_add(b),
            "{a:#x} + {b:#x}"
        );
    }
}

/// `u128` wrapping subtraction borrows across each limb and wraps at the bottom.
#[test]
fn wrapping_sub_of_u128s_borrows_between_the_halves_and_wraps() {
    let (package, context) =
        compile_test_module([Type::U128, Type::U128], [Type::U128], |builder| {
            let args = builder.current_block().borrow().arguments().to_vec();
            let (a, b) = (args[0] as ValueRef, args[1] as ValueRef);
            let difference = builder.sub_wrapping(a, b, SourceSpan::default()).unwrap();
            builder.ret(Some(difference), SourceSpan::default()).unwrap();
        });
    for (a, b) in [(1u128 << 64, 1), (0, 1), (LIMBS, !LIMBS)] {
        let mut args = Vec::new();
        a.push_to_operand_stack(&mut args);
        b.push_to_operand_stack(&mut args);
        assert_eq!(
            run_args::<u128>(&package, &context, &args),
            a.wrapping_sub(b),
            "{a:#x} - {b:#x}"
        );
    }
}

/// The bitwise complement of a `u128` inverts its four limbs in place.
#[test]
fn bnot_of_a_u128_inverts_each_limb_in_place() {
    let (package, context) = compile(Type::U128, Type::U128, bnot);
    assert_eq!(run::<u128>(&package, &context, LIMBS), !LIMBS);
}

/// A `u128` below `2^15` casts to `i16`; one at or above it traps, and `0xFFFF_FFFF` is not `-1`.
#[test]
fn cast_of_a_u128_to_i16_checks_it_is_below_2_to_the_15() {
    let (package, context) = compile(Type::U128, Type::I16, cast);
    assert_eq!(run::<i16>(&package, &context, i16::MAX as u128), i16::MAX);
    assert_traps(
        &package,
        &context,
        i16::MAX as u128 + 1,
        "16-bit integer signedness check failed",
    );
    assert_traps(
        &package,
        &context,
        0xffff_ffffu128,
        "value does not fit in unsigned 16-bit range",
    );
}

/// A `u128` below `2^7` casts to `i8`; one at or above it traps, and `0xFFFF_FFFF` is not `-1`.
#[test]
fn cast_of_a_u128_to_i8_checks_it_is_below_2_to_the_7() {
    let (package, context) = compile(Type::U128, Type::I8, cast);
    assert_eq!(run::<i8>(&package, &context, i8::MAX as u128), i8::MAX);
    assert_traps(&package, &context, i8::MAX as u128 + 1, "8-bit integer signedness check failed");
    assert_traps(
        &package,
        &context,
        0xffff_ffffu128,
        "value does not fit in unsigned 8-bit range",
    );
}

/// A negative `i64` does not fit in a `u128`.
#[test]
fn cast_of_an_i64_to_u128_checks_it_is_non_negative() {
    let (package, context) = compile(Type::I64, Type::U128, cast);
    assert_eq!(run::<u128>(&package, &context, i64::MAX), i64::MAX as u128);
    assert_traps(&package, &context, -1i64, "expected a non-negative i64 value");
}

/// `hir.assert` of a `u128` checks the value is 1, its least significant limb.
#[test]
fn assert_of_a_u128_checks_it_is_one() {
    let (package, context) = compile(Type::U128, Type::U128, assert);
    assert_eq!(run::<u128>(&package, &context, 1u128), 1);
    assert_traps(&package, &context, 1u128 << 96, "expected u128 value to equal 1");
}
