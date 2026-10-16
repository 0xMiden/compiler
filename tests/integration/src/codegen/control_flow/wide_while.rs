//! Executed tests for structured control flow ops whose forwarded values exceed the 16-felt
//! operand stack window.

use std::{rc::Rc, sync::Arc};

use miden_debug::ToMidenRepr;
use miden_mast_package::Package;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::transforms::{NarrowRegionOps, TransformSpills};
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    BlockRef, Context, Felt, OpBuilder, OperationRef, SourceSpan, Type, ValueRef,
    dialects::builtin::{self, BuiltinOpBuilder, FunctionBuilder},
    pass::{Nesting, PassManager},
};

use crate::{
    codegen::support::run_args,
    testing::{build_test_component, compile_miden_component_to_package},
};

/// The number of values the loop carries from one iteration to the next.
const WIDTH: usize = 9;

/// The number of columns of the ops whose inits or results alone exceed the window.
const WIDE: usize = 17;

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
    let (component, context) = build_test_component(params, results, build);

    let mut pm = PassManager::on::<builtin::World>(context.clone(), Nesting::Implicit);
    {
        let mut component_pm = pm.nest::<builtin::Component>();
        let mut module_pm = component_pm.nest::<builtin::Module>();
        let mut func_pm = module_pm.nest::<builtin::Function>();
        func_pm.add_pass(Box::new(NarrowRegionOps));
        func_pm.add_pass(Box::new(TransformSpills));
    }
    pm.run(component.world.as_operation_ref())
        .expect("narrowing and spill placement should succeed");

    let package = compile_miden_component_to_package(component).unwrap();
    (package, context)
}

/// The arguments of `block` as values.
fn block_args(block: BlockRef) -> Vec<ValueRef> {
    block.borrow().arguments().iter().map(|arg| *arg as ValueRef).collect()
}

/// The results of `op` as values.
fn op_results(op: OperationRef) -> Vec<ValueRef> {
    op.borrow().results().all().iter().map(|r| r.borrow().as_value_ref()).collect()
}

/// Emit `fold(values)`: `acc = acc * 3 + v` over `values` in order, wrapping, starting from the
/// first value. Every value contributes with its own weight, so a value swapped with another or
/// replaced by a stale one changes the result.
fn build_fold(
    builder: &mut FunctionBuilder<'_, OpBuilder>,
    values: &[ValueRef],
    three: ValueRef,
    span: SourceSpan,
) -> ValueRef {
    values[1..].iter().fold(values[0], |acc, v| {
        let acc = builder.mul_wrapping(acc, three, span).unwrap();
        builder.add_wrapping(acc, *v, span).unwrap()
    })
}

/// The model of [`build_fold`] over `u32` values.
fn fold_u32(values: &[u32]) -> u32 {
    values[1..]
        .iter()
        .fold(values[0], |acc, v| acc.wrapping_mul(3).wrapping_add(*v))
}

