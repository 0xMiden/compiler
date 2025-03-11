use alloc::rc::Rc;

use midenc_hir2::*;

use super::simplify_passthrough_br::collapse_branch;
use crate::*;

/// Simplify conditional branches to a block from that block's sole predecessor, so long as doing
/// so does not introduce a critical edge in the control flow graph. A critical edge is a control
/// flow edge from a block with multiple successors to a block with multiple predecessors.
///
/// # Example
///
/// ```text,ignore
///   cf.cond_br %cond, ^bb1, ^bb2
/// ^bb1
///   br ^bbN(...)
/// ^bb2
///   br ^bbK(...)
/// ```
///
/// Becomes:
///
/// ```text,ignore
///  cf.cond_br %cond, ^bbN(...), ^bbK(...)
/// ```
pub struct SimplifyPassthroughCondBr {
    info: PatternInfo,
}

impl SimplifyPassthroughCondBr {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let cond_br_op =
            cf_dialect.registered_name::<CondBr>().expect("cf.cond_br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-passthrough-cond-br",
                PatternKind::Operation(cond_br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyPassthroughCondBr {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyPassthroughCondBr {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(cond_br_op) = op.downcast_ref::<CondBr>() else {
            return Ok(false);
        };

        let true_dest = cond_br_op.then_dest();
        let mut true_dest_operands = true_dest
            .arguments
            .iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let true_dest = true_dest.successor();
        let false_dest = cond_br_op.else_dest();
        let mut false_dest_operands = false_dest
            .arguments
            .iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let false_dest = false_dest.successor();

        // Try to collapse one of the current successors.
        let new_true_dest = collapse_branch(operation, true_dest, &mut true_dest_operands);
        let new_false_dest = collapse_branch(operation, false_dest, &mut false_dest_operands);
        if new_true_dest.is_none() && new_false_dest.is_none() {
            return Ok(false);
        }
        let new_true_dest = new_true_dest.unwrap_or(true_dest);
        let new_false_dest = new_false_dest.unwrap_or(false_dest);

        // Create a new branch with the collapsed successors.
        let span = cond_br_op.span();
        let cond = cond_br_op.condition().as_value_ref();
        drop(op);
        let new_cond_br = rewriter.cond_br(
            cond,
            new_true_dest,
            true_dest_operands,
            new_false_dest,
            false_dest_operands,
            span,
        )?;
        rewriter.replace_op(operation, new_cond_br.as_operation_ref());

        Ok(true)
    }
}
