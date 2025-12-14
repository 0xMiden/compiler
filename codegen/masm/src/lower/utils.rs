use midenc_dialect_scf as scf;
use midenc_hir::{Op, Operation, Region, Report, Spanned, ValueRef};
use smallvec::SmallVec;

use crate::{Constraint, emitter::BlockEmitter, masm};

/// Emit a conditonal branch-like region, e.g. `scf.if`.
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

    let (then_stack, then_blk) = {
        let mut then_emitter = emitter.nest();
        then_emitter.emit_inline(&then_dest);
        // Rename the yielded values on the stack for us to check against
        let mut then_stack = then_emitter.stack.clone();
        for (index, result) in op.results().all().into_iter().enumerate() {
            then_stack.rename(index, *result as ValueRef);
        }
        let then_block = then_emitter.into_emitted_block(then_dest.span());
        (then_stack, then_block)
    };

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
            let mut else_emitter = emitter.nest();
            else_emitter.emit_inline(&dest);

            // Rename the yielded values on the stack for us to check against
            let mut else_stack = else_emitter.stack.clone();
            for (index, result) in op.results().all().into_iter().enumerate() {
                else_stack.rename(index, *result as ValueRef);
            }

            // Schedule realignment of the stack if needed
            if then_stack != else_stack {
                schedule_stack_realignment(&then_stack, &else_stack, &mut else_emitter);
            }

            if cfg!(debug_assertions) {
                let mut else_stack = else_emitter.stack.clone();
                for (index, result) in op.results().all().into_iter().enumerate() {
                    else_stack.rename(index, *result as ValueRef);
                }
                if then_stack != else_stack {
                    panic!(
                        "unexpected observable stack effect leaked from regions of {op}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
",
                    );
                }
            }

            else_emitter.into_emitted_block(dest.span())
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

