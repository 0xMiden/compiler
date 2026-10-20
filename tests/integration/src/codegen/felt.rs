//! Canonical field overflow semantics through the production compiler pipeline.
use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{Felt, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder};

use super::{int128::OverflowResult, support::run_args};
use crate::testing::compile_test_module;

#[test]
fn overflowing_felt_arithmetic_reports_canonical_reduction_and_underflow() {
    let p = 0xffff_ffff_0000_0001u128;
    for op in ["add", "sub", "mul"] {
        let (package, context) =
            compile_test_module([Type::Felt, Type::Felt], [Type::I1, Type::Felt], |builder| {
                let args = builder.current_block().borrow().arguments().to_vec();
                let (a, b) = (args[0] as ValueRef, args[1] as ValueRef);
                let span = SourceSpan::default();
                let (flag, result) = match op {
                    "add" => builder.add_overflowing(a, b, span).unwrap(),
                    "sub" => builder.sub_overflowing(a, b, span).unwrap(),
                    "mul" => builder.mul_overflowing(a, b, span).unwrap(),
                    _ => unreachable!(),
                };
                builder.ret([flag, result], span).unwrap();
            });
        for (a, b) in [
            (0, 1),
            (1, 0),
            (p - 1, 1),
            (p - 1, p - 1),
            ((1 << 32) - 1, (1 << 32) + 1),
            ((1 << 32) - 1, 1 << 32),
        ] {
            let (result, flag) = match op {
                "add" => ((a + b) % p, a + b >= p),
                "sub" => ((a + p - b) % p, a < b),
                "mul" => ((a * b) % p, a * b >= p),
                _ => unreachable!(),
            };
            let args = [Felt::new_unchecked(a as u64), Felt::new_unchecked(b as u64)];
            assert_eq!(
                run_args::<OverflowResult<Felt>>(&package, &context, &args),
                OverflowResult(flag, Felt::new_unchecked(result as u64)),
                "{a} {op} {b}"
            );
        }
    }
}