/// The model of [`build_fold`] over `u64` values.
fn fold_u64(values: &[u64]) -> u64 {
    values[1..]
        .iter()
        .fold(values[0], |acc, v| acc.wrapping_mul(3).wrapping_add(*v))
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
            let inits = block_args(entry);

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
            let x = block_args(before_block);
            let cond = builder.lt(x[0], x[8], span).unwrap();
            let y = (0..WIDTH)
                .map(|i| builder.add_wrapping(x[i], x[(i + 1) % WIDTH], span).unwrap())
                .collect::<Vec<_>>();
            builder.condition(cond, x.iter().chain(&y).copied(), span).unwrap();

            // after: the sums become the next iteration arguments.
            builder.switch_to_block(after_block);
            let p = block_args(after_block);
            builder.r#yield(p[WIDTH..].iter().copied(), span).unwrap();

            builder.switch_to_block(entry);
            let r = op_results(while_op.as_operation_ref());
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

/// The inits of the `wide_while_inits` loop: the nine arguments, then eight distinct constants.
fn wide_inits(a: [u32; WIDTH]) -> [u32; WIDE] {
    core::array::from_fn(|i| {
        if i < WIDTH {
            a[i]
        } else {
            wide_init_constant(i)
        }
    })
}

/// The constant init of column `i` of the `wide_while_inits` loop, for `i >= WIDTH`.
fn wide_init_constant(i: usize) -> u32 {
    1000 + 37 * i as u32
}

/// Evaluate the `wide_while_inits` loop in plain Rust, returning its result and the number of
/// trips.
fn wide_while_inits_model(a: [u32; WIDTH]) -> (u32, usize) {
    let mut x = wide_inits(a);
    let mut trips = 0;
    loop {
        if x[0].wrapping_add(x[1]) >= x[16] {
            return (fold_u32(&x), trips);
        }
        x = core::array::from_fn(|i| x[i].wrapping_add(x[(i + 1) % WIDE]));
        trips += 1;
        assert!(trips < 1000, "the model loop does not terminate for {a:?}");
    }
}

/// Executes the emitted MASM for an `scf.while` with 17 `u32` inits, more than the 16-felt
/// window, whose `scf.condition` forwards all 17 iteration arguments, so the `narrow-region-ops`
/// pass must demote columns of both the iteration group and the result group (#1459).
///
/// Every column carries a distinct value, the after region updates each one differently, and the
/// result folds all 17 results with distinct weights, so a demoted column stored to the wrong
/// local, not stored before the yield, or not loaded changes the result.
#[test]
fn wide_while_inits_exceed_the_operand_stack_window() {
    let span = SourceSpan::default();

    let (package, context) =
        compile_narrowed([const { Type::U32 }; WIDTH], [Type::U32], |builder| {
            let entry = builder.entry_block();
            let mut inits = block_args(entry);
            inits.extend((WIDTH..WIDE).map(|i| builder.u32(wide_init_constant(i), span)));

            let while_op = builder.r#while(inits, &[const { Type::U32 }; WIDE], span).unwrap();
            let (before_block, after_block) = {
                let while_op = while_op.borrow();
                (
                    while_op.before().entry_block_ref().unwrap(),
                    while_op.after().entry_block_ref().unwrap(),
                )
            };

            // before: continue while `x0 + x1 < x16`, forwarding every iteration argument.
            builder.switch_to_block(before_block);
            let x = block_args(before_block);
            let head = builder.add_wrapping(x[0], x[1], span).unwrap();
            let cond = builder.lt(head, x[16], span).unwrap();
            builder.condition(cond, x.iter().copied(), span).unwrap();

            // after: each column becomes the sum of itself and its right neighbour.
            builder.switch_to_block(after_block);
            let p = block_args(after_block);
            let y = (0..WIDE)
                .map(|i| builder.add_wrapping(p[i], p[(i + 1) % WIDE], span).unwrap())
                .collect::<Vec<_>>();
            builder.r#yield(y, span).unwrap();

            builder.switch_to_block(entry);
            let r = op_results(while_op.as_operation_ref());
            let three = builder.u32(3, span);
            let folded = build_fold(builder, &r, three, span);
            builder.ret(Some(folded), span).unwrap();
        });

    let cases: [([u32; WIDTH], usize); 3] = [
        // Exits at the first condition: `x0 + x1 >= x16`.
        ([5000, 7, 3, 4, 5, 6, 7, 8, 9], 0),
        // Several trips.
        ([1, 2, 3, 4, 5, 6, 7, 8, 9], 2),
        // The sums wrap around.
        ([1, 2, u32::MAX - 15, u32::MAX, 4, 5, 6, 7, 8], 1),
    ];
    for (input, min_trips) in cases {
        let (expected, trips) = wide_while_inits_model(input);
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
/// Column `i` yields `v + i` in the then arm and `7 + 2 * i` in the else arm, and the result
/// folds all nine results with distinct weights, so a demoted column stored to the wrong local
/// in either arm, or not loaded, changes the result.
///
/// The HIR is built directly so that the test does not depend on constant folding, and only the
/// backend's last function passes run over it (see [`compile_narrowed`]).
#[test]
fn wide_if_yields_more_than_the_operand_stack_window() {
    let span = SourceSpan::default();

    let (package, context) = compile_narrowed([Type::U32, Type::U64], [Type::U64], |builder| {
        let entry = builder.entry_block();
        let (c, v) = {
            let args = block_args(entry);
            (args[0], args[1])
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
        let then_values = (0..WIDTH as u64)
            .map(|i| {
                let i = builder.u64(i, span);
                builder.add_wrapping(v, i, span).unwrap()
            })
            .collect::<Vec<_>>();
        builder.r#yield(then_values, span).unwrap();

        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let else_values =
            (0..WIDTH as u64).map(|i| builder.u64(7 + 2 * i, span)).collect::<Vec<_>>();
        builder.r#yield(else_values, span).unwrap();

        builder.switch_to_block(entry);
        let r = op_results(if_op.as_operation_ref());
        let three = builder.u64(3, span);
        let folded = build_fold(builder, &r, three, span);
        builder.ret(Some(folded), span).unwrap();
    });

    let model = |c: u32, v: u64| {
        let columns: [u64; WIDTH] = core::array::from_fn(|i| {
            let i = i as u64;
            if c != 0 { v.wrapping_add(i) } else { 7 + 2 * i }
        });
        fold_u64(&columns)
    };
    for (c, v) in [(1u32, 21u64), (1, u64::MAX - 3), (0, 21), (0, u64::MAX)] {
        let mut args = Vec::new();
        c.push_to_operand_stack(&mut args);
        v.push_to_operand_stack(&mut args);
        let output = run_args::<u64>(&package, &context, &args);
        assert_eq!(output, model(c, v), "unexpected result for c = {c}, v = {v}");
    }
}

/// Executes the emitted MASM for an `scf.index_switch` with 17 `u32` results, more than the
/// 16-felt window, so the `narrow-region-ops` pass must move the excess to procedure locals
/// (#1459).
///
/// Column `i` yields `v + i` in case 0, `v ^ (i * 0x101)` in case 1 and `1000 + 3 * i` in the
/// default arm, and the result folds all 17 results with distinct weights, so a demoted column
/// stored to the wrong local in any arm, or not loaded, changes the result.
#[test]
fn wide_index_switch_yields_more_than_the_operand_stack_window() {
    let span = SourceSpan::default();

    let (package, context) = compile_narrowed([Type::U32, Type::U32], [Type::U32], |builder| {
        let entry = builder.entry_block();
        let (selector, v) = {
            let args = block_args(entry);
            (args[0], args[1])
        };

        let switch = builder
            .index_switch(selector, [0, 1], &[const { Type::U32 }; WIDE], span)
            .unwrap();

        let case0 = builder.create_block_in_region(switch.borrow().get_case_region(0));
        builder.switch_to_block(case0);
        let values = (0..WIDE as u32)
            .map(|i| {
                let i = builder.u32(i, span);
                builder.add_wrapping(v, i, span).unwrap()
            })
            .collect::<Vec<_>>();
        builder.r#yield(values, span).unwrap();

        let case1 = builder.create_block_in_region(switch.borrow().get_case_region(1));
        builder.switch_to_block(case1);
        let values = (0..WIDE as u32)
            .map(|i| {
                let mask = builder.u32(i * 0x101, span);
                builder.bxor(v, mask, span).unwrap()
            })
            .collect::<Vec<_>>();
        builder.r#yield(values, span).unwrap();

        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);
        let values = (0..WIDE as u32).map(|i| builder.u32(1000 + 3 * i, span)).collect::<Vec<_>>();
        builder.r#yield(values, span).unwrap();

        builder.switch_to_block(entry);
        let r = op_results(switch.as_operation_ref());
        let three = builder.u32(3, span);
        let folded = build_fold(builder, &r, three, span);
        builder.ret(Some(folded), span).unwrap();
    });

    let model = |selector: u32, v: u32| {
        let columns: [u32; WIDE] = core::array::from_fn(|i| {
            let i = i as u32;
            match selector {
                0 => v.wrapping_add(i),
                1 => v ^ (i * 0x101),
                _ => 1000 + 3 * i,
            }
        });
        fold_u32(&columns)
    };
    for (selector, v) in
        [(0u32, 41u32), (0, u32::MAX - 5), (1, 0xdead_beef), (2, 41), (u32::MAX, 7)]
    {
        let output = run_args::<u32>(&package, &context, &[Felt::from(selector), Felt::from(v)]);
        assert_eq!(output, model(selector, v), "unexpected result for selector {selector}, v {v}");
    }
}
