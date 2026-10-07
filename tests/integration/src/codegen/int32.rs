//! Casts to 32 bits and below check exactly that the value is representable in the destination
//! type, deciding the range by the source's signedness: an unsigned or felt source is never
//! negative. A narrow signed integer is its N-bit pattern, zero-extended: `-1i16` is `0x0000FFFF`.
//! The harness reads an `i16` or `i8` result from its low bits only, so the tests of that pattern
//! read the raw element.

use midenc_hir::{Felt, Type};

use super::support::{assert_traps, cast, compile, run};

/// A felt casts to an unsigned type exactly when it is at most the type's maximum.
#[test]
fn cast_of_a_felt_to_an_unsigned_type_checks_it_is_at_most_the_maximum() {
    for (ty, max, message) in [
        (Type::U32, u32::MAX as u64, "felt value does not fit in 32 bits"),
        (Type::U16, u16::MAX as u64, "value does not fit in unsigned 16-bit range"),
        (Type::U8, u8::MAX as u64, "value does not fit in unsigned 8-bit range"),
    ] {
        let (package, context) = compile(Type::Felt, ty.clone(), cast);
        for value in [5, max] {
            let output = run::<Felt>(&package, &context, Felt::new_unchecked(value));
            assert_eq!(output, Felt::new_unchecked(value), "{value} to {ty}");
        }
        assert_traps(&package, &context, Felt::new_unchecked(max + 1), message);
        let two_to_the_32 = Felt::new_unchecked(1 << 32);
        assert_traps(&package, &context, two_to_the_32, "felt value does not fit in 32 bits");
    }
}

/// A felt casts to a signed type exactly when it is at most the type's maximum; `0xFFFF_FFFF` is
/// not `-1`.
#[test]
fn cast_of_a_felt_to_a_signed_type_checks_it_is_at_most_the_maximum() {
    for (ty, max, past_max, all_ones) in [
        (
            Type::I32,
            i32::MAX as u64,
            "expected a non-negative i32 value",
            "expected a non-negative i32 value",
        ),
        (
            Type::I16,
            i16::MAX as u64,
            "16-bit integer signedness check failed",
            "value does not fit in unsigned 16-bit range",
        ),
        (
            Type::I8,
            i8::MAX as u64,
            "8-bit integer signedness check failed",
            "value does not fit in unsigned 8-bit range",
        ),
    ] {
        let (package, context) = compile(Type::Felt, ty.clone(), cast);
        let output = run::<Felt>(&package, &context, Felt::new_unchecked(max));
        assert_eq!(output, Felt::new_unchecked(max), "{max} to {ty}");
        assert_traps(&package, &context, Felt::new_unchecked(max + 1), past_max);
        assert_traps(&package, &context, Felt::new_unchecked(0xffff_ffff), all_ones);
    }
}

/// A `u32` casts to `i16` or `i8` exactly when it is at most the type's maximum; `0xFFFF_FFFF` is
/// not `-1`.
#[test]
fn cast_of_a_u32_to_a_narrow_signed_type_checks_it_is_at_most_the_maximum() {
    for (ty, max) in [(Type::I16, i16::MAX as u32), (Type::I8, i8::MAX as u32)] {
        let bits = ty.size_in_bits();
        let (package, context) = compile(Type::U32, ty.clone(), cast);
        let output = run::<Felt>(&package, &context, max);
        assert_eq!(output, Felt::new_unchecked(max as u64), "{max} to {ty}");
        let past_max = format!("{bits}-bit integer signedness check failed");
        assert_traps(&package, &context, max + 1, &past_max);
        let all_ones = format!("value does not fit in unsigned {bits}-bit range");
        assert_traps(&package, &context, 0xffff_ffffu32, &all_ones);
    }
}

/// A `u32` casts to `i32` exactly when it is at most `i32::MAX`.
#[test]
fn cast_of_a_u32_to_i32_checks_it_is_at_most_i32_max() {
    let (package, context) = compile(Type::U32, Type::I32, cast);
    assert_eq!(run::<i32>(&package, &context, i32::MAX as u32), i32::MAX);
    for value in [1u32 << 31, u32::MAX] {
        assert_traps(&package, &context, value, "value does not fit in i32");
    }
}

