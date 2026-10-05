//! Helpers for the executed tests of single operations: compile an entrypoint that applies one
//! operation to its argument through the real pipeline, run it on the VM, and check that it traps.

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

/// An operation on a value, whose result has the given type.
pub(super) type UnaryOp = fn(&mut FunctionBuilder<'_, OpBuilder>, ValueRef, Type) -> ValueRef;

pub(super) fn trunc(
    builder: &mut FunctionBuilder<'_, OpBuilder>,
    value: ValueRef,
    ty: Type,
) -> ValueRef {
    builder.trunc(value, ty, SourceSpan::default()).unwrap()
}

pub(super) fn cast(
    builder: &mut FunctionBuilder<'_, OpBuilder>,
    value: ValueRef,
    ty: Type,
) -> ValueRef {
    builder.cast(value, ty, SourceSpan::default()).unwrap()
}

/// Compile an entrypoint that applies `op` to its argument, of type `src`, giving a `dst`.
pub(super) fn compile(src: Type, dst: Type, op: UnaryOp) -> (Arc<Package>, Rc<Context>) {
    compile_test_module([src], [dst.clone()], move |builder| {
        let input = builder.current_block().borrow().arguments()[0] as ValueRef;
        let output = op(builder, input, dst.clone());
        builder.ret(Some(output), SourceSpan::default()).unwrap();
    })
}

/// Run `package` on `input`, its least significant limb on top of the operand stack.
pub(super) fn run<T>(package: &Arc<Package>, context: &Rc<Context>, input: impl ToMidenRepr) -> T
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
{
    let mut args = Vec::new();
    input.push_to_operand_stack(&mut args);
    run_args(package, context, &args)
}

/// Run `package` on the operand stack `args`, the first on top.
pub(super) fn run_args<T>(package: &Arc<Package>, context: &Rc<Context>, args: &[Felt]) -> T
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
{
    eval_package::<T, _, _>(package.clone(), None, args, context.session(), |_| Ok(())).unwrap()
}

/// Run `package` on `input`, and assert that it traps with the assertion `message`.
pub(super) fn assert_traps(
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
