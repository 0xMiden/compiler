use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    Felt, Op, OpBuilder, Report, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder},
};

use super::{compile_test_module, eval_package};

/// Shared builder type used by the switch execution fixtures in this module.
type TestFunctionBuilder<'a> = FunctionBuilder<'a, OpBuilder>;

/// An explicit `scf.index_switch` branch used by the execution tests below.
#[derive(Clone, Copy, Debug)]
struct SwitchCase {
    selector: u32,
    result: u32,
}

impl SwitchCase {
    /// Create a named switch case instead of relying on positional tuple fields.
    const fn new(selector: u32, result: u32) -> Self {
        Self { selector, result }
    }
}

/// An expected output for one selector value in a switch execution test.
#[derive(Clone, Copy, Debug)]
struct SwitchExpectation {
    selector: u32,
    result: u32,
}

impl SwitchExpectation {
    /// Create a named selector/result expectation for switch execution tests.
    const fn new(selector: u32, result: u32) -> Self {
        Self { selector, result }
    }
}

/// Compile and execute an `scf.index_switch` configured by the supplied region builders.
fn run_index_switch_execution_test<BuildCase, BuildDefault, BuildResult>(
    case_selectors: &[u32],
    expectations: &[SwitchExpectation],
    build_case: BuildCase,
    build_default: BuildDefault,
    build_result: BuildResult,
) where
    BuildCase: Fn(
        &mut TestFunctionBuilder<'_>,
        ValueRef,
        usize,
        u32,
        SourceSpan,
    ) -> Result<ValueRef, Report>,
    BuildDefault:
        Fn(&mut TestFunctionBuilder<'_>, ValueRef, SourceSpan) -> Result<ValueRef, Report>,
    BuildResult: Fn(
        &mut TestFunctionBuilder<'_>,
        ValueRef,
        ValueRef,
        SourceSpan,
    ) -> Result<ValueRef, Report>,
{
    let span = SourceSpan::default();

    let (package, context) = compile_test_module([Type::U32], [Type::U32], |builder| {
        let entry = builder.entry_block();
        let selector = entry.borrow().arguments()[0] as ValueRef;

        let switch = builder
            .index_switch(selector, case_selectors.iter().copied(), &[Type::U32], span)
            .unwrap();

        for (index, case_selector) in case_selectors.iter().copied().enumerate() {
            let case_region = switch.borrow().get_case_region(index);
            let case_block = builder.create_block_in_region(case_region);
            builder.switch_to_block(case_block);

            let result = build_case(builder, selector, index, case_selector, span).unwrap();
            builder.r#yield([result], span).unwrap();
        }

        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);

        let result = build_default(builder, selector, span).unwrap();
        builder.r#yield([result], span).unwrap();

        builder.switch_to_block(entry);
        let switch_result = switch.borrow().results()[0] as ValueRef;
        let output = build_result(builder, selector, switch_result, span).unwrap();
        builder.ret(Some(output), span).unwrap();
    });

    // Compile once, then execute the generated program with the selector values we care about.
    for expectation in expectations {
        let selector = expectation.selector;
        let output = eval_package::<u32, _, _>(
            &package,
            None,
            &[Felt::from(selector)],
            context.session(),
            |_| Ok(()),
        )
        .unwrap();

        assert_eq!(
            output, expectation.result,
            "unexpected result for selector {selector} with case selectors {case_selectors:?}"
        );
    }
}

/// Build the mixed liveness-sensitive case body used by the selector liveness tests.
fn build_selector_sensitive_case(
    builder: &mut TestFunctionBuilder<'_>,
    selector: ValueRef,
    _case_index: usize,
    case_selector: u32,
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    if case_selector.is_multiple_of(2) {
        Ok(builder.u32(case_selector * 11, span))
    } else {
        Ok(selector)
    }
}

/// Return the switch result directly from the enclosing test function.
fn return_switch_result(
    _builder: &mut TestFunctionBuilder<'_>,
    _selector: ValueRef,
    switch_result: ValueRef,
    _span: SourceSpan,
) -> Result<ValueRef, Report> {
    Ok(switch_result)
}

/// Add `selector` to the switch result to keep it live after the switch.
fn add_selector_to_switch_result(
    builder: &mut TestFunctionBuilder<'_>,
    selector: ValueRef,
    switch_result: ValueRef,
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    builder.add(switch_result, selector, span)
}

