use alloc::rc::Rc;
use core::any::TypeId;

use midenc_hir2::{traits::BranchOpInterface, *};

use crate::*;

/// Ensure that any critical edges in the control flow graph introduced by branch-like operations
/// with multiple successors, are broken, by introducing passthrough blocks.
///
/// NOTE: This does not conflict with the SimplifyPassthrough* canonicalization patterns, as those
/// are explicitly written to avoid introducing critical edges, and so will not undo any changes
/// performed by this pattern rewrite.
///
/// # Example
///
/// ```text,ignore
/// ^bb0:
///   cf.cond_br %c0, ^bb2(%v0), ^bb3
/// ^bb1:
///   cf.cond_br %c1, ^bb2(%v1), ^bb4
/// ^bb2(%arg)
///   ...
/// ```
///
/// Becomes:
///
/// ```text,ignore
/// ^bb0:
///   cf.cond_br %c0, ^bb5, ^bb3
/// ^bb1:
///   cf.cond_br %c1, ^bb6, ^bb4
/// ^bb2(%arg):
///   ...
/// ^bb5:
///   cf.br ^bb2(%v0)
/// ^bb6:
///   cf.br ^bb2(%v1)
/// ```
pub struct SplitCriticalEdges {
    info: PatternInfo,
}

impl SplitCriticalEdges {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            info: PatternInfo::new(
                context,
                "split-critical-edges",
                PatternKind::Trait(TypeId::of::<dyn BranchOpInterface>()),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SplitCriticalEdges {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SplitCriticalEdges {
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
        let mut op = operation.borrow_mut();
        let Some(br_op) = op.as_trait_mut::<dyn BranchOpInterface>() else {
            return Ok(false);
        };

        if br_op.num_successors() < 2 {
            return Ok(false);
        }

        let mut critical_edges = SmallVec::<[_; 4]>::default();
        for succ in br_op.successors().all() {
            let successor = succ.successor();
            if successor.borrow().get_unique_predecessor().is_none() {
                critical_edges.push((successor, succ.index()));
            }
        }

        if critical_edges.is_empty() {
            return Ok(false);
        }

        // For each critical edge, introduce a new block with an unconditional branch to the target
        // block, moving successor operands from the original op to the new unconditional branch
        for (successor, successor_index) in critical_edges {
            // Remove successor operands from `br_op`
            let operands = {
                let mut succ_operands = br_op.get_successor_operands_mut(successor_index);
                let operands = succ_operands
                    .forwarded()
                    .iter()
                    .map(|o| o.borrow().as_value_ref())
                    .collect::<SmallVec<[_; 4]>>();
                succ_operands.forwarded_mut().take();
                operands
            };

            // Create new empty block, and insert an unconditional branch to `successor` with the
            // original operands of `br_op`.
            let mut guard = InsertionGuard::new(rewriter);
            let mut new_block = guard.create_block_before(successor, &[]);
            guard.br(successor, operands, br_op.as_operation().span())?;

            // Rewrite successor block operand
            let mut block_operand = br_op.successors_mut()[successor_index].block;
            {
                let mut block_operand = block_operand.borrow_mut();
                block_operand.unlink();
            }
            new_block.borrow_mut().insert_use(block_operand);
        }

        // We modified the operation in-place, so notify any attached listeners
        rewriter.notify_operation_modified(operation);

        Ok(true)
    }
}
