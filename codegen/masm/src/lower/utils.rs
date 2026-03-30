use midenc_dialect_scf as scf;
use midenc_hir::{Op, Operation, Region, RegionRef, Report, SourceSpan, Spanned, ValueRef};
use smallvec::SmallVec;

use crate::{Constraint, OperandStack, emitter::BlockEmitter, masm, opt::operands::SolverOptions};

/// Emit a conditional branch-like region, e.g. `scf.if`.
///
/// This assumes that the conditional value on top of the stack has been removed from the emitter's
/// view of the stack, but has not yet been consumed by the caller.
pub fn emit_if(
    emitter: &mut BlockEmitter<'_>,
    op: &Operation,
    then_body: &Region,
    else_body: &Region,
) -> Result<(), Report> {
    let span = op.span();
    let then_dest = then_body.entry();
    let else_dest = else_body.entry_block_ref();

    let (then_blk, then_stack) =
        emit_branch_block(op, emitter, then_dest.span(), None, |then_emitter| {
            then_emitter.emit_inline(&then_dest);
            rename_region_results(op, &mut then_emitter.stack);
            Ok(())
        })?;

    let else_blk = match else_dest {
        None => {
            assert!(
                op.results().is_empty(),
                "an elided 'hir.if' else block requires the '{}' to have no results",
                op.name()
            );

            masm::Block::new(span, Default::default())
        }
        Some(dest) => {
            let dest = dest.borrow();
            let (else_blk, else_stack) =
                emit_branch_block(op, emitter, dest.span(), Some(&then_stack), |else_emitter| {
                    else_emitter.emit_inline(&dest);
                    rename_region_results(op, &mut else_emitter.stack);
                    Ok(())
                })?;
            debug_assert_eq!(then_stack, else_stack);
            else_blk
        }
    };

    emitter.emit_op(masm::Op::If {
        span,
        then_blk,
        else_blk,
    });

    emitter.stack = then_stack;

    Ok(())
}

/// A sorted explicit `scf.index_switch` case paired with its region.
#[derive(Clone, Copy, Debug)]
pub(super) struct SwitchCase {
    selector: u32,
    region: RegionRef,
}

impl SwitchCase {
    /// Create a case descriptor that keeps the selector and region paired while lowering.
    const fn new(selector: u32, region: RegionRef) -> Self {
        Self { selector, region }
    }

    /// Get the selector handled by this case.
    pub(super) const fn selector(&self) -> u32 {
        self.selector
    }
}

/// Collect and sort the explicit cases of `op`, preserving their regions.
pub(super) fn sorted_switch_cases(op: &scf::IndexSwitch) -> SmallVec<[SwitchCase; 4]> {
    let mut cases = op
        .cases()
        .iter()
        .copied()
        .enumerate()
        .map(|(index, selector)| SwitchCase::new(selector, op.get_case_region(index)))
        .collect::<SmallVec<[SwitchCase; 4]>>();
    cases.sort_by_key(SwitchCase::selector);
    cases
}

/// Return true when sorted explicit switch cases span one contiguous selector interval.
pub(super) fn are_switch_cases_contiguous(cases: &[SwitchCase]) -> bool {
    cases.windows(2).all(|pair| {
        debug_assert_ne!(
            pair[0].selector(),
            u32::MAX,
            "sorted switch cases cannot place `u32::MAX` before another selector"
        );
        pair[0].selector() + 1 == pair[1].selector()
    })
}

/// The inclusive explicit selector interval spanned by a sorted `scf.index_switch` case slice.
#[derive(Clone, Copy, Debug)]
struct SwitchCaseInterval {
    /// The smallest explicit selector handled by the interval, inclusive.
    lower: u32,
    /// The largest explicit selector handled by the interval, inclusive.
    upper: u32,
}

impl SwitchCaseInterval {
    /// Derive the explicit selector interval represented by `cases`.
    fn from_cases(cases: &[SwitchCase]) -> Self {
        let lower = cases
            .first()
            .expect("switch case interval requires at least one case")
            .selector();
        let upper = cases
            .last()
            .expect("switch case interval requires at least one case")
            .selector();
        Self { lower, upper }
    }
}

