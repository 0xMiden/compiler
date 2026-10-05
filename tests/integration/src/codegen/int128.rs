//! Narrowing a 128-bit integer keeps its low limbs: the least significant limb is on top of the
//! operand stack, as in the core library's `u128`. The values below have four different limbs, so
//! a conversion that took or checked the wrong ones returns a different value, or traps. One test
//! runs the 64-bit cast to `i32`, which shares its range check with the 128-bit casts.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::Arc,
};

use miden_debug::{FromMidenRepr, ToMidenRepr};
use miden_mast_package::Package;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Context, Felt, OpBuilder, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder},
};

use crate::{
    testing::{compile_test_module, eval_package},
    trap_helpers::{panic_message, trap_matches},
};

/// A `u128` whose limbs, least significant first, are `0x11111111`, `0x22222222`, `0x33333333`
/// and `0x44444444`.
const LIMBS: u128 = 0x4444_4444_3333_3333_2222_2222_1111_1111;

/// The low half of [LIMBS], which fits in a felt.
const LOW_HALF: u64 = LIMBS as u64;

/// A conversion of a value to a type.
type Conversion = fn(&mut FunctionBuilder<'_, OpBuilder>, ValueRef, Type) -> ValueRef;

fn trunc(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, ty: Type) -> ValueRef {
    builder.trunc(value, ty, SourceSpan::default()).unwrap()
}

fn cast(builder: &mut FunctionBuilder<'_, OpBuilder>, value: ValueRef, ty: Type) -> ValueRef {
    builder.cast(value, ty, SourceSpan::default()).unwrap()
}

/// Compile an entrypoint that converts its argument, of type `src`, to `dst` with `convert`.
fn compile(src: Type, dst: Type, convert: Conversion) -> (Arc<Package>, Rc<Context>) {
    compile_test_module([src], [dst.clone()], move |builder| {
        let input = builder.current_block().borrow().arguments()[0] as ValueRef;
        let output = convert(builder, input, dst.clone());
        builder.ret(Some(output), SourceSpan::default()).unwrap();
    })
}

/// Run `package` on `input`, its least significant limb on top of the operand stack.
fn run<T>(package: &Arc<Package>, context: &Rc<Context>, input: impl ToMidenRepr) -> T
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
{
    let mut args = Vec::new();
    input.push_to_operand_stack(&mut args);
    eval_package::<T, _, _>(package.clone(), None, &args, context.session(), |_| Ok(())).unwrap()
}

/// Run `package` on `input`, and assert that it traps with the assertion `message`.
fn assert_traps(
    package: &Arc<Package>,
    context: &Rc<Context>,
    input: impl ToMidenRepr + Copy + core::fmt::Debug,
    message: &str,
) {
    let result = catch_unwind(AssertUnwindSafe(|| run::<Felt>(package, context, input)));
    match result {
        Err(panic) => {
            let err = panic_message(panic);
            assert!(
                trap_matches(&err, message),
                "expected {input:?} to trap with {message:?}: {err}"
            );
        }
        Ok(output) => panic!("expected {input:?} to trap with {message:?}, got {output:?}"),
    }
}

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

/// An `i128` in the `i16` range casts to its least significant limb; one past either bound traps.
#[test]
fn cast_of_an_i128_to_i16_checks_the_value_is_in_range() {
    let (package, context) = compile(Type::I128, Type::I16, cast);
    for value in [i16::MIN as i128, i16::MAX as i128] {
        assert_eq!(run::<i16>(&package, &context, value), value as i16);
    }
    for value in [i16::MIN as i128 - 1, i16::MAX as i128 + 1] {
        assert_traps(&package, &context, value, "i64 value does not fit in signed 16-bit range");
    }
}

/// An `i128` in the `i8` range casts to its least significant limb; one past either bound traps.
#[test]
fn cast_of_an_i128_to_i8_checks_the_value_is_in_range() {
    let (package, context) = compile(Type::I128, Type::I8, cast);
    for value in [i8::MIN as i128, i8::MAX as i128] {
        assert_eq!(run::<i8>(&package, &context, value), value as i8);
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
