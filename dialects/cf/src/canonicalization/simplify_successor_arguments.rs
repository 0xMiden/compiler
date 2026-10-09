use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

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
        // Both destinations are read under a short borrow: no borrow of `operation` may be alive
        // while the rewriter runs below, since its listeners may inspect the op.
        let (then_dest, else_dest) = {
            let op = operation.borrow();
            let Some(br_op) = op.downcast_ref::<CondBr>() else {
                return Ok(false);
            };
            (br_op.successors()[0], br_op.successors()[1])
        };
        let parent = operation.parent().unwrap();

        let mut changed = false;
        for target in [then_dest, else_dest] {
            // Check that the successor block has a single predecessor.
            let mut succ = target.successor();
            if succ == parent || succ.borrow().get_single_predecessor().is_none() {
                continue;
            }

            // Pair each successor block argument with the corresponding successor operand
            let operand_group = target.successor_operand_group();
            let replacements = {
                let succ_block = succ.borrow();
                // If there are no arguments, there is nothing to do for this successor
                if !succ_block.has_arguments() {
                    continue;
                }
                let op = operation.borrow();
                succ_block
                    .arguments()
                    .as_value_range()
                    .into_iter()
                    .zip(op.operands().group(operand_group).as_value_range())
                    .collect::<SmallVec<[_; 4]>>()
            };

            // Rewrite uses of the successor block arguments with the successor operands
            for (block_arg, operand) in replacements {
                rewriter.replace_all_uses_of_value_with(block_arg, operand);
            }

            // Remove the dead successor block arguments
            succ.borrow_mut().erase_arguments(|_| true);

            // Remove the now-unnecessary successor operands
            operation.borrow_mut().operands_mut().group_mut(operand_group).clear();

            changed = true;
        }

        if changed {
            // We modified the operation in-place, so notify any attached listeners
            rewriter.notify_operation_modified(operation);
        }

        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format};

    use midenc_expect_test::expect;
    use midenc_hir::testing::parse_function_fixpoint;

    use super::*;
    use crate::canonicalization::testing::apply_with_borrowing_listener;

    /// Both destinations of a `cf.cond_br` lose their arguments when each has the branch as its
    /// single predecessor, while a listener borrows the ops around every change the pattern makes.
    #[test]
    fn remove_unused_single_pred_block_args_with_a_borrowing_listener() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @args(%c: i1, %a: u32, %b: u32) -> u32 {
    cf.cond_br %c ^then(%a : u32), ^else(%b : u32) : (i1);
^then(%x: u32):
    builtin.ret %x : (u32);
^else(%y: u32):
    builtin.ret %y : (u32);
};";
        let (function, _) =
            parse_function_fixpoint(&context, "simplify_successor_arguments.hir", source)?;

        let pattern: Box<dyn RewritePattern> =
            Box::new(RemoveUnusedSinglePredBlockArgs::new(context.clone()));
        let changed = apply_with_borrowing_listener(&context, function, pattern)?;
        assert!(changed, "expected the successor arguments to be removed");

        let printed = format!("{}", function.as_operation_ref().borrow());
        expect![[r#"
            builtin.function public extern("C") @args(%0: i1, %1: u32, %2: u32) -> u32 {
                cf.cond_br %0 ^block2, ^block3 : (i1);
            ^block2:
                builtin.ret %1 : (u32);
            ^block3:
                builtin.ret %2 : (u32);
            };"#]]
        .assert_eq(&printed);

        Ok(())
    }
}