/// XOR `selector` with the switch result to keep it live without overflow-sensitive arithmetic.
fn xor_selector_with_switch_result(
    builder: &mut TestFunctionBuilder<'_>,
    selector: ValueRef,
    switch_result: ValueRef,
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    builder.bxor(switch_result, selector, span)
}

/// Compile and execute an `scf.index_switch`, checking the result for each `selector`.
fn run_index_switch_test(
    cases: &[SwitchCase],
    default_result: u32,
    expectations: &[SwitchExpectation],
) {
    let case_selectors = cases.iter().map(|case| case.selector).collect::<Vec<_>>();
    run_index_switch_execution_test(
        &case_selectors,
        expectations,
        |builder, _selector, case_index, _case_selector, span| {
            Ok(builder.u32(cases[case_index].result, span))
        },
        |builder, _selector, span| Ok(builder.u32(default_result, span)),
        return_switch_result,
    );
}

/// Compile and execute a switch where some arms consume the selector and others do not.
fn run_index_switch_selector_liveness_test(cases: &[u32], expectations: &[SwitchExpectation]) {
    run_index_switch_execution_test(
        cases,
        expectations,
        build_selector_sensitive_case,
        |builder, _selector, span| Ok(builder.u32(99, span)),
        return_switch_result,
    );
}

/// Compile and execute a switch that keeps `selector` live after the switch result is produced.
fn run_index_switch_selector_live_after_switch_test(
    cases: &[u32],
    expectations: &[SwitchExpectation],
) {
    run_index_switch_execution_test(
        cases,
        expectations,
        build_selector_sensitive_case,
        |builder, _selector, span| Ok(builder.u32(99, span)),
        add_selector_to_switch_result,
    );
}

/// Compile and execute a constant-result switch while keeping `selector` live after the switch.
fn run_index_switch_constant_results_with_selector_live_after_switch_test(
    cases: &[SwitchCase],
    default_result: u32,
    expectations: &[SwitchExpectation],
) {
    let case_selectors = cases.iter().map(|case| case.selector).collect::<Vec<_>>();
    run_index_switch_execution_test(
        &case_selectors,
        expectations,
        |builder, _selector, case_index, _case_selector, span| {
            Ok(builder.u32(cases[case_index].result, span))
        },
        |builder, _selector, span| Ok(builder.u32(default_result, span)),
        add_selector_to_switch_result,
    );
}

#[test]
fn index_switch_contiguous_cases() {
    run_index_switch_test(
        &[SwitchCase::new(0, 11), SwitchCase::new(1, 22), SwitchCase::new(2, 33)],
        99,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 33),
            SwitchExpectation::new(3, 99),
        ],
    );
}

#[test]
fn index_switch_single_nonzero_case() {
    run_index_switch_test(
        &[SwitchCase::new(1, 22)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 11),
        ],
    );
}

#[test]
fn index_switch_two_nonzero_cases() {
    run_index_switch_test(
        &[SwitchCase::new(1, 22), SwitchCase::new(2, 33)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 33),
            SwitchExpectation::new(3, 11),
        ],
    );
}

#[test]
fn index_switch_single_nonzero_case_with_selector_live_after_switch() {
    run_index_switch_constant_results_with_selector_live_after_switch_test(
        &[SwitchCase::new(1, 22)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 23),
            SwitchExpectation::new(2, 13),
        ],
    );
}

#[test]
fn index_switch_two_nonzero_cases_with_selector_live_after_switch() {
    run_index_switch_constant_results_with_selector_live_after_switch_test(
        &[SwitchCase::new(1, 22), SwitchCase::new(2, 33)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 23),
            SwitchExpectation::new(2, 35),
            SwitchExpectation::new(3, 14),
        ],
    );
}

#[test]
fn index_switch_nonzero_contiguous_cases() {
    run_index_switch_test(
        &[SwitchCase::new(1, 22), SwitchCase::new(2, 33), SwitchCase::new(3, 44)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 33),
            SwitchExpectation::new(3, 44),
            SwitchExpectation::new(4, 11),
        ],
    );
}

#[test]
fn index_switch_unsorted_contiguous_cases() {
    run_index_switch_test(
        &[SwitchCase::new(3, 44), SwitchCase::new(1, 22), SwitchCase::new(2, 33)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 33),
            SwitchExpectation::new(3, 44),
            SwitchExpectation::new(4, 11),
        ],
    );
}

