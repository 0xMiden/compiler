use midenc_codegen_masm::OptimizeMasmArithmetic;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    Felt, OpBuilder, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder},
    pass::{Nesting, PassManager},
};

use super::support::run_args;
use crate::{testing::compile_test_module_with_masm, trap_helpers::panic_message};

const EDGES: &[i32] = &[i32::MIN, i32::MIN + 1, -65_537, -1, 0, 1, 65_537, i32::MAX - 1, i32::MAX];

#[test]
fn target_cost_wrapping_multiplication_preserves_sign_bits_and_wrapping_overflow() {
    let (masm, package, context) =
        compile_test_module_with_masm([Type::I32, Type::I32], [Type::I32], wrapping_product);
    assert!(masm.contains("u32wrapping_mul"), "{masm}");
    assert!(!masm.contains("::intrinsics::i32::wrapping_mul"), "{masm}");
    let mut inputs = EDGES
        .iter()
        .flat_map(|lhs| EDGES.iter().map(move |rhs| (*lhs, *rhs)))
        .collect::<Vec<_>>();
    let mut seed = 0x1234_5678u32;
    for _ in 0..256 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let lhs = seed as i32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        inputs.push((lhs, seed as i32));
    }
    for (lhs, rhs) in inputs {
        let args = [Felt::from(lhs as u32), Felt::from(rhs as u32)];
        let actual: i32 = run_args(&package, &context, &args);
        assert_eq!(actual, lhs.wrapping_mul(rhs), "{lhs} * {rhs}");
    }
}

#[test]
fn target_cost_wrapping_multiplication_still_rejects_out_of_range_inputs() {
    let (_, package, context) =
        compile_test_module_with_masm([Type::I32, Type::I32], [Type::I32], wrapping_product);
    let invalid = Felt::new_unchecked(1u64 << 32);
    for args in [[invalid, Felt::ZERO], [Felt::ZERO, invalid], [invalid, invalid]] {
        let failure = catch_unwind(AssertUnwindSafe(|| run_args::<i32>(&package, &context, &args)))
            .expect_err("wrapping multiplication must still validate both input elements");
        let message = panic_message(failure);
        assert!(message.contains("operation expected u32 values"), "{message}");
    }
}

#[test]
fn target_cost_wrapping_multiplication_preserves_shared_operands() {
    let (_, package, context) =
        compile_test_module_with_masm([Type::I32, Type::I32], [Type::I32], |builder| {
            let arguments = builder.current_block().borrow().arguments().to_vec();
            let lhs = arguments[0] as ValueRef;
            let rhs = arguments[1] as ValueRef;
            let product = builder.mul_wrapping(lhs, rhs, SourceSpan::UNKNOWN).unwrap();
            let sum = builder.add_wrapping(product, lhs, SourceSpan::UNKNOWN).unwrap();
            let result = builder.bxor(sum, rhs, SourceSpan::UNKNOWN).unwrap();
            builder.ret(Some(result), SourceSpan::UNKNOWN).unwrap();
            optimize(builder);
        });
    for &lhs in EDGES {
        for &rhs in EDGES {
            let args = [Felt::from(lhs as u32), Felt::from(rhs as u32)];
            let actual: i32 = run_args(&package, &context, &args);
            assert_eq!(actual, lhs.wrapping_mul(rhs).wrapping_add(lhs) ^ rhs, "{lhs}, {rhs}");
        }
    }
}

fn wrapping_product(builder: &mut FunctionBuilder<'_, OpBuilder>) {
    let arguments = builder.current_block().borrow().arguments().to_vec();
    let product = builder
        .mul_wrapping(arguments[0] as ValueRef, arguments[1] as ValueRef, SourceSpan::UNKNOWN)
        .unwrap();
    builder.ret(Some(product), SourceSpan::UNKNOWN).unwrap();
    optimize(builder);
}

#[test]
fn target_cost_immediate_multiplication_preserves_wrapped_results() {
    for rhs in [0i32, 1, 2, 3, -1, i32::MIN, i32::MAX] {
        let (masm, package, context) =
            compile_test_module_with_masm([Type::I32], [Type::I32], |builder| {
                let input = builder.current_block().borrow().arguments()[0] as ValueRef;
                let immediate = builder.i32(rhs, SourceSpan::UNKNOWN);
                let product = builder.mul_wrapping(input, immediate, SourceSpan::UNKNOWN).unwrap();
                builder.ret(Some(product), SourceSpan::UNKNOWN).unwrap();
                optimize(builder);
            });
        assert!(!masm.contains("::intrinsics::i32::wrapping_mul"), "{rhs}: {masm}");
        for &lhs in EDGES {
            let actual: i32 = run_args(&package, &context, &[Felt::from(lhs as u32)]);
            assert_eq!(actual, lhs.wrapping_mul(rhs), "{lhs} * {rhs}");
        }
    }
}

fn optimize(builder: &mut FunctionBuilder<'_, OpBuilder>) {
    let function = builder.current_block().borrow().parent_op().unwrap();
    let context = function.borrow().context_rc();
    let mut passes = PassManager::on::<Function>(context, Nesting::Implicit);
    passes.enable_verifier(true);
    passes.add_pass(Box::new(OptimizeMasmArithmetic::default()));
    passes.run(function).expect("target arithmetic should optimize and verify");
}
use std::panic::{AssertUnwindSafe, catch_unwind};
