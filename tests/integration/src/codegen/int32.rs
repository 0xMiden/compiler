use std::{
    panic::{self, AssertUnwindSafe},
    rc::Rc,
    sync::Arc,
};

use miden_mast_package::Package;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{Context, Felt, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder};

use crate::testing::{compile_test_module, eval_package};

const HIGH_BIT_VALUE: u32 = 1 << 31;

fn compile_guarded_int32_cast(source_ty: Type, target_ty: Type) -> (Arc<Package>, Rc<Context>) {
    let span = SourceSpan::default();
    let cast_target_ty = target_ty.clone();

    compile_test_module(
        [source_ty.clone(), source_ty.clone(), source_ty],
        [target_ty],
        move |builder| {
            let block = builder.current_block();
            let expected_guard = block.borrow().arguments()[0] as ValueRef;
            let live_guard = block.borrow().arguments()[1] as ValueRef;
            let value = block.borrow().arguments()[2] as ValueRef;

            let narrowed = builder.cast(value, cast_target_ty.clone(), span).unwrap();

            // Keep guard values live on the operand stack below the cast operand. If codegen
            // consumes the wrong stack slot, the guard assertion or range check fails.
            builder.assert_eq(live_guard, expected_guard, span).unwrap();
            builder.ret(Some(narrowed), span).unwrap();
        },
    )
}

fn compile_guarded_u8_overflowing_add() -> (Arc<Package>, Rc<Context>) {
    let span = SourceSpan::default();

    compile_test_module([Type::U32, Type::U32, Type::U8, Type::U8], [Type::I1], |builder| {
        let block = builder.current_block();
        let expected_guard = block.borrow().arguments()[0] as ValueRef;
        let live_guard = block.borrow().arguments()[1] as ValueRef;
        let lhs = block.borrow().arguments()[2] as ValueRef;
        let rhs = block.borrow().arguments()[3] as ValueRef;

        let (overflowed, _sum) = builder.add_overflowing(lhs, rhs, span).unwrap();
        // Keep guard values live on the operand stack below the sum. If codegen consumes the
        // wrong stack slot, the guard assertion or overflow flag check fails.
        builder.assert_eq(live_guard, expected_guard, span).unwrap();
        builder.ret(Some(overflowed), span).unwrap();
    })
}

fn try_eval_guarded_cast(
    package: &Package,
    context: &Context,
    args: [u32; 3],
) -> Result<u32, String> {
    let args = args.map(|arg| Felt::new_unchecked(u64::from(arg)));
    panic::catch_unwind(AssertUnwindSafe(|| {
        eval_package::<u32, _, _>(package, None, &args, context.session(), |_| Ok(()))
    }))
    .map_err(panic_payload_to_string)?
    .map_err(|err| format!("{err:?}"))
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else {
        "unknown panic".to_string()
    }
}

fn eval_guarded_u8_overflowing_add(package: &Package, context: &Context, args: [u32; 4]) -> u32 {
    let args = args.map(|arg| Felt::new_unchecked(u64::from(arg)));
    eval_package::<u32, _, _>(package, None, &args, context.session(), |_| Ok(())).unwrap()
}

#[track_caller]
fn assert_cast_succeeds(
    package: &Package,
    context: &Context,
    source_name: &str,
    target_name: &str,
    args: [u32; 3],
    expected: u32,
) {
    let actual = try_eval_guarded_cast(package, context, args).unwrap_or_else(|err| {
        panic!(
            "expected checked {source_name}-to-{target_name} cast of {} to succeed, got: {err}",
            args[2],
        )
    });

    assert_eq!(
        actual, expected,
        "checked {source_name}-to-{target_name} cast returned the wrong value"
    );
}

#[track_caller]
fn assert_cast_traps(
    package: &Package,
    context: &Context,
    source_name: &str,
    target_name: &str,
    args: [u32; 3],
) {
    match try_eval_guarded_cast(package, context, args) {
        Ok(actual) => panic!(
            "expected checked {source_name}-to-{target_name} cast of {} to trap, but returned \
             {actual}",
            args[2]
        ),
        Err(err) => assert!(
            err.contains("does not fit in unsigned"),
            "expected checked {source_name}-to-{target_name} cast of {} to fail the unsigned \
             range check, got: {err}",
            args[2]
        ),
    }
}

#[track_caller]
fn assert_overflowing_add_flag(
    package: &Package,
    context: &Context,
    args: [u32; 4],
    expected_overflowed: bool,
) {
    let actual = eval_guarded_u8_overflowing_add(package, context, args);

    assert_eq!(
        actual,
        u32::from(expected_overflowed),
        "overflow flag for guarded u8 overflowing add was incorrect"
    );
}

#[track_caller]
fn assert_guarded_int32_cast(
    source_ty: Type,
    source_name: &str,
    target_ty: Type,
    target_name: &str,
    max: u32,
    first_invalid: u32,
) {
    // Keep the high-bit guard representable as an i32 value while still setting a bit
    // outside every narrower unsigned target range covered by this test.
    let (package, context) = compile_guarded_int32_cast(source_ty, target_ty);

    assert_cast_succeeds(
        &package,
        &context,
        source_name,
        target_name,
        [HIGH_BIT_VALUE, HIGH_BIT_VALUE, 0],
        0,
    );
    assert_cast_succeeds(
        &package,
        &context,
        source_name,
        target_name,
        [HIGH_BIT_VALUE, HIGH_BIT_VALUE, max],
        max,
    );
    assert_cast_traps(&package, &context, source_name, target_name, [0, 0, first_invalid]);
    assert_cast_traps(&package, &context, source_name, target_name, [0, 0, HIGH_BIT_VALUE]);
}

#[test]
fn checked_int32_to_unsigned_narrowing_checks_the_cast_operand() {
    for (source_ty, source_name) in [(Type::U32, "u32"), (Type::I32, "i32")] {
        for (target_ty, target_name, max, first_invalid) in [
            (Type::I1, "i1", 1u32, 2u32),
            (Type::U8, "u8", u32::from(u8::MAX), u32::from(u8::MAX) + 1),
            (Type::U16, "u16", u32::from(u16::MAX), u32::from(u16::MAX) + 1),
        ] {
            assert_guarded_int32_cast(
                source_ty.clone(),
                source_name,
                target_ty,
                target_name,
                max,
                first_invalid,
            );
        }
    }
}

#[test]
fn overflowing_u8_add_checks_the_sum_being_narrowed() {
    let (package, context) = compile_guarded_u8_overflowing_add();

    assert_overflowing_add_flag(
        &package,
        &context,
        [HIGH_BIT_VALUE, HIGH_BIT_VALUE, u32::from(u8::MAX) - 1, 1],
        false,
    );
    assert_overflowing_add_flag(&package, &context, [0, 0, u32::from(u8::MAX), 1], true);
}
