//! Rebuilding a [While] operation with fewer iteration arguments or results.
//!
//! Several canonicalization patterns remove iteration arguments or results of an `scf.while`.
//! They all do it by replacing the loop with a new one built from the regions of the original,
//! which keeps the operands, block arguments, terminators and results of the loop consistent
//! with each other at every step. [rebuild_while] is the single implementation of that rewrite.

use midenc_hir::{dialects::debuginfo::transform::erase_debug_info_with, patterns::Rewriter, *};

use crate::*;

/// What a rewrite does with one iteration argument of an `scf.while`, i.e. with the init operand,
/// the before block argument and the yield operand at the same position.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum IterArg {
    /// The iteration argument is kept.
    Keep,
    /// The iteration argument is removed. The uses of its before block argument are replaced with
    /// the given value. `None` is for an argument without real uses: the `di.debug_value`
    /// operations that still use it become `di.debug_kill`.
    Remove(Option<ValueRef>),
}

/// Replaces `operation`, an `scf.while`, with a new `scf.while` that has a subset of its
/// iteration arguments and results, and moves the body of both regions into the new op.
///
/// `iter_args` has one entry per iteration argument of the original loop.
///
/// `results` has one entry per result of the original loop: the index of the result of the new
/// loop that replaces it, or `None` if the result and the after block argument at the same
/// position are dropped without replacement, which requires that neither has real uses (the
/// `di.debug_value` operations using them become `di.debug_kill`). Every result of the new loop
/// must replace at least one result of the original one; when it replaces several, they must all
/// be forwarded the same value by the `scf.condition`.
///
/// Returns `Ok(false)`, with nothing modified, if `operation` is not an `scf.while`.
pub(super) fn rebuild_while(
    rewriter: &mut dyn Rewriter,
    operation: OperationRef,
    iter_args: &[IterArg],
    results: &[Option<usize>],
) -> Result<bool, Report> {
    let op = operation.borrow();
    let Some(while_op) = op.downcast_ref::<While>() else {
        return Ok(false);
    };

    let before_block = while_op.before().entry_block_ref().unwrap();
    let after_block = while_op.after().entry_block_ref().unwrap();
    let mut cond_op = while_op.condition_op();
    let mut yield_op = while_op.yield_op();
    let inits = while_op
        .inits()
        .into_iter()
        .map(|o| o.borrow().as_value_ref())
        .collect::<SmallVec<[_; 4]>>();
    let yielded = yield_op
        .borrow()
        .yielded()
        .into_iter()
        .map(|o| o.borrow().as_value_ref())
        .collect::<SmallVec<[_; 4]>>();
    let forwarded = cond_op
        .borrow()
        .forwarded()
        .into_iter()
        .map(|o| o.borrow().as_value_ref())
        .collect::<SmallVec<[_; 4]>>();

    // The lists above are paired up by position below; the verifier of `scf.while` guarantees
    // that they agree with each other.
    assert_eq!(iter_args.len(), inits.len(), "expected one entry per iteration argument");
    assert_eq!(results.len(), forwarded.len(), "expected one entry per result");

    let is_kept = |index: &usize| matches!(iter_args[*index], IterArg::Keep);
    let new_inits =
        (0..inits.len()).filter(is_kept).map(|i| inits[i]).collect::<SmallVec<[_; 4]>>();
    let new_yielded = (0..inits.len())
        .filter(is_kept)
        .map(|i| yielded[i])
        .collect::<SmallVec<[_; 4]>>();

    // Each result of the new loop takes its type and its forwarded value from the first result
    // of the original loop that it replaces.
    let num_new_results = results.iter().flatten().map(|index| index + 1).max().unwrap_or(0);
    let mut new_forwarded = SmallVec::<[ValueRef; 4]>::with_capacity(num_new_results);
    let mut new_result_types = SmallVec::<[Type; 4]>::with_capacity(num_new_results);
    for new_index in 0..num_new_results {
        let index = results
            .iter()
            .position(|result| *result == Some(new_index))
            .expect("every result of the new loop must replace a result of the original loop");
        new_forwarded.push(forwarded[index]);
        new_result_types.push(while_op.results()[index].borrow().ty().clone());
    }

    // Creating the new op is the only fallible step; nothing has been modified before it.
    let new_while =
        rewriter.r#while(new_inits.iter().copied(), &new_result_types, while_op.span())?;

    // The builder populates both regions of the new op with an entry block whose arguments match
    // the iteration arguments (before) and the results (after), so the original blocks are merged
    // into them. Only `BlockRef`s and `ValueRef`s are kept from here on: an `EntityRef` of a
    // region or block, such as the temporary produced by `before()`/`after()` inside a rewriter
    // call's argument list, must not be alive while the rewriter moves or erases blocks of that
    // region, or it fails with an aliasing violation (#1419).
    let (new_before_block, new_after_block, new_results) = {
        let new_while = new_while.borrow();
        (
            new_while.before().entry_block_ref().unwrap(),
            new_while.after().entry_block_ref().unwrap(),
            new_while
                .results()
                .all()
                .into_iter()
                .map(|r| *r as ValueRef)
                .collect::<SmallVec<[_; 4]>>(),
        )
    };

    // The replacement of each before block argument: the next argument of the new before block
    // for a kept iteration argument, the value chosen by the caller for a removed one.
    let before_args = {
        let new_before_block = new_before_block.borrow();
        let mut new_args = new_before_block.arguments().iter();
        iter_args
            .iter()
            .map(|iter_arg| match iter_arg {
                IterArg::Keep => Some(
                    *new_args.next().expect("missing argument in new before block") as ValueRef,
                ),
                IterArg::Remove(replacement) => *replacement,
            })
            .collect::<SmallVec<[Option<ValueRef>; 4]>>()
    };
    // The replacement of each after block argument and of each result of the original loop.
    let after_args = {
        let new_after_block = new_after_block.borrow();
        results
            .iter()
            .map(|result| result.map(|index| new_after_block.arguments()[index] as ValueRef))
            .collect::<SmallVec<[Option<ValueRef>; 4]>>()
    };
    let result_values = results
        .iter()
        .map(|result| result.map(|index| new_results[index]))
        .collect::<SmallVec<[Option<ValueRef>; 4]>>();

    // Narrow the terminators of the original regions in place.
    if new_yielded.len() != yielded.len() {
        let yield_ref = yield_op.as_operation_ref();
        let _guard = rewriter.modify_op_in_place(yield_ref);
        let mut yield_op = yield_op.borrow_mut();
        let context = yield_op.as_operation().context_rc();
        yield_op.yielded_mut().set_operands(new_yielded, yield_ref, &context);
    }
    if new_forwarded != forwarded {
        let cond_ref = cond_op.as_operation_ref();
        let _guard = rewriter.modify_op_in_place(cond_ref);
        let mut cond_op = cond_op.borrow_mut();
        let context = cond_op.as_operation().context_rc();
        cond_op.forwarded_mut().set_operands(new_forwarded, cond_ref, &context);
    }

    // The values that are dropped without a replacement.
    let dropped = {
        let before_block = before_block.borrow();
        let after_block = after_block.borrow();
        let dropped_before_args = iter_args
            .iter()
            .zip(before_block.arguments().iter())
            .filter(|(iter_arg, _)| matches!(iter_arg, IterArg::Remove(None)))
            .map(|(_, arg)| *arg as ValueRef);
        let dropped_after_args_and_results = results
            .iter()
            .zip(after_block.arguments().iter().zip(while_op.results().iter()))
            .filter(|(result, _)| result.is_none())
            .flat_map(|(_, (arg, result))| [*arg as ValueRef, *result as ValueRef]);
        dropped_before_args
            .chain(dropped_after_args_and_results)
            .collect::<SmallVec<[ValueRef; 4]>>()
    };

    // The original op is borrowed mutably by `replace_op_with_values` below.
    drop(op);

    // A dropped value has no real uses, but debug info may still describe a variable through it.
    for value in dropped.iter() {
        debug_assert!(!value.borrow().has_real_uses(), "cannot drop a value with real uses");
        erase_debug_info_with(value, rewriter);
    }

    rewriter.merge_blocks(before_block, new_before_block, &before_args);
    rewriter.merge_blocks(after_block, new_after_block, &after_args);
    rewriter.replace_op_with_values(operation, &result_values);

    Ok(true)
}