/// Emit nested equality checks for a sorted set of explicit switch cases.
///
/// Unlike the binary-search lowering, this path makes no assumptions about the density of the
/// case set, so it is used for small case counts where duplicating the search chain is cheaper
/// than setting up interval guards.
pub(super) fn emit_linear_search(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    cases: &[SwitchCase],
) -> Result<(), Report> {
    let span = op.span();
    let selector = op.selector().as_value_ref();
    let [case, rest @ ..] = cases else {
        return emit_switch_region(op, emitter, &op.default_region());
    };

    let case_is_live_after = {
        let case_region = case.region.borrow();
        emitter
            .liveness
            .is_live_at_start(selector, case_region.entry_block_ref().unwrap())
    };
    let else_needs_selector = if rest.is_empty() {
        let default_region = op.default_region();
        emitter
            .liveness
            .is_live_at_start(selector, default_region.entry_block_ref().unwrap())
    } else {
        true
    };
    let is_live_after_switch = emitter.liveness.is_live_after(selector, op.as_operation());
    if case_is_live_after || else_needs_selector || is_live_after_switch {
        emitter.emitter().dup(0, span);
    }
    emitter.emitter().eq_imm(case.selector.into(), span);

    // Remove the branch condition from the emitter's view of the stack.
    emitter.stack.drop();

    let case_span = case.region.borrow().entry().span();
    let (then_blk, then_stack) =
        emit_branch_block(op.as_operation(), emitter, case_span, None, |then_emitter| {
            let case_region = case.region.borrow();
            emit_switch_region(op, then_emitter, &case_region)
        })?;

    let (else_blk, else_stack) = if rest.is_empty() {
        emit_default_block(op, emitter, Some(&then_stack))?
    } else {
        emit_branch_block(
            op.as_operation(),
            emitter,
            op.span(),
            Some(&then_stack),
            |else_emitter| emit_linear_search(op, else_emitter, rest),
        )?
    };

    debug_assert_eq!(then_stack, else_stack);

    emitter.emit_op(masm::Op::If {
        span,
        then_blk,
        else_blk,
    });
    emitter.stack = then_stack;

    Ok(())
}

/// Emit a binary search for a sorted set of explicit switch cases.
///
/// The helper makes the explicit selector interval part of the lowering state instead of assuming
/// an implicit lower bound of `0`. Values outside the explicit interval are routed to the default
/// region up front, and recursive search only runs once the selector is known to be inside the
/// contiguous interval represented by `cases`.
pub(super) fn emit_binary_search(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    cases: &[SwitchCase],
) -> Result<(), Report> {
    debug_assert!(!cases.is_empty());
    debug_assert!(
        are_switch_cases_contiguous(cases),
        "binary search lowering requires contiguous switch cases"
    );

    let interval = SwitchCaseInterval::from_cases(cases);
    emit_binary_search_with_interval_guard(op, emitter, cases, interval)
}

/// Rename the yielded region results on `stack` to the result values of `op`.
fn rename_region_results(op: &Operation, stack: &mut OperandStack) {
    for (index, result) in op.results().all().into_iter().enumerate() {
        stack.rename(index, *result as ValueRef);
    }
}