/// Emit a sequence of nested branches that perform a binary search for a case which matches some
/// selector value on top of the operand stack.
///
/// For now, this has the following requirements:
///
/// * The selector value is on top of the stack when called, and the emitter is still aware of it
/// * The cases which have been partitioned into `a` and `b` are contiguous, e.g. `[1, 2]` and
///   `[3, 4]`.
/// * If `a` is empty, then `b` is processed such that the fallback case will be emitted once only
///   a single case in `b` remains (as a branch between that case and the fallback case). If there
///   are two cases, then the search will partition them into the "then" branch, with the fallback
///   in the "else" branch.
/// * If `a` is non-empty, and `b` is empty, then `num_cases` dictates whether we handle `a` as
///   described in the previous bullet point, i.e. if the total number of cases is 2, and `a` has
///   two cases, then we will partition them into the "then" branch, and emit the fallback in the
///   "else" branch. Otherwise, if the number of cases is greater than 2, and `a` has <= 2 cases,
///   then they will be emitted into the "then" branch without emitting the fallback case.
///   Otherwise, `a` will be partitioned such that we can recursively call this function and rely
///   on only emitting the fallback case once, in the final "else" branch.
///
/// # Parameters
///
/// * `midpoint` is the case value (real or otherwise) representing the approximate middle of the
///   range of cases. It is used to derive the actual partition point that produced `a` and `b`
/// * `a` is the set of cases which are < the partition point derived from `midpoint`
/// * `b` is the set of cases which are >= the partition point derived from `midpoint`
/// * `num_cases` is the total number of cases that were partitioned into `a` and `b`. This is used
///   to inform us whether or not there are additional cases to be emitted, or if we should emit
///   the fallback case once the search is exhausted.
pub fn emit_binary_search(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    a: &[u32],
    b: &[u32],
    midpoint: u32,
    num_cases: usize,
) -> Result<(), Report> {
    let span = op.span();
    let selector = op.selector().as_value_ref();

    match a {
        [] => {
            match b {
                [then_case] => {
                    // There is only a single case to emit, so we can emit an 'hir.if' with fallback
                    //
                    // Emit `selector == then_case`
                    //
                    // NOTE: We duplicate the selector if it is live in either the case region or
                    // the fallback region
                    let then_index = op.get_case_index_for_selector(*then_case).unwrap();
                    let then_body = op.get_case_region(then_index);
                    let else_body = op.default_region();
                    let is_live_after = emitter
                        .liveness
                        .is_live_at_start(selector, then_body.borrow().entry_block_ref().unwrap())
                        || emitter
                            .liveness
                            .is_live_at_start(selector, else_body.entry_block_ref().unwrap());
                    if is_live_after {
                        emitter.emitter().dup(0, span);
                    }
                    emitter.emitter().eq_imm(b[0].into(), span);

                    // Remove the condition for the if from the emitter's view of the stack
                    emitter.stack.drop();

                    // Emit as 'hir.if'
                    emit_if(emitter, op.as_operation(), &then_body.borrow(), &else_body)
                }
                [_then_case, else_case] => {
                    // This is similar to the case of a = [_, _], b is non-empty
                    //
                    // We must emit an `hir.if` for then/else cases in the first branch, and place
                    // the fallback in the second branch.
                    //
                    // Emit `selector <= else_case ? (selector == then_case : then_case ? else_case) ? fallback`
                    {
                        let mut emitter = emitter.emitter();
                        emitter.dup(0, span);
                        emitter.lte_imm((*else_case).into(), span);
                    }

                    // Remove the condition for the branch selection from the emitter's view of the
                    // stack
                    emitter.stack.drop();

                    let (then_blk, then_stack) = {
                        let mut then_emitter = emitter.nest();
                        emit_binary_search(op, &mut then_emitter, b, &[], midpoint, usize::MAX)?;
                        let then_stack = then_emitter.stack.clone();
                        (then_emitter.into_emitted_block(span), then_stack)
                    };

                    let (else_blk, else_stack) = {
                        let default_region = op.default_region();
                        let is_live_after = emitter
                            .liveness
                            .is_live_at_start(selector, default_region.entry_block_ref().unwrap());
                        let mut else_emitter = emitter.nest();
                        if !is_live_after {
                            // Consume the original selector
                            else_emitter.emitter().drop(span);
                        }
                        else_emitter.emit_inline(&default_region.entry());
                        // Rename the yielded values on the stack for us to check against
                        let mut else_stack = else_emitter.stack.clone();
                        for (index, result) in op.results().all().into_iter().enumerate() {
                            else_stack.rename(index, *result as ValueRef);
                        }
                        (else_emitter.into_emitted_block(span), else_stack)
                    };

                    if then_stack != else_stack {
                        panic!(
                            "unexpected observable stack effect leaked from regions of {}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
                        ",
                            op.as_operation()
                        );
                    }

                    emitter.emit_op(masm::Op::If {
                        span,
                        then_blk,
                        else_blk,
                    });

                    emitter.stack = then_stack;

                    Ok(())
                }
                _ => panic!(
                    "unexpected partitioning of switch cases: a = empty, b = {b:#?}, midpoint = \
                     {midpoint}"
                ),
            }
        }
        [_then_case, else_case] if b.is_empty() && num_cases == 2 => {
            // There were exactly two cases and we are handling them here, but we must also emit
            // a fallback branch in the case where an out of range selector value is given
            //
            // We must emit an `hir.if` for then/else cases in the first branch, and place
            // the fallback in the second branch.
            //
            // Emit `selector <= else_case ? (selector == then_case : then_case ? else_case) ? fallback`
            {
                let mut emitter = emitter.emitter();
                emitter.dup(0, span);
                emitter.lte_imm((*else_case).into(), span);
            }

            // Remove the condition for the branch selection from the emitter's view of the
            // stack
            emitter.stack.drop();

            let (then_blk, then_stack) = {
                let mut then_emitter = emitter.nest();
                emit_binary_search(op, &mut then_emitter, a, &[], midpoint, usize::MAX)?;
                let then_stack = then_emitter.stack.clone();
                (then_emitter.into_emitted_block(span), then_stack)
            };

            let (else_blk, else_stack) = {
                let default_region = op.default_region();
                let is_live_after = emitter
                    .liveness
                    .is_live_at_start(selector, default_region.entry_block_ref().unwrap());
                let mut else_emitter = emitter.nest();
                if !is_live_after {
                    // Consume the original selector
                    else_emitter.emitter().drop(span);
                }
                else_emitter.emit_inline(&default_region.entry());
                // Rename the yielded values on the stack for us to check against
                let mut else_stack = else_emitter.stack.clone();
                for (index, result) in op.results().all().into_iter().enumerate() {
                    else_stack.rename(index, *result as ValueRef);
                }
                (else_emitter.into_emitted_block(span), else_stack)
            };

            if then_stack != else_stack {
                panic!(
                    "unexpected observable stack effect leaked from regions of {}

            stack on exit from 'then': {then_stack:#?}
            stack on exit from 'else': {else_stack:#?}
                                    ",
                    op.as_operation()
                );
            }

            emitter.emit_op(masm::Op::If {
                span,
                then_blk,
                else_blk,
            });

            emitter.stack = then_stack;

            Ok(())
        }
        [then_case, else_case] if b.is_empty() && num_cases > 2 => {
            // We can emit 'a' as an 'hir.if' with no fallback, as this is a subset of the total
            // cases and we were given enough to populate a single `hir.if`.
            //
            // Emit `selector == then_case`
            let then_index = op.get_case_index_for_selector(*then_case).unwrap();
            let then_body = op.get_case_region(then_index);
            let else_index = op.get_case_index_for_selector(*else_case).unwrap();
            let else_body = op.get_case_region(else_index);
            let is_live_after = emitter
                .liveness
                .is_live_at_start(selector, then_body.borrow().entry_block_ref().unwrap())
                || emitter
                    .liveness
                    .is_live_at_start(selector, else_body.borrow().entry_block_ref().unwrap());
            if is_live_after {
                emitter.emitter().dup(0, span);
            }
            emitter.emitter().eq_imm((*then_case).into(), span);

            // Remove the selector from the emitter's view of the stack
            emitter.stack.drop();

            // Emit as 'hir.if'
            emit_if(emitter, op.as_operation(), &then_body.borrow(), &else_body.borrow())
        }
        [_then_case, else_case] => {
            // We need to emit an 'hir.if' to split the search at the midpoint, and emit 'a' in
            // the then region, and then recurse with 'b' on the else region
            //
            // Emit `selector < partition_point`
            {
                let mut emitter = emitter.emitter();
                emitter.dup(0, span);
                emitter.lte_imm((*else_case).into(), span);
            }

            // Remove the selector used for this branch selection from the emitter's view of the
            // stack
            emitter.stack.drop();

            let (then_blk, then_stack) = {
                let mut then_emitter = emitter.nest();
                emit_binary_search(op, &mut then_emitter, a, &[], midpoint, usize::MAX)?;
                let then_stack = then_emitter.stack.clone();
                (then_emitter.into_emitted_block(span), then_stack)
            };

            // If we have exactly
            let (else_blk, else_stack) = {
                let mut else_emitter = emitter.nest();
                let midpoint = b[0].midpoint(b[b.len() - 1]);
                let partition_point = core::cmp::min(
                    b.len(),
                    b.partition_point(|item| *item < midpoint).next_multiple_of(2),
                );
                let (b_then, b_else) = b.split_at(partition_point);
                emit_binary_search(op, &mut else_emitter, b_then, b_else, midpoint, b.len())?;
                let else_stack = else_emitter.stack.clone();
                (else_emitter.into_emitted_block(span), else_stack)
            };

            if then_stack != else_stack {
                panic!(
                    "unexpected observable stack effect leaked from regions of {}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
                ",
                    op.as_operation()
                );
            }

            emitter.emit_op(masm::Op::If {
                span,
                then_blk,
                else_blk,
            });

            emitter.stack = then_stack;

            Ok(())
        }
        a => {
            {
                let mut emitter = emitter.emitter();
                emitter.dup(0, span);
                emitter.lte_imm(midpoint.into(), span);
            }

            // Remove the selector used for this branch selection from the emitter's view of the
            // stack
            emitter.stack.drop();

            let (then_blk, then_stack) = {
                let mut then_emitter = emitter.nest();
                let midpoint = a[0].midpoint(a[a.len() - 1]);
                let partition_point = core::cmp::min(
                    a.len(),
                    a.partition_point(|item| *item < midpoint).next_multiple_of(2),
                );
                let (a_then, a_else) = a.split_at(partition_point);
                emit_binary_search(op, &mut then_emitter, a_then, a_else, midpoint, a.len())?;
                let then_stack = then_emitter.stack.clone();
                (then_emitter.into_emitted_block(span), then_stack)
            };

            let (else_blk, else_stack) = {
                let mut else_emitter = emitter.nest();
                let midpoint = b[0].midpoint(b[b.len() - 1]);
                let partition_point = core::cmp::min(
                    b.len(),
                    b.partition_point(|item| *item < midpoint).next_multiple_of(2),
                );
                let (b_then, b_else) = b.split_at(partition_point);
                emit_binary_search(op, &mut else_emitter, b_then, b_else, midpoint, b.len())?;
                let else_stack = else_emitter.stack.clone();
                (else_emitter.into_emitted_block(span), else_stack)
            };

            if then_stack != else_stack {
                panic!(
                    "unexpected observable stack effect leaked from regions of {}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
                ",
                    op.as_operation()
                );
            }

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

    log::trace!(target: "codegen", "stack realignment required, scheduling moves..");
    log::trace!(target: "codegen", "  desired stack state:    {lhs:#?}");
    log::trace!(target: "codegen", "  misaligned stack state: {rhs:#?}");

    let mut constraints = SmallVec::<[Constraint; 8]>::with_capacity(lhs.len());
    constraints.resize(lhs.len(), Constraint::Move);

    let expected = lhs
        .iter()
        .rev()
        .map(|o| o.as_value().expect("unexpected operand type"))
        .collect::<SmallVec<[_; 8]>>();
    match OperandMovementConstraintSolver::new(&expected, &constraints, rhs) {
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
    use alloc::rc::Rc;

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_scf::StructuredControlFlowOpBuilder;
    use midenc_expect_test::expect_file;
    use midenc_hir::{
        AbiParam, Context, Ident, OpBuilder, Signature, Type,
        dialects::builtin::{self, BuiltinOpBuilder, FunctionBuilder, FunctionRef},
        formatter::PrettyPrint,
        pass::AnalysisManager,
        version::Version,
    };
    use midenc_hir_analysis::analyses::LivenessAnalysis;

    use super::*;
    use crate::{OperandStack, linker::LinkInfo};

    #[test]
    fn util_emit_if_test() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let mut builder = OpBuilder::new(context.clone());

        let function_ref = builder.create_function(
            Ident::with_empty_span("test".into()),
            Signature::new(
                [AbiParam::new(Type::U32), AbiParam::new(Type::U32)],
                [AbiParam::new(Type::U32)],
            ),
        )?;

        let (a, b) = {
            let span = function_ref.span();
            let mut builder = FunctionBuilder::new(function_ref, &mut builder);
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

        let mut stack = OperandStack::default();
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
        };

        // Lower input
        let function = function_ref.borrow();
        let entry = function.entry_block();
        let body = emitter.emit(&entry.borrow());

        // Verify emitted block contents
        let input = format!("{}", function.as_operation());
        expect_file!["expected/utils_emit_if.hir"].assert_eq(&input);

        let output = body.to_pretty_string();
        expect_file!["expected/utils_emit_if.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_if_nested_test() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let mut builder = OpBuilder::new(context.clone());

        let function_ref = builder.create_function(
            Ident::with_empty_span("test".into()),
            Signature::new(
                [AbiParam::new(Type::U32), AbiParam::new(Type::U32)],
                [AbiParam::new(Type::U32)],
            ),
        )?;

        let (a, b) = {
            let span = function_ref.span();
            let mut builder = FunctionBuilder::new(function_ref, &mut builder);
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

        let mut stack = OperandStack::default();
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
        };

        // Lower input
        let function = function_ref.borrow();
        let entry = function.entry_block();
        let body = emitter.emit(&entry.borrow());

        // Verify emitted block contents
        let input = format!("{}", function.as_operation());
        expect_file!["expected/utils_emit_if_nested.hir"].assert_eq(&input);

        let output = body.to_pretty_string();
        expect_file!["expected/utils_emit_if_nested.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_binary_search_single_case_test() -> Result<(), Report> {
        let _ = env_logger::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();

        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let (function, block) = generate_emit_binary_search_test(1, context.clone())?;

        // Verify emitted block contents
        let input = format!("{}", function.borrow().as_operation());
        expect_file!["expected/utils_emit_binary_search_1_case.hir"].assert_eq(&input);

        let output = block.to_pretty_string();
        expect_file!["expected/utils_emit_binary_search_1_case.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_binary_search_two_cases_test() -> Result<(), Report> {
        let _ = env_logger::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();

        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let (function, block) = generate_emit_binary_search_test(2, context.clone())?;

        // Verify emitted block contents
        let input = format!("{}", function.borrow().as_operation());
        expect_file!["expected/utils_emit_binary_search_2_cases.hir"].assert_eq(&input);

        let output = block.to_pretty_string();
        expect_file!["expected/utils_emit_binary_search_2_cases.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_binary_search_three_cases_test() -> Result<(), Report> {
        let _ = env_logger::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();

        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let (function, block) = generate_emit_binary_search_test(3, context.clone())?;

        // Verify emitted block contents
        let input = format!("{}", function.borrow().as_operation());
        expect_file!["expected/utils_emit_binary_search_3_cases.hir"].assert_eq(&input);

        let output = block.to_pretty_string();
        expect_file!["expected/utils_emit_binary_search_3_cases.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_binary_search_four_cases_test() -> Result<(), Report> {
        let _ = env_logger::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();

        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let (function, block) = generate_emit_binary_search_test(4, context.clone())?;

        // Verify emitted block contents
        let input = format!("{}", function.borrow().as_operation());
        expect_file!["expected/utils_emit_binary_search_4_cases.hir"].assert_eq(&input);

        let output = block.to_pretty_string();
        expect_file!["expected/utils_emit_binary_search_4_cases.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_binary_search_five_cases_test() -> Result<(), Report> {
        let _ = env_logger::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();

        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let (function, block) = generate_emit_binary_search_test(5, context.clone())?;

        // Verify emitted block contents
        let input = format!("{}", function.borrow().as_operation());
        expect_file!["expected/utils_emit_binary_search_5_cases.hir"].assert_eq(&input);

        let output = block.to_pretty_string();
        expect_file!["expected/utils_emit_binary_search_5_cases.masm"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn util_emit_binary_search_seven_cases_test() -> Result<(), Report> {
        let _ = env_logger::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();

        let context = Rc::new(Context::default());
        crate::register_dialect_hooks(&context);

        let (function, block) = generate_emit_binary_search_test(7, context.clone())?;

        // Verify emitted block contents
        let input = format!("{}", function.borrow().as_operation());
        expect_file!["expected/utils_emit_binary_search_7_cases.hir"].assert_eq(&input);

        let output = block.to_pretty_string();
        expect_file!["expected/utils_emit_binary_search_7_cases.masm"].assert_eq(&output);

        Ok(())
    }

    fn generate_emit_binary_search_test(
        num_cases: usize,
        context: Rc<Context>,
    ) -> Result<(FunctionRef, masm::Block), Report> {
        let mut builder = OpBuilder::new(context.clone());

        let function_ref = builder.create_function(
            Ident::with_empty_span("test".into()),
            Signature::new(
                [AbiParam::new(Type::U32), AbiParam::new(Type::U32)],
                [AbiParam::new(Type::U32)],
            ),
        )?;

        let (a, b) = {
            let span = function_ref.span();
            let mut builder = FunctionBuilder::new(function_ref, &mut builder);
            let entry = builder.entry_block();
            let a = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let b = builder.entry_block().borrow().arguments()[1] as ValueRef;

            let cases = SmallVec::<[_; 4]>::from_iter(0u32..(num_cases as u32));
            let switch = builder.index_switch(a, cases, &[Type::U32], span)?;

            let fallback_region = switch.borrow().default_region().as_region_ref();
            let case_regions = (0..num_cases).map(|index| switch.borrow().get_case_region(index));

            for (case, case_region) in case_regions.enumerate() {
                let case_block = builder.create_block_in_region(case_region);
                builder.switch_to_block(case_block);
                let case_result = builder.u32(case as u32, span);
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

        let mut stack = OperandStack::default();
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
        };

        // Lower input
        let function = function_ref.borrow();
        let entry = function.entry_block();
        let body = emitter.emit(&entry.borrow());

        Ok((function_ref, body))
    }
}
