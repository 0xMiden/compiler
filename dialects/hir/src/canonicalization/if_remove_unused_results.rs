use alloc::rc::Rc;

use midenc_hir2::*;

use crate::{
    builders::{DefaultInstBuilder, InstBuilder},
    ops::If,
    HirDialect, Yield,
};

/// Removed unused results of an [If] instruction
pub struct IfRemoveUnusedResults {
    info: PatternInfo,
}

impl IfRemoveUnusedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let hir_dialect = context.get_or_register_dialect::<HirDialect>();
        let if_op = hir_dialect.registered_name::<If>().expect("hir.if is not registered");
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
        let yield_op = dest.borrow().terminator().unwrap();
        let mut yield_op = unsafe {
            UnsafeIntrusiveEntityRef::from_raw(yield_op.borrow().downcast_ref::<Yield>().unwrap())
        };

        let yield_ = yield_op.borrow();
        let mut used_operands = SmallVec::<[ValueRef; 4]>::with_capacity(used_results.len());
        for used_result in used_results {
            let value = yield_.operands()[used_result.borrow().index()].borrow().as_value_ref();
            used_operands.push(value);
        }

        let yield_operation = yield_op.as_operation_ref();
        let _guard = rewriter.modify_op_in_place(yield_operation);
        yield_op.borrow_mut().yielded_mut().set_operands(used_operands, yield_operation);
    }
}

impl Pattern for IfRemoveUnusedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for IfRemoveUnusedResults {
    fn matches(&self, _op: OperationRef) -> Result<bool, Report> {
        panic!("call match_and_rewrite")
    }

    fn rewrite(&self, _op: OperationRef, _rewriter: &mut dyn Rewriter) {
        panic!("call match_and_rewrite")
    }

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
        let new_if = DefaultInstBuilder::new(rewriter).r#if(
            if_op.condition().as_value_ref(),
            &new_types,
            if_op.span(),
        )?;
        let new_if_op = new_if.borrow();

        let new_then_block =
            rewriter.create_block(new_if_op.then_body().as_region_ref(), None, &[]);
        let new_else_block =
            rewriter.create_block(new_if_op.else_body().as_region_ref(), None, &[]);

        // Move the bodies and replace the terminators (note there is a then and an else region
        // since the operation returns results).
        self.transfer_body(
            if_op.then_body().entry_block_ref().unwrap(),
            new_then_block,
            &used_results,
            rewriter,
        );
        self.transfer_body(
            if_op.else_body().entry_block_ref().unwrap(),
            new_else_block,
            &used_results,
            rewriter,
        );
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
