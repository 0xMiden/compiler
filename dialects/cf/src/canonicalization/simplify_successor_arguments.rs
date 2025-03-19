use alloc::rc::Rc;

use midenc_hir2::*;

use crate::*;

/// Remove redundant successor arguments for conditional branches to a block with a single
/// predecessor.
///
/// This is only applied to `cf.cond_br`, because other canonicalization supercede this one for
/// `cf.br`.
pub struct RemoveUnusedSinglePredBlockArgs {
    info: PatternInfo,
}

impl RemoveUnusedSinglePredBlockArgs {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let br_op = cf_dialect.registered_name::<CondBr>().expect("cf.cond_br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "remove-unused-single-pred-block-args",
                PatternKind::Operation(br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for RemoveUnusedSinglePredBlockArgs {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for RemoveUnusedSinglePredBlockArgs {
    fn match_and_rewrite(
        &self,
        mut operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let mut op = operation.borrow_mut();
        let Some(br_op) = op.downcast_mut::<CondBr>() else {
            return Ok(false);
        };

        let then_dest = br_op.successors()[0];
        let else_dest = br_op.successors()[0];

        let mut changed = false;
        for target in [then_dest, else_dest] {
            // Check that the successor block has a single predecessor.
            let mut succ = target.successor();
            let parent = operation.parent().unwrap();
            if succ == parent || succ.borrow().get_single_predecessor().is_none() {
                continue;
            }

            // Rewrite uses of the successor block arguments with the corresponding successor
            // operands
            let succ_block = succ.borrow();
            // If there are no arguments, there is nothing to do for this successor
            if !succ_block.has_arguments() {
                continue;
            }

            for (block_arg, operand) in succ_block
                .arguments()
                .as_value_range()
                .into_iter()
                .zip(br_op.operands().group(target.successor_operand_group()).as_value_range())
            {
                rewriter.replace_all_uses_of_value_with(block_arg, operand);
            }
            drop(succ_block);

            // Remove the dead successor block arguments
            succ.borrow_mut().erase_arguments(|_| true);

            // Remove the now-unnecessary successor operands
            br_op.operands_mut().group_mut(target.successor_operand_group()).clear();

            changed = true;
        }

        drop(op);

        Ok(changed)
    }
}
