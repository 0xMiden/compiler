//! A 64-bit integer is `[lo, hi]` on the operand stack, the low limb on top, as in the core
//! library's `u64`. The values below have different limbs, so an operation that took or checked the
//! wrong one returns a different value, or traps.

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{Felt, OpBuilder, SourceSpan, Type, ValueRef, dialects::builtin::FunctionBuilder};

use super::support::{assert_traps, cast, compile, run};

/// The field modulus.
const P: u64 = 0xffff_ffff_0000_0001;

fn is_odd(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.is_odd(value, SourceSpan::default()).unwrap()
}

fn pow2(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.pow2(value, SourceSpan::default()).unwrap()
}

fn assert(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    builder.assert(value, SourceSpan::default()).unwrap()
}

/// What `Felt::as_u64` lowers to: the felt as a `u64`, carried in Wasm's `i64`.
fn felt_as_u64(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, _: Type) -> ValueRef {
    let value = builder.cast(value, Type::U64, SourceSpan::default()).unwrap();
    builder.bitcast(value, Type::I64, SourceSpan::default()).unwrap()
}

/// A `u64` is odd when its low limb is: `2^32` has an even low limb and an odd high one, and the
/// other value the reverse.
#[test]
fn is_odd_of_a_u64_tests_its_low_limb() {
    let (package, context) = compile(Type::U64, Type::I1, is_odd);
    assert!(!run::<bool>(&package, &context, 1u64 << 32));
    assert!(run::<bool>(&package, &context, 0x2_0000_0001u64));
}

/// `pow2` of a `u64` takes its exponent from the low limb; `2^64` does not fit, and neither does
/// an exponent with anything in its high limb.
#[test]
fn pow2_of_a_u64_raises_2_to_its_low_limb() {
    let (package, context) = compile(Type::U64, Type::U64, pow2);
    for exponent in [0u64, 1, 31, 32, 63] {
        assert_eq!(run::<u64>(&package, &context, exponent), 1 << exponent, "2^{exponent}");
    }
    assert_traps(&package, &context, 64u64, "assertion failed");
    assert_traps(&package, &context, 1u64 << 32, "u64 exponent for pow2 must fit in u32");
}

/// An `i64` casts to `i16` or `i8` exactly when it is in the type's range, and a negative result is
/// the N-bit pattern.
#[test]
fn cast_of_an_i64_to_a_narrow_signed_type_checks_the_range_and_keeps_n_bits() {
    for (ty, min, max) in [
        (Type::I16, i16::MIN as i64, i16::MAX as i64),
        (Type::I8, i8::MIN as i64, i8::MAX as i64),
    ] {
        let bits = ty.size_in_bits();
        let pattern = |value: i64| Felt::new_unchecked(value as u64 & ((1u64 << bits) - 1));
        let (package, context) = compile(Type::I64, ty.clone(), cast);
        for value in [min, -1, 0, max] {
            assert_eq!(run::<Felt>(&package, &context, value), pattern(value), "{value} to {ty}");
        }
        let message = format!("i64 value does not fit in signed {bits}-bit range");
        for value in [min - 1, max + 1, -(1 << 32)] {
            assert_traps(&package, &context, value, &message);
        }
    }
}

/// `hir.assert` of a `u64` checks the value is 1, in its low limb.
#[test]
fn assert_of_a_u64_checks_it_is_one() {
    let (package, context) = compile(Type::U64, Type::U64, assert);
    assert_eq!(run::<u64>(&package, &context, 1u64), 1);
    assert_traps(&package, &context, 1u64 << 32, "expected u64 value to equal 1");
}

/// A `u64` casts to `i64` exactly when it is at most `i64::MAX`.
#[test]
fn cast_of_a_u64_to_i64_checks_it_is_at_most_i64_max() {
    let (package, context) = compile(Type::U64, Type::I64, cast);
    assert_eq!(run::<i64>(&package, &context, i64::MAX as u64), i64::MAX);
    assert_traps(&package, &context, 1u64 << 63, "value does not fit in i64");
}

/// An `i64` casts to a felt exactly when it is non-negative: a negative one does not trap only by
/// exceeding the modulus as a `u64`.
#[test]
fn cast_of_an_i64_to_felt_checks_it_is_non_negative() {
    let (package, context) = compile(Type::I64, Type::Felt, cast);
    for value in [0, i64::MAX] {
        assert_eq!(run::<Felt>(&package, &context, value), Felt::new_unchecked(value as u64));
    }
    for value in [i64::MIN, -(1i64 << 32), -1] {
        assert_traps(&package, &context, value, "expected a non-negative i64 value");
    }
}

/// A non-negative `i32` casts to the `u64` of the same value, its zero high limb below it.
#[test]
fn cast_of_an_i32_to_u64_zero_extends_a_non_negative_value() {
    let (package, context) = compile(Type::I32, Type::U64, cast);
    for value in [5, i32::MAX] {
        assert_eq!(run::<u64>(&package, &context, value), value as u64);
    }
    for value in [i32::MIN, -1] {
        assert_traps(&package, &context, value, "expected a non-negative i32 value");
    }
}

/// A felt casts to `i64` exactly when it is below `2^63`.
#[test]
fn cast_of_a_felt_to_i64_checks_it_is_below_2_to_the_63() {
    let (package, context) = compile(Type::Felt, Type::I64, cast);
    assert_eq!(run::<i64>(&package, &context, Felt::new_unchecked(i64::MAX as u64)), i64::MAX);
    assert_traps(
        &package,
        &context,
        Felt::new_unchecked(1 << 63),
        "felt value does not fit in i64",
    );
}

/// `Felt::as_u64` returns the felt unchanged, at or above `2^63` too: the frontend casts it to `u64`
/// and reinterprets that as Wasm's `i64`.
#[test]
fn felt_as_u64_returns_every_felt_unchanged() {
    let (package, context) = compile(Type::Felt, Type::I64, felt_as_u64);
    for value in [5, 1 << 63, P - 1] {
        assert_eq!(run::<i64>(&package, &context, Felt::new_unchecked(value)), value as i64);
    }
}

/// A `u64` casts to `i16` exactly when it is below `2^15`.
#[test]
fn cast_of_a_u64_to_i16_checks_it_is_below_2_to_the_15() {
    let (package, context) = compile(Type::U64, Type::I16, cast);
    assert_eq!(run::<i16>(&package, &context, i16::MAX as u64), i16::MAX);
    let past_max = i16::MAX as u64 + 1;
    assert_traps(&package, &context, past_max, "16-bit integer signedness check failed");
    assert_traps(
        &package,
        &context,
        1u64 << 32,
        "u64 value does not fit in unsigned 16-bit range",
    );
}
