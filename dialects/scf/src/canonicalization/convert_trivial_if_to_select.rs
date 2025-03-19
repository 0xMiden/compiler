use alloc::rc::Rc;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    adt::SmallDenseMap,
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Hoist any yielded results whose operands are defined outside an [If], to a [Select] instruction.
pub struct ConvertTrivialIfToSelect {
    info: PatternInfo,
}

impl ConvertTrivialIfToSelect {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let if_op = scf_dialect.registered_name::<If>().expect("scf.if is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "convert-trivial-if-to-select",
                PatternKind::Operation(if_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for ConvertTrivialIfToSelect {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for ConvertTrivialIfToSelect {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let num_results = op.num_results();
        if num_results == 0 {
            return Ok(false);
        }

        let Some(if_op) = op.downcast_ref::<If>() else {
            return Ok(false);
        };
        let if_op_ref = unsafe { UnsafeIntrusiveEntityRef::from_raw(if_op) };

        let span = if_op.span();
        let cond = if_op.condition().as_value_ref();
        let then_region = if_op.then_body().as_region_ref();
        let else_region = if_op.else_body().as_region_ref();
        let then_yield = if_op.then_yield();
        let else_yield = if_op.else_yield();
        let then_yield_args = then_yield
            .borrow()
            .yielded()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let else_yield_args = else_yield
            .borrow()
            .yielded()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        drop(op);

        let mut non_hoistable = SmallVec::<[_; 4]>::default();
        for (true_value, false_value) in
            then_yield_args.iter().copied().zip(else_yield_args.iter().copied())
        {
            let true_value = true_value.borrow();
            if true_value.parent_region().unwrap() == then_region
                || false_value.borrow().parent_region().unwrap() == else_region
            {
                non_hoistable.push(true_value.ty().clone());
            }
        }

        // Early exit if there aren't any yielded results we can hoist
        if non_hoistable.len() == num_results {
            return Ok(false);
        }

        // Create a new `scf.if` for the non-hoistable results, if there are any.
        //
        // Then, use either the new `scf.if`, or the original, as the anchor for inserting hoisted
        // `hir.select`s.
        let anchor = if !non_hoistable.is_empty() {
            // Create a new `scf.if` with the non-hoistable results
            let mut new_if = rewriter.r#if(cond, &non_hoistable, span)?;
            let mut new_if_op = new_if.borrow_mut();
            new_if_op.then_body_mut().take_body(then_region);
            new_if_op.else_body_mut().take_body(else_region);
            new_if
        } else {
            // We can hoist everything from the original `scf.if`, so we do not need to create a
            // new one, we can simply insert all of the selects and then erase the scf.if
            if_op_ref
        };

        // Insert `scf.select` ops for each hoisted result
        let mut results = SmallVec::<[_; 4]>::with_capacity(num_results);
        assert_eq!(then_yield.borrow().num_operands(), num_results);
        assert_eq!(else_yield.borrow().num_operands(), num_results);
        let mut true_yields = SmallVec::<[ValueRef; 4]>::default();
        let mut false_yields = SmallVec::<[ValueRef; 4]>::default();
        let mut deduplicated_selections =
            SmallDenseMap::<(ValueRef, ValueRef), ValueRef, 4>::default();
        let anchor_op = anchor.borrow();
        let new_then_region = anchor_op.then_body().as_region_ref();
        let new_else_region = anchor_op.else_body().as_region_ref();
        rewriter.set_insertion_point_before(anchor.as_operation_ref());
        for (true_value, false_value) in
            then_yield_args.iter().copied().zip(else_yield_args.iter().copied())
        {
            let true_parent_region = true_value.borrow().parent_region().unwrap();
            let false_parent_region = false_value.borrow().parent_region().unwrap();
            if new_then_region == true_parent_region || new_else_region == false_parent_region {
                results.push(Some(anchor_op.results()[true_yields.len()] as ValueRef));
                true_yields.push(true_value);
                false_yields.push(false_value);
            } else if true_value == false_value {
                results.push(Some(true_value));
            } else if let Some(duplicate) = deduplicated_selections.get(&(true_value, false_value))
            {
                results.push(Some(*duplicate));
            } else {
                let selected = rewriter.select(cond, true_value, false_value, span)?;
                results.push(Some(selected));
                deduplicated_selections.insert((true_value, false_value), selected);
            }
        }

        // If we have non-hoistable values, rewrite the `scf.yield` ops in the new `scf.if`
        if !non_hoistable.is_empty() {
            let new_then_yield = anchor_op.then_yield();
            let new_else_yield = anchor_op.else_yield();

            rewriter
                .set_insertion_point_to_end(new_then_region.borrow().entry_block_ref().unwrap());
            let replacement_then_yield = rewriter.r#yield(true_yields, span)?.as_operation_ref();
            rewriter.replace_op(new_then_yield.as_operation_ref(), replacement_then_yield);

            rewriter
                .set_insertion_point_to_end(new_else_region.borrow().entry_block_ref().unwrap());
            let replacement_else_yield = rewriter.r#yield(false_yields, span)?;
            rewriter.replace_op(
                new_else_yield.as_operation_ref(),
                replacement_else_yield.as_operation_ref(),
            );
        }
        drop(anchor_op);

        rewriter.replace_op_with_values(operation, &results);

        Ok(true)
    }
}
