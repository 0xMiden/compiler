use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplify unconditional branches to a block from that block's sole predecessor
///
/// # Example
///
/// ```text,ignore
///   br ^bb1
/// ^bb1
///   br ^bbN(...)
/// ```
///
/// Becomes:
///
/// ```text,ignore
///   br ^bbN(...)
/// ```
pub struct SimplifyPassthroughBr {
    info: PatternInfo,
}

impl SimplifyPassthroughBr {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let br_op = cf_dialect.registered_name::<Br>().expect("cf.br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-passthrough-br",
                PatternKind::Operation(br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyPassthroughBr {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyPassthroughBr {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(br_op) = op.downcast_ref::<Br>() else {
            return Ok(false);
        };

        let successor = br_op.target();
        let dest = successor.successor();
        let mut dest_operands = successor
            .arguments
            .iter()
            .copied()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();

        // Try to collapse the successor if it points somewhere other than this block.
        if dest == op.parent().unwrap() {
            return Ok(false);
        }

        let span = op.span();
        drop(op);

        let Some(new_dest) = collapse_branch(operation, dest, &mut dest_operands) else {
            return Ok(false);
        };

        // Create a new branch with the collapsed successor.
        let new_br = rewriter.br(new_dest, dest_operands, span)?;
        rewriter.replace_op(operation, new_br.as_operation_ref());

        Ok(true)
    }
}

/// Given a successor, try to collapse it to a new destination if it only contains a passthrough
/// unconditional branch. If the successor is collapsable, the function returns `Ok` with the new
/// successor, and `successor_operands` is updated to reference the new destination and values.
/// `arg_storage` is used as storage if operands to the collapsed successor need to be remapped. It
/// must outlive uses of `successor_operands`.
pub fn collapse_branch(
    predecessor: OperationRef,
    successor: BlockRef,
    successor_operands: &mut SmallVec<[ValueRef; 4]>,
) -> Option<BlockRef> {
    // Check that the successor only contains a unconditional branch.
    let succ = successor.borrow();
    let terminator = succ.terminator()?;
    if succ.body().front().as_pointer() != Some(terminator) {
        return None;
    }

    // Check that the terminator is an unconditional branch.
    let terminator_op = terminator.borrow();
    let successor_br = terminator_op.downcast_ref::<Br>()?;

    // Check that the block arguments are only used by the terminator.
    for arg in succ.arguments().iter() {
        let arg = arg.borrow();
        for user in arg.iter_uses() {
            if user.owner != terminator {
                return None;
            }
        }
    }

    // Don't try to collapse branches to infinite loops.
    let target = successor_br.target();
    let successor_dest = target.successor();
    if successor_dest == successor {
        return None;
    }

    // Don't try to collapse branches when doing so would introduce a critical edge in the CFG
    //
    // A critical edge is an edge from a predecessor with multiple successors, to a successor with
    // multiple predecessors. It is necessary to break these edges with passthrough blocks that we
    // would otherwise wish to collapse during canonicalization. By avoiding introducing these edges
    // we can break any existing critical edges as a separate canonicalization, without the two
    // working against each other.
    if predecessor.borrow().num_successors() > 1
        && successor_dest.borrow().get_unique_predecessor().is_none()
    {
        return None;
    }

    // Update the operands to the successor. If the branch parent has no arguments, we can use the
    // branch operands directly.
    if !succ.has_arguments() {
        successor_operands.clear();
        successor_operands.extend(target.arguments.iter().map(|o| o.borrow().as_value_ref()));
        return Some(successor_dest);
    }

    // Otherwise, we need to remap any argument operands.
    let mut new_operands = SmallVec::default();
    for operand in target.arguments.iter() {
        let value = operand.borrow().as_value_ref();
        let operand = value.borrow();
        let block_arg = operand.downcast_ref::<BlockArgument>();
        match block_arg {
            Some(block_arg) if block_arg.owner() == successor => {
                new_operands.push(successor_operands[block_arg.index()]);
            }
            _ => {
                new_operands.push(value);
            }
        }
    }

    *successor_operands = new_operands;

    Some(successor_dest)
}
