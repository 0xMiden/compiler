use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Removed unused results of an [If] instruction
pub struct IfRemoveUnusedResults {
    info: PatternInfo,
}

impl IfRemoveUnusedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let if_op = scf_dialect.registered_name::<If>().expect("scf.if is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "if-remove-unused-results",
                PatternKind::Operation(if_op),
                PatternBenefit::MAX,
            ),
        }
    }

    fn transfer_body(
        &self,
        src: BlockRef,
        dest: BlockRef,
        used_results: &[OpResultRef],
        rewriter: &mut dyn Rewriter,
    ) {
        // Move all operations to the destination block
        rewriter.merge_blocks(src, dest, &[]);

        // Replace the yield op with one that returns only the used values.
        let op = { dest.borrow().terminator().unwrap() };
        let mut yield_op = unsafe {
            UnsafeIntrusiveEntityRef::from_raw(op.borrow().downcast_ref::<Yield>().unwrap())
        };

        let mut used_operands = SmallVec::<[ValueRef; 4]>::with_capacity(used_results.len());
        {
            let yield_ = yield_op.borrow();
            for used_result in used_results {
                let operand = yield_.operands()[used_result.borrow().index()];
                used_operands.push(operand.borrow().as_value_ref());
            }
        }

        let _guard = rewriter.modify_op_in_place(op);
        let mut yield_ = yield_op.borrow_mut();
        let context = yield_.as_operation().context_rc();
        yield_.yielded_mut().set_operands(used_operands, op, &context);
    }
}

impl Pattern for IfRemoveUnusedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for IfRemoveUnusedResults {
    fn match_and_rewrite(
        &self,
        mut operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        // Compute the list of used results.
        let used_results = operation
            .borrow()
            .results()
            .iter()
            .copied()
            .filter(|result| result.borrow().is_used())
            .collect::<SmallVec<[_; 4]>>();

        // Replace the operation if only a subset of its results have uses.
        let num_results = operation.borrow().num_results();
        if used_results.len() == num_results {
            return Ok(false);
        }

        let mut op = operation.borrow_mut();
        let Some(if_op) = op.downcast_mut::<If>() else {
            return Ok(false);
        };

        // Compute the result types of the replacement operation.
        let new_types = used_results
            .iter()
            .map(|result| result.borrow().ty().clone())
            .collect::<SmallVec<[_; 4]>>();

        // Create a replacement operation with empty then and else regions.
        let new_if = rewriter.r#if(if_op.condition().as_value_ref(), &new_types, if_op.span())?;
        let new_if_op = new_if.borrow();

        let new_then_region = new_if_op.then_body().as_region_ref();
        let new_then_block = rewriter.create_block(new_then_region, None, &[]);
        let new_else_region = new_if_op.else_body().as_region_ref();
        let new_else_block = rewriter.create_block(new_else_region, None, &[]);

        // Move the bodies and replace the terminators (note there is a then and an else region
        // since the operation returns results).
        let then_entry = { if_op.then_body().entry_block_ref().unwrap() };
        self.transfer_body(then_entry, new_then_block, &used_results, rewriter);
        let else_entry = { if_op.else_body().entry_block_ref().unwrap() };
        self.transfer_body(else_entry, new_else_block, &used_results, rewriter);
        drop(op);

        // Replace the operation by the new one.
        let mut replaced_results = SmallVec::<[_; 4]>::with_capacity(num_results);
        replaced_results.resize(num_results, None);
        for (index, result) in used_results.into_iter().enumerate() {
            replaced_results[result.borrow().index()] =
                Some(new_if_op.results()[index] as ValueRef);
        }
        rewriter.replace_op_with_values(operation, &replaced_results);

        Ok(true)
    }
}
