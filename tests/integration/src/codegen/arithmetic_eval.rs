use std::rc::Rc;

use midenc_codegen_masm::OptimizeMasmArithmetic;
use midenc_hir::{
    Context, Forward, OperationRef, Overflow, RawWalk, Report, SymbolName, SymbolTable, Type,
    diagnostics::Uri,
    dialects::builtin::{Module, ModuleRef},
    parse::{ParserConfig, parse},
    pass::{Nesting, PassManager},
};
use midenc_hir_eval::{HirEvaluator, Value};

use super::arithmetic::EDGES;
use crate::testing::setup::dummy_context;

const ARITHMETIC: &str = include_str!("../../../lit/hir-opt/masm-arithmetic.hir");
const DATAFLOW: &str = include_str!("../../../lit/hir-opt/masm-arithmetic-dataflow.hir");
const CONTROL_FLOW: &str = include_str!("../../../lit/hir-opt/masm-arithmetic-control-flow.hir");
const BOUNDARIES: &str = include_str!("../../../lit/hir-opt/masm-arithmetic-boundaries.hir");

struct EvalTest {
    context: Rc<Context>,
    module: ModuleRef,
}

impl EvalTest {
    fn new(source: &str) -> Self {
        let context = dummy_context(&[]);
        let module = parse::<Module>(
            ParserConfig::new(context.clone()),
            Uri::new("target-cost-eval.hir"),
            source,
        )
        .expect("arithmetic fixture should parse");
        Self { context, module }
    }

    fn eval(&self, name: &str, arguments: &[Value]) -> Result<Vec<Value>, Report> {
        let function = self.module.borrow().get(SymbolName::intern(name)).unwrap();
        let mut evaluator = HirEvaluator::new(self.context.clone());
        evaluator
            .eval(function.borrow().as_symbol_operation(), arguments.iter().copied())
            .map(|results| results.into_vec())
    }

    fn optimize(&self) {
        let mut passes = PassManager::on::<Module>(self.context.clone(), Nesting::Implicit);
        passes.enable_verifier(true);
        passes.add_pass(Box::new(OptimizeMasmArithmetic::default()));
        passes
            .run(self.module.as_operation_ref())
            .expect("arithmetic should optimize and verify");
    }

    fn wrapping_i32_products(&self) -> usize {
        let mut count = 0;
        self.module
            .as_operation_ref()
            .raw_prewalk_all::<Forward, _>(|operation: OperationRef| {
                let operation = operation.borrow();
                if let Some(product) = operation.downcast_ref::<midenc_dialect_arith::Mul>()
                    && *product.get_overflow() == Overflow::Wrapping
                    && product.lhs().ty() == Type::I32
                {
                    count += 1;
                }
            });
        count
    }
}

fn assert_equivalent(
    source: &str,
    name: &str,
    cases: impl IntoIterator<Item = (Vec<Value>, Vec<Value>)>,
) {
    let test = EvalTest::new(source);
    let cases = cases.into_iter().collect::<Vec<_>>();
    assert!(!cases.is_empty());
    let before = cases
        .iter()
        .map(|(arguments, expected)| {
            let actual = test.eval(name, arguments).expect("original HIR should evaluate");
            assert_eq!(&actual, expected, "original {name}({arguments:?})");
            actual
        })
        .collect::<Vec<_>>();
    assert!(test.wrapping_i32_products() > 0, "fixture must exercise the rewrite");
    test.optimize();
    assert_eq!(test.wrapping_i32_products(), 0, "wrapping i32 products must be rewritten");
    for ((arguments, expected), baseline) in cases.into_iter().zip(before) {
        let actual = test.eval(name, &arguments).expect("optimized HIR should evaluate");
        assert_eq!(actual, baseline, "before/after {name}({arguments:?})");
        assert_eq!(actual, expected, "optimized {name}({arguments:?})");
    }
}

fn pairs() -> impl Iterator<Item = (i32, i32)> {
    EDGES.iter().flat_map(|&lhs| EDGES.iter().map(move |&rhs| (lhs, rhs)))
}

#[test]
fn target_cost_eval_wrapping_products_preserve_edge_and_random_results() {
    let mut inputs = pairs().collect::<Vec<_>>();
    let mut seed = 0x1234_5678u32;
    for _ in 0..256 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let lhs = seed as i32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        inputs.push((lhs, seed as i32));
    }
    assert_equivalent(
        ARITHMETIC,
        "wrapping_product",
        inputs
            .into_iter()
            .map(|(lhs, rhs)| (vec![lhs.into(), rhs.into()], vec![lhs.wrapping_mul(rhs).into()])),
    );
}

#[test]
fn target_cost_eval_preserves_repeated_and_shared_operands() {
    assert_equivalent(
        DATAFLOW,
        "square",
        EDGES
            .iter()
            .map(|&input| (vec![input.into()], vec![input.wrapping_mul(input).into()])),
    );
    assert_equivalent(
        DATAFLOW,
        "shared_operands",
        pairs().map(|(lhs, rhs)| {
            (
                vec![lhs.into(), rhs.into()],
                vec![(lhs.wrapping_mul(rhs).wrapping_add(lhs) ^ rhs).into()],
            )
        }),
    );
}

