use alloc::rc::Rc;

use midenc_hir2::*;

use crate::*;

/// Simplify a branch to a block that has a single predecessor. This effectively merges the two
/// blocks.
pub struct SimplifyBrToBlockWithSinglePred {
    info: PatternInfo,
}

impl SimplifyBrToBlockWithSinglePred {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let br_op = cf_dialect.registered_name::<Br>().expect("cf.br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-br-to-block-with-single-predecessor",
                PatternKind::Operation(br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyBrToBlockWithSinglePred {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyBrToBlockWithSinglePred {
    fn matches(&self, _op: OperationRef) -> Result<bool, Report> {
        panic!("call match_and_rewrite")
    }

    fn rewrite(&self, _op: OperationRef, _rewriter: &mut dyn Rewriter) {
        panic!("call match_and_rewrite")
    }

    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(br_op) = op.downcast_ref::<Br>() else {
            return Ok(false);
        };

        // Check that the successor block has a single predecessor.
        let target = br_op.target();
        let succ = target.successor();
        let parent = op.parent().unwrap();
        if succ == parent || succ.borrow().get_single_predecessor().is_none() {
            return Ok(false);
        }

        // Merge the successor into the current block and erase the branch.
        let operands = target
            .arguments
            .iter()
            .map(|o| Some(o.borrow().as_value_ref()))
            .collect::<SmallVec<[_; 4]>>();

        drop(op);
        rewriter.erase_op(operation);
        rewriter.merge_blocks(succ, parent, &operands);

        Ok(true)
    }
}
