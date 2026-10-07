use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use super::while_rebuild::{IterArg, rebuild_while};
use crate::*;

/// Remove unused init/yield args of a [While] loop.
pub struct WhileRemoveUnusedArgs {
    info: PatternInfo,
}

impl WhileRemoveUnusedArgs {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-remove-unused-args",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileRemoveUnusedArgs {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileRemoveUnusedArgs {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        // An iteration argument is removed when its before block argument has no real uses.
        let iter_args = while_op
            .before()
            .entry()
            .arguments()
            .iter()
            .map(|arg| {
                if arg.borrow().has_real_uses() {
                    IterArg::Keep
                } else {
                    IterArg::Remove(None)
                }
            })
            .collect::<SmallVec<[_; 4]>>();
        if !iter_args.contains(&IterArg::Remove(None)) {
            return Ok(false);
        }
        let results = (0..while_op.num_results()).map(Some).collect::<SmallVec<[_; 4]>>();
        drop(op);

        rebuild_while(rewriter, operation, &iter_args, &results)
    }
}
