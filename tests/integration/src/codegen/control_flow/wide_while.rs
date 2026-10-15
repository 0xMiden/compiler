//! Executed tests for structured control flow ops whose forwarded values exceed the 16-felt
//! operand stack window.

use std::{rc::Rc, sync::Arc};

use miden_debug::ToMidenRepr;
use miden_mast_package::Package;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::transforms::{NarrowRegionOps, TransformSpills};
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    Context, Felt, Op, OpBuilder, SourceSpan, Type, ValueRef,
    dialects::builtin::{self, BuiltinOpBuilder, FunctionBuilder, attributes::Signature},
    pass::{Nesting, PassManager},
};

use crate::{
    codegen::support::run_args,
    testing::{compile_miden_component_to_package, setup},
};

/// The number of values the loop carries from one iteration to the next.
const WIDTH: usize = 9;

/// Compile an entrypoint built by `build`, after the region narrowing and spill placement that
/// the backend runs at the end of every function pipeline.
///
/// `compile_test_module` lowers hand-built HIR as it stands, so the over-wide ops would reach
/// codegen untouched; the rest of the rewrite pipeline is left out so that the canonicalizer does
/// not fold the ops under test.
fn compile_narrowed(
    params: impl IntoIterator<Item = Type>,
    results: impl IntoIterator<Item = Type>,
    build: impl Fn(&mut FunctionBuilder<'_, OpBuilder>),
) -> (Arc<Package>, Rc<Context>) {
    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let signature = Signature::new(&context, params, results);
    let component = setup::build_empty_component_for_test(context.clone());
    let function = setup::build_entrypoint(
        component.component.expect("expected HIR component"),
        &signature,
        build,
    );

    let mut pm = PassManager::on::<builtin::Function>(context.clone(), Nesting::Implicit);
    pm.add_pass(Box::new(NarrowRegionOps));
    pm.add_pass(Box::new(TransformSpills));
    pm.run(function.as_operation_ref())
        .expect("narrowing and spill placement should succeed");

    let package = compile_miden_component_to_package(component).unwrap();
    (package, context)
}

/// Evaluate the `wide_while` loop in plain Rust, returning its result and the number of trips.
fn wide_while_model(a: [u32; WIDTH]) -> (u32, usize) {
    let mut x = a;
    let mut trips = 0;
    loop {
        let y: [u32; WIDTH] = core::array::from_fn(|i| x[i].wrapping_add(x[(i + 1) % WIDTH]));
        if x[0] >= x[8] {
            // The results are the values forwarded by the last condition: `x` then `y`.
            return (x[0].wrapping_add(x[2]).wrapping_add(y[8]), trips);
        }
        x = y;
        trips += 1;
        assert!(trips < 1000, "the model loop does not terminate for {a:?}");
    }
}

/// Executes the emitted MASM for an `scf.while` whose `scf.condition` forwards 18 felts (nine
/// iteration arguments plus nine values computed in the before region), more than the 16-felt
/// window, so the `narrow-region-ops` pass must move the excess to procedure locals (#1459).
///
/// The HIR is built directly so that the test does not depend on constant folding, and only the
/// backend's last function passes run over it (see [`compile_narrowed`]).
#[test]
fn wide_while_forwards_more_than_the_operand_stack_window() {
    let span = SourceSpan::default();

    let (package, context) =
        compile_narrowed([const { Type::U32 }; WIDTH], [Type::U32], |builder| {
            let entry = builder.entry_block();
            let inits = entry
                .borrow()
                .arguments()
                .iter()
                .map(|arg| *arg as ValueRef)
                .collect::<Vec<_>>();

            let while_op = builder.r#while(inits, &[const { Type::U32 }; 2 * WIDTH], span).unwrap();
            let (before_block, after_block) = {
                let while_op = while_op.borrow();
                (
                    while_op.before().entry_block_ref().unwrap(),
                    while_op.after().entry_block_ref().unwrap(),
                )
            };

            // before: forward the iteration arguments and their pairwise sums.
            builder.switch_to_block(before_block);
            let x = before_block
                .borrow()
                .arguments()
                .iter()
                .map(|arg| *arg as ValueRef)
                .collect::<Vec<_>>();
            let cond = builder.lt(x[0], x[8], span).unwrap();
            let y = (0..WIDTH)
                .map(|i| builder.add_wrapping(x[i], x[(i + 1) % WIDTH], span).unwrap())
                .collect::<Vec<_>>();
            builder.condition(cond, x.iter().chain(&y).copied(), span).unwrap();

            // after: the sums become the next iteration arguments.
            builder.switch_to_block(after_block);
            let p = after_block
                .borrow()
                .arguments()
                .iter()
                .map(|arg| *arg as ValueRef)
                .collect::<Vec<_>>();
            builder.r#yield(p[WIDTH..].iter().copied(), span).unwrap();

            builder.switch_to_block(entry);
            let r = while_op
                .borrow()
                .as_operation()
                .results()
                .all()
                .iter()
                .map(|result| result.borrow().as_value_ref())
                .collect::<Vec<_>>();
            let sum = builder.add_wrapping(r[0], r[2], span).unwrap();
            let sum = builder.add_wrapping(sum, r[17], span).unwrap();
            builder.ret(Some(sum), span).unwrap();
        });

    let cases: [([u32; WIDTH], usize); 3] = [
        // Exits at the first condition.
        ([9, 8, 7, 6, 5, 4, 3, 2, 1], 0),
        // Several trips.
        ([1, 2, 3, 4, 5, 6, 7, 8, 100], 2),
        // The sums wrap around.
        ([u32::MAX - 15, 5, u32::MAX - 15, 3, 4, 5, 6, 7, u32::MAX], 1),
    ];
    for (input, min_trips) in cases {
        let (expected, trips) = wide_while_model(input);
        assert!(trips >= min_trips, "expected at least {min_trips} trips for {input:?}");
        let args = input.map(Felt::from);
        let output = run_args::<u32>(&package, &context, &args);
        assert_eq!(output, expected, "unexpected result for {input:?}");
    }
}

/// Executes the emitted MASM for an `scf.if` with nine `u64` results (18 felts), more than the
/// 16-felt window, so the `narrow-region-ops` pass must move the excess to procedure locals
/// (#1459).
///
/// The HIR is built directly so that the test does not depend on constant folding, and only the
/// backend's last function passes run over it (see [`compile_narrowed`]).
#[test]
fn wide_if_yields_more_than_the_operand_stack_window() {
    let span = SourceSpan::default();

    let (package, context) = compile_narrowed([Type::U32, Type::U64], [Type::U64], |builder| {
        let entry = builder.entry_block();
        let (c, v) = {
            let entry = entry.borrow();
            let args = entry.arguments();
            (args[0] as ValueRef, args[1] as ValueRef)
        };
        let zero = builder.u32(0, span);
        let cond = builder.neq(c, zero, span).unwrap();

        let if_op = builder.r#if(cond, &[const { Type::U64 }; WIDTH], span).unwrap();
        let (then_region, else_region) = {
            let if_op = if_op.borrow();
            (if_op.then_body().as_region_ref(), if_op.else_body().as_region_ref())
        };

        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        builder.r#yield([v; WIDTH], span).unwrap();

        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let seven = builder.u64(7, span);
        builder.r#yield([seven; WIDTH], span).unwrap();

        builder.switch_to_block(entry);
        let (r0, r8) = {
            let if_op = if_op.borrow();
            let results = if_op.as_operation().results();
            (results[0].borrow().as_value_ref(), results[8].borrow().as_value_ref())
        };
        let sum = builder.add_wrapping(r0, r8, span).unwrap();
        builder.ret(Some(sum), span).unwrap();
    });

    for (c, v, expected) in [
        (1u32, 21u64, 42u64),
        (1, u64::MAX, u64::MAX - 1),
        (0, 21, 14),
        (0, u64::MAX, 14),
    ] {
        let mut args = Vec::new();
        c.push_to_operand_stack(&mut args);
        v.push_to_operand_stack(&mut args);
        let output = run_args::<u64>(&package, &context, &args);
        assert_eq!(output, expected, "unexpected result for c = {c}, v = {v}");
    }
}
