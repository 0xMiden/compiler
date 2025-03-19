use alloc::rc::Rc;

use midenc_hir::{
    dialects::builtin::{self, BuiltinOpBuilder},
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplify a branch to a block that contains only a `builtin.ret`.
///
/// This applies in cases where we cannot rely on [super::SimplifyBrToBlockWithSinglePred], or
/// [super::SimplifyPassthroughBr] to be applied because doing so would introduce critical edges.
/// The branch is redundant when we can simply lift the return instead.
///
/// This transformation is only safe (and only applied) when the successor:
///
/// 1. Only contains a `builtin.return`
/// 2. Either:
///    a. Has a single predecessor
///    b. The `builtin.return` returns no value, or returns a value whose definition is either a
///       block argument of the successor, or dominates the predecessor `cf.br`
pub struct SimplifyBrToReturn {
    info: PatternInfo,
}

impl SimplifyBrToReturn {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let br_op = cf_dialect.registered_name::<Br>().expect("cf.br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-br-to-return",
                PatternKind::Operation(br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyBrToReturn {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyBrToReturn {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(br_op) = op.downcast_ref::<Br>() else {
            return Ok(false);
        };

        let target = br_op.target();
        let succ = target.successor();
        let parent = op.parent().unwrap();

        // If the successor is the parent block of this `cf.br`, this transform does not apply
        if succ == parent {
            return Ok(false);
        }

        let successor = succ.borrow();
        let Some(terminator) = successor.terminator() else {
            return Ok(false);
        };

        // If there are more operations in the successor block than just the return, this transform
        // is not applied.
        if successor.body().front().as_pointer() != Some(terminator) {
            return Ok(false);
        }

        // Check if the successor block contains a single `builtin.return`
        let terminator_op = terminator.borrow();
        let terminator_ret_imm = terminator_op.downcast_ref::<builtin::RetImm>();
        let is_terminator_return =
            terminator_op.is::<builtin::Ret>() || terminator_ret_imm.is_some();
        if !is_terminator_return {
            return Ok(false);
        }

        // Determine if we're the sole predecessor of the successor block
        let is_sole_predecessor =
            successor.get_single_predecessor().is_some_and(|pred| pred == parent);

        // If we're the sole predecessor, we can merge the successor entirely
        if is_sole_predecessor {
            drop(successor);

            // Merge the successor into the current block and erase the branch.
            let operands = target
                .arguments
                .iter()
                .map(|o| Some(o.borrow().as_value_ref()))
                .collect::<SmallVec<[_; 4]>>();

            drop(op);
            rewriter.erase_op(operation);
            rewriter.merge_blocks(succ, parent, &operands);
            return Ok(true);
        }

        // Otherwise, we must replace the current `cf.br` with a `builtin.ret`/`builtin.ret_imm`
        // that is a copy of the successor return op. Any return values by definition must either be
        // a block argument of the successor, or dominating `op`, so we simply need to map the set
        // of return values to their appropriate values in the current block.
        //
        // In the case of `builtin.return_imm` nothing needs to be done about operands as it has
        // none.
        let returned = ValueRange::<2>::from(terminator_op.operands().all());
        let mut new_returned = SmallVec::<[_; 4]>::default();
        if terminator_ret_imm.is_none() {
            let successor_args = ValueRange::<2>::from(successor.arguments());

            for return_value in returned.iter() {
                // The return value is a block argument of the successor, track its replacement
                if let Some(index) = successor_args.iter().position(|arg| arg == return_value) {
                    new_returned.push(target.arguments[index].borrow().as_value_ref());
                } else {
                    new_returned.push(return_value);
                }
            }
        }
        drop(successor);

        // Create the new `builtin.(return|return_imm)`
        let new_op = if let Some(ret_imm) = terminator_ret_imm {
            rewriter.ret_imm(*ret_imm.value(), ret_imm.span())?.as_operation_ref()
        } else {
            rewriter.ret(new_returned, terminator_op.span())?.as_operation_ref()
        };

        // Replace `op` with the new return
        drop(op);
        rewriter.replace_op(operation, new_op);

        Ok(true)
    }
}