#[test]
fn target_cost_eval_preserves_chained_products_and_debug_uses() {
    assert_equivalent(
        DATAFLOW,
        "chained_product",
        pairs().flat_map(|(lhs, rhs)| {
            [-1i32, 0, 3, i32::MIN].into_iter().map(move |factor| {
                (
                    vec![lhs.into(), rhs.into(), factor.into()],
                    vec![lhs.wrapping_mul(rhs).wrapping_mul(factor).into()],
                )
            })
        }),
    );
}

#[test]
fn target_cost_eval_preserves_signed_comparisons_and_extension() {
    assert_equivalent(
        DATAFLOW,
        "signed_uses",
        pairs().map(|(lhs, rhs)| {
            let product = lhs.wrapping_mul(rhs);
            (vec![lhs.into(), rhs.into()], vec![(product < 0).into()])
        }),
    );
    assert_equivalent(
        DATAFLOW,
        "wide_product",
        pairs().map(|(lhs, rhs)| {
            (vec![lhs.into(), rhs.into()], vec![i64::from(lhs.wrapping_mul(rhs)).into()])
        }),
    );
    assert_equivalent(
        DATAFLOW,
        "negative_constant",
        EDGES
            .iter()
            .map(|&input| (vec![input.into()], vec![input.wrapping_mul(-1).into()])),
    );
    assert_equivalent(
        DATAFLOW,
        "min_constant",
        EDGES
            .iter()
            .map(|&input| (vec![input.into()], vec![input.wrapping_mul(i32::MIN).into()])),
    );
}

#[test]
fn target_cost_eval_preserves_cfg_join_arguments() {
    assert_equivalent(
        CONTROL_FLOW,
        "cfg_product",
        pairs().flat_map(|(lhs, rhs)| {
            [false, true].into_iter().map(move |condition| {
                let result = if condition {
                    lhs.wrapping_mul(rhs)
                } else {
                    lhs.wrapping_mul(lhs)
                };
                (vec![condition.into(), lhs.into(), rhs.into()], vec![result.into()])
            })
        }),
    );
}

#[test]
fn target_cost_eval_preserves_nested_region_branches() {
    assert_equivalent(
        CONTROL_FLOW,
        "nested_product",
        pairs().flat_map(|(lhs, rhs)| {
            [(false, false), (false, true), (true, false), (true, true)].into_iter().map(
                move |(outer, inner)| {
                    let result = match (outer, inner) {
                        (false, _) => lhs,
                        (true, false) => rhs.wrapping_mul(rhs),
                        (true, true) => lhs.wrapping_mul(rhs),
                    };
                    (vec![outer.into(), inner.into(), lhs.into(), rhs.into()], vec![result.into()])
                },
            )
        }),
    );
}

#[test]
fn target_cost_eval_preserves_zero_one_and_repeated_loop_iterations() {
    assert_equivalent(
        CONTROL_FLOW,
        "loop_product",
        pairs().flat_map(|(initial, factor)| {
            [0u32, 1, 2, 5].into_iter().map(move |count| {
                let result = (0..count).fold(initial, |value, _| value.wrapping_mul(factor));
                (vec![initial.into(), factor.into(), count.into()], vec![result.into()])
            })
        }),
    );
}

#[test]
fn target_cost_eval_keeps_checked_overflow_and_overflow_flags() {
    let test = EvalTest::new(BOUNDARIES);
    assert_eq!(test.wrapping_i32_products(), 1);
    for optimized in [false, true] {
        if optimized {
            test.optimize();
            assert_eq!(test.wrapping_i32_products(), 0);
        }
        for (lhs, rhs) in pairs() {
            let arguments = [lhs.into(), rhs.into()];
            let actual = test.eval("checked", &arguments);
            match lhs.checked_mul(rhs) {
                Some(expected) => assert_eq!(actual.unwrap(), vec![Value::from(expected)]),
                None => {
                    let error = actual.expect_err("checked overflow must remain an error");
                    assert!(
                        error
                            .labels()
                            .unwrap()
                            .any(|label| label.label() == Some("arithmetic overflow")),
                        "{error:?}"
                    );
                }
            }
            let (product, overflowed) = lhs.overflowing_mul(rhs);
            assert_eq!(
                test.eval("overflow_flag", &arguments).unwrap(),
                vec![Value::from(overflowed)],
                "overflow flag for {lhs} * {rhs}, optimized={optimized}"
            );
            assert_eq!(
                test.eval("overflow_product", &arguments).unwrap(),
                vec![Value::from(product)],
                "overflow product for {lhs} * {rhs}, optimized={optimized}"
            );
        }
    }
}

#[test]
fn target_cost_eval_keeps_unchecked_multiplication_on_defined_inputs() {
    assert_equivalent(
        ARITHMETIC,
        "unchecked",
        [(0i32, i32::MIN), (i32::MIN, 1), (-3, 7), (-1, -1), (i32::MAX, 1)]
            .into_iter()
            .map(|(lhs, rhs)| (vec![lhs.into(), rhs.into()], vec![(lhs * rhs).into()])),
    );
}

#[test]
fn target_cost_eval_leaves_other_arithmetic_unchanged() {
    assert_equivalent(
        BOUNDARIES,
        "add_zero",
        EDGES.iter().map(|&input| (vec![input.into()], vec![input.into()])),
    );
    assert_equivalent(
        ARITHMETIC,
        "doubling",
        EDGES
            .iter()
            .map(|&input| (vec![input.into()], vec![input.wrapping_add(input).into()])),
    );
}
