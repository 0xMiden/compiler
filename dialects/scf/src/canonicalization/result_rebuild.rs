//! Rebuilding an [If] or [IndexSwitch] operation with fewer results.
//!
//! The canonicalization patterns that drop unused results, and the transforms that move results
//! out of an op, all replace the op with a new one built from its regions, which keeps the yields
//! of every region consistent with the results. [rebuild_if] and [rebuild_index_switch] are the
//! single implementation of that rewrite for each op.

use alloc::vec::Vec;

use midenc_hir::{patterns::Rewriter, *};

use crate::*;

/// Replaces `operation`, an `scf.if`, with a new `scf.if` that has a subset of its results, and
/// moves the body of both regions into the new op.
///
/// `results` has one entry per result of the original op: the index of the result of the new op
/// that replaces it, or `None` if the result is dropped without replacement, which requires that
/// it has no real uses (the `di.debug_value` operations using it are erased with the op). Every
/// result of the new op must replace at least one result of the original one; when it replaces
/// several, every region must yield them the same value.
///
/// Returns `Ok(false)`, with nothing modified, if `operation` is not an `scf.if`.
pub fn rebuild_if(
    rewriter: &mut dyn Rewriter,
    operation: OperationRef,
    results: &[Option<usize>],
) -> Result<bool, Report> {
    let (condition, result_types, old_entries, dropped, span) = {
        let op = operation.borrow();
        let Some(if_op) = op.downcast_ref::<If>() else {
            return Ok(false);
        };
        (
            if_op.condition().as_value_ref(),
            new_result_types(&op, results),
            alloc::vec![
                if_op.then_body().entry_block_ref().unwrap(),
                if_op.else_body().entry_block_ref().unwrap(),
            ],
            dropped_results(&op, results),
            if_op.span(),
        )
    };

    // Creating the new op is the only fallible step; nothing has been modified before it. It takes
    // the place of the original, wherever the rewriter was positioned by the caller.
    rewriter.set_insertion_point_before(operation);
    let new_if = rewriter.r#if(condition, &result_types, span)?;
    let new_regions = {
        let new_if = new_if.borrow();
        alloc::vec![new_if.then_body().as_region_ref(), new_if.else_body().as_region_ref()]
    };

    finish_rebuild(
        rewriter,
        operation,
        new_if.as_operation_ref(),
        old_entries,
        new_regions,
        results,
        dropped,
    );

    Ok(true)
}

/// Replaces `operation`, an `scf.index_switch`, with a new `scf.index_switch` that has a subset
/// of its results, and moves the body of every region into the new op.
///
/// `results` is interpreted as in [rebuild_if].
///
/// Returns `Ok(false)`, with nothing modified, if `operation` is not an `scf.index_switch`.
pub fn rebuild_index_switch(
    rewriter: &mut dyn Rewriter,
    operation: OperationRef,
    results: &[Option<usize>],
) -> Result<bool, Report> {
    let (selector, cases, result_types, old_entries, dropped, span) = {
        let op = operation.borrow();
        let Some(switch_op) = op.downcast_ref::<IndexSwitch>() else {
            return Ok(false);
        };
        (
            switch_op.selector().as_value_ref(),
            switch_op.cases().iter().copied().collect::<Vec<_>>(),
            new_result_types(&op, results),
            switch_op
                .regions()
                .iter()
                .map(|region| region.entry_block_ref().unwrap())
                .collect::<Vec<_>>(),
            dropped_results(&op, results),
            switch_op.span(),
        )
    };

    // Creating the new op is the only fallible step; nothing has been modified before it. It takes
    // the place of the original, wherever the rewriter was positioned by the caller.
    rewriter.set_insertion_point_before(operation);
    let new_switch = rewriter.index_switch(selector, cases, &result_types, span)?;
    let new_regions = {
        let new_switch = new_switch.borrow();
        new_switch
            .regions()
            .iter()
            .map(|region| region.as_region_ref())
            .collect::<Vec<_>>()
    };

    finish_rebuild(
        rewriter,
        operation,
        new_switch.as_operation_ref(),
        old_entries,
        new_regions,
        results,
        dropped,
    );

    Ok(true)
}

/// The types of the results of the new op: each one takes the type of the first result of the
/// original op that it replaces.
fn new_result_types(op: &Operation, results: &[Option<usize>]) -> SmallVec<[Type; 4]> {
    assert_eq!(results.len(), op.num_results(), "expected one entry per result");
    let num_new_results = results.iter().flatten().map(|index| index + 1).max().unwrap_or(0);
    (0..num_new_results)
        .map(|new_index| {
            let index = results
                .iter()
                .position(|result| *result == Some(new_index))
                .expect("every result of the new op must replace a result of the original op");
            op.results()[index].borrow().ty().clone()
        })
        .collect()
}

/// The results of the original op that are dropped without a replacement.
fn dropped_results(op: &Operation, results: &[Option<usize>]) -> SmallVec<[ValueRef; 4]> {
    results
        .iter()
        .zip(op.results().iter())
        .filter(|(result, _)| result.is_none())
        .map(|(_, result)| *result as ValueRef)
        .collect()
}

/// Moves every region body of the original op into the corresponding empty region of the new op,
/// narrows each `scf.yield` to the kept results, and replaces the original op.
fn finish_rebuild(
    rewriter: &mut dyn Rewriter,
    operation: OperationRef,
    new_op: OperationRef,
    old_entries: Vec<BlockRef>,
    new_regions: Vec<RegionRef>,
    results: &[Option<usize>],
    dropped: SmallVec<[ValueRef; 4]>,
) {
    let num_new_results = new_op.borrow().num_results();
    // The position of the original result (and yield operand) each new result takes its value
    // from.
    let sources = (0..num_new_results)
        .map(|new_index| results.iter().position(|result| *result == Some(new_index)).unwrap())
        .collect::<SmallVec<[usize; 4]>>();

    for (old_entry, new_region) in old_entries.into_iter().zip(new_regions) {
        let new_entry = rewriter.create_block(new_region, None, &[]);
        rewriter.merge_blocks(old_entry, new_entry, &[]);

        let yield_ref = new_entry.borrow().terminator().unwrap();
        let mut yield_op = yield_ref.try_downcast_op::<Yield>().expect("expected scf.yield");
        let yielded = {
            let yield_op = yield_op.borrow();
            sources
                .iter()
                .map(|index| yield_op.yielded()[*index].borrow().as_value_ref())
                .collect::<SmallVec<[ValueRef; 4]>>()
        };
        if yielded.len() != yield_op.borrow().yielded().len() {
            let _guard = rewriter.modify_op_in_place(yield_ref);
            let mut yield_op = yield_op.borrow_mut();
            let context = yield_op.as_operation().context_rc();
            yield_op.yielded_mut().set_operands(yielded, yield_ref, &context);
        }
    }

    // A dropped result has no real uses; the `di.debug_value` operations that may still describe
    // a variable through it are erased together with the op below.
    for value in dropped.iter() {
        debug_assert!(!value.borrow().has_real_uses(), "cannot drop a result with real uses");
    }

    let result_values = {
        let new_op = new_op.borrow();
        results
            .iter()
            .map(|result| result.map(|index| new_op.results()[index] as ValueRef))
            .collect::<SmallVec<[Option<ValueRef>; 4]>>()
    };
    rewriter.replace_op_with_values(operation, &result_values);
}
