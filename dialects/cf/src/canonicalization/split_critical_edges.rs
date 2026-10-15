use alloc::rc::Rc;
use core::any::TypeId;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    traits::BranchOpInterface,
    *,
};

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
    #[allow(unused)]
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

    pub fn for_op(context: Rc<Context>, op: OperationName) -> Self {
        Self {
            info: PatternInfo::new(
                context,
                "split-critical-edges",
                PatternKind::Operation(op),
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
    fn match_and_rewrite(
        &self,
        mut operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        // The critical edges are collected under a short borrow: no borrow of `operation` may be
        // alive while the rewriter runs below, since its listeners may inspect the op.
        let (critical_edges, span) = {
            let op = operation.borrow();
            let Some(br_op) = op.as_trait::<dyn BranchOpInterface>() else {
                return Ok(false);
            };

            if br_op.num_successors() < 2 {
                return Ok(false);
            }

            let mut critical_edges = SmallVec::<[_; 4]>::default();
            for succ in br_op.successors().all() {
                let successor = succ.successor();
                if successor.borrow().get_single_predecessor().is_none() {
                    critical_edges.push((successor, succ.index()));
                }
            }
            (critical_edges, op.span())
        };

        if critical_edges.is_empty() {
            return Ok(false);
        }

        // For each critical edge, introduce a new block with an unconditional branch to the target
        // block, moving successor operands from the original op to the new unconditional branch
        for (successor, successor_index) in critical_edges {
            // Remove successor operands from the branch, and take its block operand for rewiring
            let (operands, mut block_operand) = {
                let mut op = operation.borrow_mut();
                let br_op = op
                    .as_trait_mut::<dyn BranchOpInterface>()
                    .expect("the op matched as a branch above");
                let mut succ_operands = br_op.get_successor_operands_mut(successor_index);
                let operands = succ_operands
                    .forwarded()
                    .iter()
                    .map(|o| o.borrow().as_value_ref())
                    .collect::<SmallVec<[_; 4]>>();
                succ_operands.forwarded_mut().clear();
                (operands, br_op.successors_mut()[successor_index].block)
            };

            // Create new empty block, and insert an unconditional branch to `successor` with the
            // original operands of the branch.
            let mut guard = InsertionGuard::new(rewriter);
            let mut new_block = guard.create_block_before(successor, &[]);
            guard.br(successor, operands, span)?;

            // Rewrite successor block operand
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

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format};

    use midenc_expect_test::expect;
    use midenc_hir::testing::parse_function_fixpoint;

    use super::*;
    use crate::canonicalization::testing::apply_with_borrowing_listener;

    /// The critical edge of a `cf.cond_br` into a block with another predecessor is split while a
    /// listener borrows the ops around every change the pattern makes.
    #[test]
    fn split_critical_edges_with_a_borrowing_listener() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @split(%c: i1, %a: u32) -> u32 {
    cf.cond_br %c ^join(%a : u32), ^other : (i1);
^join(%x: u32):
    builtin.ret %x : (u32);
^other:
    cf.br ^join(%a : u32);
};";
        let (function, _) = parse_function_fixpoint(&context, "split_critical_edges.hir", source)?;

        let pattern: Box<dyn RewritePattern> = Box::new(SplitCriticalEdges::new(context.clone()));
        let changed = apply_with_borrowing_listener(&context, function, pattern)?;
        assert!(changed, "expected the critical edge to be split");

        let printed = format!("{}", function.as_operation_ref().borrow());
        expect![[r#"
            builtin.function public extern("C") @split(%0: i1, %1: u32) -> u32 {
                cf.cond_br %0 ^block4, ^block3 : (i1);
            ^block4:
                cf.br ^block2(%1 : u32);
            ^block2(%2: u32):
                builtin.ret %2 : (u32);
            ^block3:
                cf.br ^block2(%1 : u32);
            };"#]]
        .assert_eq(&printed);

        Ok(())
    }
}