/// Realign `emitter` to `expected_stack`, panicking if the branch effects remain observably
/// different after scheduling.
fn align_branch_stack(
    op: &Operation,
    expected_stack: &OperandStack,
    emitter: &mut BlockEmitter<'_>,
) {
    let actual_stack = emitter.stack.clone();
    if expected_stack != &actual_stack {
        schedule_stack_realignment(expected_stack, &actual_stack, emitter);
    }

    if cfg!(debug_assertions) {
        let actual_stack = emitter.stack.clone();
        if expected_stack != &actual_stack {
            panic!(
                "unexpected observable stack effect leaked from regions of {}

stack on exit from expected branch: {expected_stack:#?}
stack on exit from actual branch: {actual_stack:#?}
",
                op
            );
        }
    }
}

/// Emit `region` inline, consuming the selector only when it is dead in both the region and the
/// enclosing switch.
fn emit_switch_region(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    region: &Region,
) -> Result<(), Report> {
    let selector = op.selector().as_value_ref();
    let span = op.span();
    let is_live_in_region =
        emitter.liveness.is_live_at_start(selector, region.entry_block_ref().unwrap());
    let is_live_after_switch = emitter.liveness.is_live_after(selector, op.as_operation());
    if !is_live_in_region
        && !is_live_after_switch
        && let Some(selector_index) = emitter.stack.find(&selector)
    {
        emitter.emitter().drop_operand_at_position(selector_index, span);
    }
    emitter.emit_inline(&region.entry());
    rename_region_results(op.as_operation(), &mut emitter.stack);
    if !is_live_after_switch && let Some(selector_index) = emitter.stack.find(&selector) {
        emitter.emitter().drop_operand_at_position(selector_index, span);
    }

    Ok(())
}

/// Emit a nested branch block and optionally realign its observable stack effect to
/// `expected_stack`.
fn emit_branch_block<F>(
    op: &Operation,
    emitter: &mut BlockEmitter<'_>,
    span: SourceSpan,
    expected_stack: Option<&OperandStack>,
    build: F,
) -> Result<(masm::Block, OperandStack), Report>
where
    F: FnOnce(&mut BlockEmitter<'_>) -> Result<(), Report>,
{
    let mut nested_emitter = emitter.nest();
    build(&mut nested_emitter)?;
    if let Some(expected_stack) = expected_stack {
        align_branch_stack(op, expected_stack, &mut nested_emitter);
    }
    let branch_stack = nested_emitter.stack.clone();
    let branch_block = nested_emitter.into_emitted_block(span);
    Ok((branch_block, branch_stack))
}

/// Emit the default region as a nested block.
fn emit_default_block(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    expected_stack: Option<&OperandStack>,
) -> Result<(masm::Block, OperandStack), Report> {
    let default_region = op.default_region();
    let default_span = default_region.entry().span();
    emit_branch_block(op.as_operation(), emitter, default_span, expected_stack, |nested_emitter| {
        emit_switch_region(op, nested_emitter, &default_region)
    })
}

/// Emit a single out-of-range guard for `cases`, then enter the in-bounds binary search.
fn emit_binary_search_with_interval_guard(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    cases: &[SwitchCase],
    interval: SwitchCaseInterval,
) -> Result<(), Report> {
    let span = op.span();

    match (interval.lower > 0, interval.upper < u32::MAX) {
        (false, false) => return emit_binary_search_in_bounds(op, emitter, cases, interval),
        (true, false) => {
            let mut op_emitter = emitter.emitter();
            op_emitter.dup(0, span);
            op_emitter.lt_imm(interval.lower.into(), span);
        }
        (false, true) => {
            let mut op_emitter = emitter.emitter();
            op_emitter.dup(0, span);
            op_emitter.gt_imm(interval.upper.into(), span);
        }
        (true, true) => {
            let mut op_emitter = emitter.emitter();
            op_emitter.dup(0, span);
            op_emitter.lt_imm(interval.lower.into(), span);
            op_emitter.dup(1, span);
            op_emitter.gt_imm(interval.upper.into(), span);
            op_emitter.or(span);
        }
    }
    emitter.stack.drop();

    let (then_blk, then_stack) = emit_default_block(op, emitter, None)?;
    let (else_blk, else_stack) = emit_branch_block(
        op.as_operation(),
        emitter,
        op.span(),
        Some(&then_stack),
        |else_emitter| emit_binary_search_in_bounds(op, else_emitter, cases, interval),
    )?;

    debug_assert_eq!(then_stack, else_stack);

    emitter.emit_op(masm::Op::If {
        span,
        then_blk,
        else_blk,
    });
    emitter.stack = then_stack;

    Ok(())
}

/// Emit binary search over `cases`, assuming the selector is already inside `interval`.
fn emit_binary_search_in_bounds(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    cases: &[SwitchCase],
    interval: SwitchCaseInterval,
) -> Result<(), Report> {
    let span = op.span();

    match cases {
        [case] => {
            debug_assert_eq!(interval.lower, case.selector);
            debug_assert_eq!(interval.upper, case.selector);
            let case_region = case.region.borrow();
            emit_switch_region(op, emitter, &case_region)
        }
        _ => {
            let split = cases.len() / 2;
            let (left_cases, right_cases) = cases.split_at(split);
            let left_interval = SwitchCaseInterval::from_cases(left_cases);
            let right_interval = SwitchCaseInterval::from_cases(right_cases);

            debug_assert_eq!(interval.lower, left_interval.lower);
            debug_assert_eq!(interval.upper, right_interval.upper);
            debug_assert_ne!(left_interval.upper, u32::MAX);

            {
                let mut op_emitter = emitter.emitter();
                op_emitter.dup(0, span);
                op_emitter.lte_imm(left_interval.upper.into(), span);
            }
            emitter.stack.drop();

            let (then_blk, then_stack) =
                emit_branch_block(op.as_operation(), emitter, op.span(), None, |then_emitter| {
                    emit_binary_search_in_bounds(op, then_emitter, left_cases, left_interval)
                })?;
            debug_assert_eq!(left_interval.upper + 1, right_interval.lower);

            let (else_blk, else_stack) = emit_branch_block(
                op.as_operation(),
                emitter,
                op.span(),
                Some(&then_stack),
                |else_emitter| {
                    emit_binary_search_in_bounds(op, else_emitter, right_cases, right_interval)
                },
            )?;

            debug_assert_eq!(then_stack, else_stack);

            emitter.emit_op(masm::Op::If {
                span,
                then_blk,
                else_blk,
            });
            emitter.stack = then_stack;

            Ok(())
        }
    }
}

/// This analyzes the `lhs` and `rhs` operand stacks, and computes the set of actions required to
/// make `rhs` match `lhs`. Those actions are then applied to `emitter`, such that its stack will
/// match `lhs` once value renaming has been applied.
///
/// NOTE: It is expected that `emitter`'s stack is the same size as `lhs`, and that `lhs` and `rhs`
/// are the same size.
pub fn schedule_stack_realignment(
    lhs: &crate::OperandStack,
    rhs: &crate::OperandStack,
    emitter: &mut BlockEmitter<'_>,
) {
    use crate::opt::{OperandMovementConstraintSolver, SolverError};

    if lhs.is_empty() && rhs.is_empty() {
        return;
    }

    assert_eq!(lhs.len(), rhs.len());

    let trace_target = emitter.trace_target.clone().with_topic("operand-scheduling");

    log::trace!(target: &trace_target, "stack realignment required, scheduling moves..");
    log::trace!(target: &trace_target, "  desired stack state:    {lhs:#?}");
    log::trace!(target: &trace_target, "  misaligned stack state: {rhs:#?}");

    let mut constraints = SmallVec::<[Constraint; 8]>::with_capacity(lhs.len());
    constraints.resize(lhs.len(), Constraint::Move);

    let expected = lhs
        .iter()
        .rev()
        .map(|o| o.as_value().expect("unexpected operand type"))
        .collect::<SmallVec<[_; 8]>>();
    let options = SolverOptions {
        trace_target: emitter.trace_target.clone().with_topic("solver"),
        ..SolverOptions::default()
    };
    match OperandMovementConstraintSolver::new_with_options(&expected, &constraints, rhs, options) {
        Ok(solver) => {
            solver
                .solve_and_apply(&mut emitter.emitter(), Default::default())
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to realign stack\nwith error: {err:?}\nconstraints: \
                         {constraints:?}\nexpected: {lhs:#?}\nstack: {rhs:#?}",
                    )
                });
        }
        Err(SolverError::AlreadySolved) => (),
        Err(err) => {
            panic!("unexpected error constructing operand movement constraint solver: {err:?}")
        }
    }
}

#[cfg(test)]
mod tests {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_scf::StructuredControlFlowOpBuilder;
    use midenc_expect_test::expect_file;
    use midenc_hir::{
        TraceTarget, Type,
        dialects::builtin::{self, BuiltinOpBuilder, FunctionRef},
        formatter::PrettyPrint,
        pass::AnalysisManager,
        testing::Test,
        version::Version,
    };
    use midenc_hir_analysis::analyses::LivenessAnalysis;

    use super::*;
    use crate::{OperandStack, linker::LinkInfo};

    #[test]
    fn util_emit_if_test() -> Result<(), Report> {
        let mut test = Test::new("util_emit_if_test", &[Type::U32, Type::U32], &[Type::U32]);

        let function_ref = test.function();
        let (a, b) = {
            let span = function_ref.span();
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let a = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let b = builder.entry_block().borrow().arguments()[1] as ValueRef;

            // Unused in `then` branch, used on `else` branch
            let count = builder.u32(0, span);

            let is_eq = builder.eq(a, b, span)?;
            let conditional = builder.r#if(is_eq, &[Type::U32], span)?;

            let then_region = conditional.borrow().then_body().as_region_ref();
            let then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            let is_true = builder.u32(1, span);
            builder.r#yield([is_true], span)?;

            let else_region = conditional.borrow().else_body().as_region_ref();
            let else_block = builder.create_block_in_region(else_region);
            builder.switch_to_block(else_block);
            let is_false = builder.mul(a, count, span)?;
            builder.r#yield([is_false], span)?;

            builder.switch_to_block(entry);
            builder.ret(Some(conditional.borrow().results()[0] as ValueRef), span)?;

            (a, b)
        };

        // Obtain liveness
        let analysis_manager = AnalysisManager::new(function_ref.as_operation_ref(), None);
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        // Generate linker info
        let link_info = LinkInfo::new(builtin::ComponentId {
            namespace: "root".into(),
            name: "root".into(),
            version: Version::new(1, 0, 0),
        });

        let mut stack = OperandStack::new(test.context_rc());
        stack.push(b);
        stack.push(a);

        // Instantiate block emitter
        let function_name = *function_ref.borrow().get_name();
        let mut invoked = Default::default();
        let emitter = BlockEmitter {
            liveness: &liveness,
            link_info: &link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target: TraceTarget::category("codegen")
                .with_relevant_symbol(function_name.as_symbol()),
        };

        // Lower input
        let function = function_ref.borrow();
        let entry = function.entry_block();
        let body = emitter.emit(&entry.borrow());

        // Verify emitted block contents
        let input = format!("{}", function.as_operation());
        let test_file_hir = format!("expected/{}.hir", test.name());
        expect_file![&test_file_hir].assert_eq(&input);

        let output = body.to_pretty_string();
        let test_file_masm = format!("expected/{}.masm", test.name());
        expect_file![&test_file_masm].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_if_nested_test() -> Result<(), Report> {
        let mut test = Test::new("util_emit_if_nested_test", &[Type::U32, Type::U32], &[Type::U32]);

        let function_ref = test.function();

        let (a, b) = {
            let span = function_ref.span();
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let a = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let b = builder.entry_block().borrow().arguments()[1] as ValueRef;

            let is_eq = builder.eq(a, b, span)?;
            let conditional = builder.r#if(is_eq, &[Type::U32], span)?;

            let then_region = conditional.borrow().then_body().as_region_ref();
            let then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            let case1 = builder.u32(1, span);
            builder.r#yield([case1], span)?;

            let else_region = conditional.borrow().else_body().as_region_ref();
            let else_block = builder.create_block_in_region(else_region);
            builder.switch_to_block(else_block);

            let is_lt = builder.lt(a, b, span)?;
            let nested = builder.r#if(is_lt, &[Type::U32], span)?;
            let then_region = nested.borrow().then_body().as_region_ref();
            let then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            let case2 = builder.u32(2, span);
            builder.r#yield([case2], span)?;

            let else_region = nested.borrow().else_body().as_region_ref();
            let nested_else_block = builder.create_block_in_region(else_region);
            builder.switch_to_block(nested_else_block);
            let case3 = builder.mul(a, b, span)?;
            builder.r#yield([case3], span)?;

            builder.switch_to_block(else_block);
            builder.r#yield([nested.borrow().results()[0] as ValueRef], span)?;

            builder.switch_to_block(entry);
            builder.ret(Some(conditional.borrow().results()[0] as ValueRef), span)?;

            (a, b)
        };

        // Obtain liveness
        let analysis_manager = AnalysisManager::new(function_ref.as_operation_ref(), None);
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        // Generate linker info
        let link_info = LinkInfo::new(builtin::ComponentId {
            namespace: "root".into(),
            name: "root".into(),
            version: Version::new(1, 0, 0),
        });

        let mut stack = OperandStack::new(test.context_rc());
        stack.push(b);
        stack.push(a);

        // Instantiate block emitter
        let function_name = *function_ref.borrow().get_name();
        let mut invoked = Default::default();
        let emitter = BlockEmitter {
            liveness: &liveness,
            link_info: &link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target: TraceTarget::category("codegen")
                .with_relevant_symbol(function_name.as_symbol()),
        };

        // Lower input
        let function = function_ref.borrow();
        let entry = function.entry_block();
        let body = emitter.emit(&entry.borrow());

        // Verify emitted block contents
        let input = format!("{}", function.as_operation());
        let test_file_hir = format!("expected/{}.hir", test.name());
        expect_file![&test_file_hir].assert_eq(&input);

        let output = body.to_pretty_string();
        let test_file_masm = format!("expected/{}.masm", test.name());
        expect_file![&test_file_masm].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_single_case_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_single_case_test");

        let (function, block) = generate_switch_lowering_test(&mut test, 1)?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_two_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_two_cases_test");

        let (function, block) = generate_switch_lowering_test(&mut test, 2)?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_three_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_three_cases_test");

        let (function, block) = generate_switch_lowering_test(&mut test, 3)?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_four_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_four_cases_test");

        let (function, block) = generate_switch_lowering_test(&mut test, 4)?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_five_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_five_cases_test");

        let (function, block) = generate_switch_lowering_test(&mut test, 5)?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_seven_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_seven_cases_test");

        let (function, block) = generate_switch_lowering_test(&mut test, 7)?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_two_nonzero_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_two_nonzero_cases_test");

        let (function, block) = generate_switch_lowering_test_with_cases(&mut test, &[1, 2])?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_sparse_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_sparse_cases_test");

        let (function, block) = generate_switch_lowering_test_with_cases(&mut test, &[1, 3, 5])?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_nonzero_contiguous_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_nonzero_contiguous_cases_test");

        let (function, block) = generate_switch_lowering_test_with_cases(&mut test, &[1, 2, 3])?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_unsorted_contiguous_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_unsorted_contiguous_cases_test");

        let (function, block) = generate_switch_lowering_test_with_cases(&mut test, &[3, 1, 2])?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    #[test]
    fn util_switch_lowering_unsorted_sparse_cases_test() -> Result<(), Report> {
        let mut test = Test::named("util_switch_lowering_unsorted_sparse_cases_test");

        let (function, block) = generate_switch_lowering_test_with_cases(&mut test, &[5, 1, 3])?;

        assert_switch_lowering_output(test.name(), &function, &block);

        Ok(())
    }

    /// Verify the HIR and MASM snapshots emitted for lowered switch code.
    fn assert_switch_lowering_output(test_name: &str, function: &FunctionRef, block: &masm::Block) {
        let test_file_hir = format!("expected/{test_name}.hir");
        let input = format!("{}", function.borrow().as_operation());
        expect_file![&test_file_hir].assert_eq(&input);

        let test_file_masm = format!("expected/{test_name}.masm");
        let output = block.to_pretty_string();
        expect_file![&test_file_masm].assert_eq(&output);
    }

    /// Generate a switch-lowering fixture whose explicit cases are `0..num_cases`.
    fn generate_switch_lowering_test(
        test: &mut Test,
        num_cases: usize,
    ) -> Result<(FunctionRef, masm::Block), Report> {
        let cases = SmallVec::<[_; 4]>::from_iter(0u32..(num_cases as u32));
        generate_switch_lowering_test_with_cases(test, &cases)
    }

    /// Generate a switch-lowering fixture whose explicit case selectors are taken from `cases`.
    fn generate_switch_lowering_test_with_cases(
        test: &mut Test,
        cases: &[u32],
    ) -> Result<(FunctionRef, masm::Block), Report> {
        let name = test.name();
        test.with_function(name, &[Type::U32, Type::U32], &[Type::U32]);
        let function_ref = test.function();

        let (a, b) = {
            let span = function_ref.span();
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let a = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let b = builder.entry_block().borrow().arguments()[1] as ValueRef;

            let switch = builder.index_switch(a, cases.iter().copied(), &[Type::U32], span)?;

            let fallback_region = switch.borrow().default_region().as_region_ref();
            let case_regions = (0..cases.len())
                .map(|index| (cases[index], switch.borrow().get_case_region(index)));

            for (case, case_region) in case_regions {
                let case_block = builder.create_block_in_region(case_region);
                builder.switch_to_block(case_block);
                let case_result = builder.u32(case, span);
                builder.r#yield([case_result], span)?;
            }

            let fallback_block = builder.create_block_in_region(fallback_region);
            builder.switch_to_block(fallback_block);
            let fallback_result = builder.mul(a, b, span)?;
            builder.r#yield([fallback_result], span)?;

            builder.switch_to_block(entry);
            builder.ret(Some(switch.borrow().results()[0] as ValueRef), span)?;

            (a, b)
        };

        // Obtain liveness
        let analysis_manager = AnalysisManager::new(function_ref.as_operation_ref(), None);
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        // Generate linker info
        let link_info = LinkInfo::new(builtin::ComponentId {
            namespace: "root".into(),
            name: "root".into(),
            version: Version::new(1, 0, 0),
        });

        let mut stack = OperandStack::new(test.context_rc());
        stack.push(b);
        stack.push(a);

        // Instantiate block emitter
        let mut invoked = Default::default();
        let emitter = BlockEmitter {
            liveness: &liveness,
            link_info: &link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target: TraceTarget::category("codegen").with_relevant_symbol(name),
        };

        // Lower input
        let function = function_ref.borrow();
        let entry = function.entry_block();
        let body = emitter.emit(&entry.borrow());

        Ok((function_ref, body))
    }
}