/// An `i32` casts to `u32` exactly when it is non-negative.
#[test]
fn cast_of_an_i32_to_u32_checks_it_is_non_negative() {
    let (package, context) = compile(Type::I32, Type::U32, cast);
    assert_eq!(run::<u32>(&package, &context, i32::MAX), i32::MAX as u32);
    for value in [i32::MIN, -1] {
        assert_traps(&package, &context, value, "expected a non-negative i32 value");
    }
}

/// An `i32` casts to `i16` or `i8` exactly when it is in the type's range, and a negative result is
/// the N-bit pattern.
#[test]
fn cast_of_an_i32_to_a_narrow_signed_type_checks_the_range_and_keeps_n_bits() {
    for (ty, min, max) in [
        (Type::I16, i16::MIN as i32, i16::MAX as i32),
        (Type::I8, i8::MIN as i32, i8::MAX as i32),
    ] {
        let bits = ty.size_in_bits();
        let pattern = |value: i32| Felt::new_unchecked(value as u32 as u64 & ((1u64 << bits) - 1));
        let (package, context) = compile(Type::I32, ty.clone(), cast);
        for value in [min, -1, 0, max] {
            assert_eq!(run::<Felt>(&package, &context, value), pattern(value), "{value} to {ty}");
        }
        let message = format!("value does not fit in signed {bits}-bit range");
        for value in [min - 1, max + 1, 70000, i32::MIN] {
            assert_traps(&package, &context, value, &message);
        }
    }
}

/// An `i16` casts to `i8` exactly when it is in the `i8` range, and a negative result is the 8-bit
/// pattern.
#[test]
fn cast_of_an_i16_to_i8_checks_the_range_and_keeps_8_bits() {
    let (package, context) = compile(Type::I16, Type::I8, cast);
    for value in [i8::MIN as i16, -1, 0, i8::MAX as i16] {
        let pattern = Felt::new_unchecked(value as u8 as u64);
        assert_eq!(run::<Felt>(&package, &context, value), pattern, "{value} to i8");
    }
    for value in [i8::MIN as i16 - 1, i8::MAX as i16 + 1, i16::MIN, i16::MAX] {
        assert_traps(&package, &context, value, "value does not fit in signed 8-bit range");
    }
}

/// An `i16` casts to `u8` exactly when it is in the `u8` range; a negative `i16` is not.
#[test]
fn cast_of_an_i16_to_u8_checks_it_is_in_the_u8_range() {
    let (package, context) = compile(Type::I16, Type::U8, cast);
    for value in [0, 200, u8::MAX as i16] {
        assert_eq!(run::<u8>(&package, &context, value), value as u8, "{value} to u8");
    }
    for value in [-1, i16::MIN, u8::MAX as i16 + 1] {
        assert_traps(&package, &context, value, "value does not fit in unsigned 8-bit range");
    }
}

/// An `i16` casts to `u16`, and an `i8` to `u8`, exactly when it is non-negative.
#[test]
fn cast_of_a_narrow_signed_type_to_its_unsigned_type_checks_it_is_non_negative() {
    let (package, context) = compile(Type::I16, Type::U16, cast);
    for value in [0, i16::MAX] {
        assert_eq!(run::<u16>(&package, &context, value), value as u16, "{value} to u16");
    }
    for value in [-1, i16::MIN] {
        assert_traps(&package, &context, value, "16-bit integer signedness check failed");
    }
    let (package, context) = compile(Type::I8, Type::U8, cast);
    for value in [0, i8::MAX] {
        assert_eq!(run::<u8>(&package, &context, value), value as u8, "{value} to u8");
    }
    for value in [-1, i8::MIN] {
        assert_traps(&package, &context, value, "8-bit integer signedness check failed");
    }
}

/// An `i8` casts to `u32` exactly when it is non-negative.
#[test]
fn cast_of_an_i8_to_u32_checks_it_is_non_negative() {
    let (package, context) = compile(Type::I8, Type::U32, cast);
    assert_eq!(run::<u32>(&package, &context, i8::MAX), i8::MAX as u32);
    assert_traps(&package, &context, -1i8, "8-bit integer signedness check failed");
}