#[test]
fn index_switch_sparse_cases() {
    run_index_switch_test(
        &[SwitchCase::new(1, 22), SwitchCase::new(3, 44), SwitchCase::new(5, 66)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 11),
            SwitchExpectation::new(3, 44),
            SwitchExpectation::new(4, 11),
            SwitchExpectation::new(5, 66),
            SwitchExpectation::new(6, 11),
        ],
    );
}

#[test]
fn index_switch_unsorted_sparse_cases() {
    run_index_switch_test(
        &[SwitchCase::new(5, 66), SwitchCase::new(1, 22), SwitchCase::new(3, 44)],
        11,
        &[
            SwitchExpectation::new(0, 11),
            SwitchExpectation::new(1, 22),
            SwitchExpectation::new(2, 11),
            SwitchExpectation::new(3, 44),
            SwitchExpectation::new(4, 11),
            SwitchExpectation::new(5, 66),
            SwitchExpectation::new(6, 11),
        ],
    );
}

#[test]
fn index_switch_max_upper_bound_cases() {
    run_index_switch_test(
        &[
            SwitchCase::new(u32::MAX - 2, 22),
            SwitchCase::new(u32::MAX - 1, 33),
            SwitchCase::new(u32::MAX, 44),
        ],
        11,
        &[
            SwitchExpectation::new(u32::MAX - 3, 11),
            SwitchExpectation::new(u32::MAX - 2, 22),
            SwitchExpectation::new(u32::MAX - 1, 33),
            SwitchExpectation::new(u32::MAX, 44),
        ],
    );
}

#[test]
fn index_switch_max_upper_bound_cases_with_selector_live_after_switch() {
    let cases = [
        SwitchCase::new(u32::MAX - 2, 22),
        SwitchCase::new(u32::MAX - 1, 33),
        SwitchCase::new(u32::MAX, 44),
    ];
    let case_selectors = cases.map(|case| case.selector);

    run_index_switch_execution_test(
        &case_selectors,
        &[
            SwitchExpectation::new(u32::MAX - 3, 4_294_967_287),
            SwitchExpectation::new(u32::MAX - 2, 4_294_967_275),
            SwitchExpectation::new(u32::MAX - 1, 4_294_967_263),
            SwitchExpectation::new(u32::MAX, 4_294_967_251),
        ],
        |builder, _selector, case_index, _case_selector, span| {
            Ok(builder.u32(cases[case_index].result, span))
        },
        |builder, _selector, span| Ok(builder.u32(11, span)),
        xor_selector_with_switch_result,
    );
}

#[test]
fn index_switch_contiguous_cases_with_selector_liveness() {
    run_index_switch_selector_liveness_test(
        &[1, 2, 3],
        &[
            SwitchExpectation::new(0, 99),
            SwitchExpectation::new(1, 1),
            SwitchExpectation::new(2, 22),
            SwitchExpectation::new(3, 3),
            SwitchExpectation::new(4, 99),
        ],
    );
}

#[test]
fn index_switch_sparse_cases_with_selector_liveness() {
    run_index_switch_selector_liveness_test(
        &[1, 2, 5],
        &[
            SwitchExpectation::new(0, 99),
            SwitchExpectation::new(1, 1),
            SwitchExpectation::new(2, 22),
            SwitchExpectation::new(4, 99),
            SwitchExpectation::new(5, 5),
            SwitchExpectation::new(6, 99),
        ],
    );
}

#[test]
fn index_switch_contiguous_cases_with_selector_live_after_switch() {
    run_index_switch_selector_live_after_switch_test(
        &[1, 2, 3],
        &[
            SwitchExpectation::new(0, 99),
            SwitchExpectation::new(1, 2),
            SwitchExpectation::new(2, 24),
            SwitchExpectation::new(3, 6),
            SwitchExpectation::new(4, 103),
        ],
    );
}

#[test]
fn index_switch_sparse_cases_with_selector_live_after_switch() {
    run_index_switch_selector_live_after_switch_test(
        &[1, 2, 5],
        &[
            SwitchExpectation::new(0, 99),
            SwitchExpectation::new(1, 2),
            SwitchExpectation::new(2, 24),
            SwitchExpectation::new(4, 103),
            SwitchExpectation::new(5, 10),
            SwitchExpectation::new(6, 105),
        ],
    );
}
