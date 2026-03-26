use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{Felt, Op, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder};

use super::{compile_test_module, eval_package};

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

/// Compile and execute an `scf.index_switch`, checking the result for each `selector`.
fn run_index_switch_test(
    cases: &[SwitchCase],
    default_result: u32,
    expectations: &[SwitchExpectation],
) {
    let span = SourceSpan::default();
    let case_selectors: Vec<u32> = cases.iter().map(|case| case.selector).collect();
    let case_results: Vec<u32> = cases.iter().map(|case| case.result).collect();

    let (package, context) = compile_test_module([Type::U32], [Type::U32], |builder| {
        let entry = builder.entry_block();
        let selector = entry.borrow().arguments()[0] as ValueRef;

        let switch = builder
            .index_switch(selector, case_selectors.iter().copied(), &[Type::U32], span)
            .unwrap();

        for (index, case_result) in case_results.iter().copied().enumerate() {
            let case_region = switch.borrow().get_case_region(index);
            let case_block = builder.create_block_in_region(case_region);
            builder.switch_to_block(case_block);

            let result = builder.u32(case_result, span);
            builder.r#yield([result], span).unwrap();
        }

        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);

        let result = builder.u32(default_result, span);
        builder.r#yield([result], span).unwrap();

        builder.switch_to_block(entry);
        builder.ret(Some(switch.borrow().results()[0] as ValueRef), span).unwrap();
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
            "unexpected result for selector {selector} with cases {cases:?}"
        );
    }
}

/// Compile and execute a switch where some arms consume the selector and others do not.
fn run_index_switch_selector_liveness_test(cases: &[u32], expectations: &[SwitchExpectation]) {
    let span = SourceSpan::default();

    let (package, context) = compile_test_module([Type::U32], [Type::U32], |builder| {
        let entry = builder.entry_block();
        let selector = entry.borrow().arguments()[0] as ValueRef;

        let switch = builder
            .index_switch(selector, cases.iter().copied(), &[Type::U32], span)
            .unwrap();

        for (index, case_selector) in cases.iter().copied().enumerate() {
            let case_region = switch.borrow().get_case_region(index);
            let case_block = builder.create_block_in_region(case_region);
            builder.switch_to_block(case_block);

            if case_selector % 2 == 0 {
                let result = builder.u32(case_selector * 11, span);
                builder.r#yield([result], span).unwrap();
            } else {
                builder.r#yield([selector], span).unwrap();
            }
        }

        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);
        let result = builder.u32(99, span);
        builder.r#yield([result], span).unwrap();

        builder.switch_to_block(entry);
        builder.ret(Some(switch.borrow().results()[0] as ValueRef), span).unwrap();
    });

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
            "unexpected result for selector {selector} with liveness-sensitive cases {cases:?}"
        );
    }
}

/// Compile and execute a switch that keeps `selector` live after the switch result is produced.
fn run_index_switch_selector_live_after_switch_test(
    cases: &[u32],
    expectations: &[SwitchExpectation],
) {
    let span = SourceSpan::default();

    let (package, context) = compile_test_module([Type::U32], [Type::U32], |builder| {
        let entry = builder.entry_block();
        let selector = entry.borrow().arguments()[0] as ValueRef;

        let switch = builder
            .index_switch(selector, cases.iter().copied(), &[Type::U32], span)
            .unwrap();

        for (index, case_selector) in cases.iter().copied().enumerate() {
            let case_region = switch.borrow().get_case_region(index);
            let case_block = builder.create_block_in_region(case_region);
            builder.switch_to_block(case_block);

            if case_selector % 2 == 0 {
                let result = builder.u32(case_selector * 11, span);
                builder.r#yield([result], span).unwrap();
            } else {
                builder.r#yield([selector], span).unwrap();
            }
        }

        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);
        let result = builder.u32(99, span);
        builder.r#yield([result], span).unwrap();

        builder.switch_to_block(entry);
        let switch_result = switch.borrow().results()[0] as ValueRef;
        let output = builder.add(switch_result, selector, span).unwrap();
        builder.ret(Some(output), span).unwrap();
    });

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
            "unexpected result for selector {selector} with post-switch-live cases {cases:?}"
        );
    }
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
