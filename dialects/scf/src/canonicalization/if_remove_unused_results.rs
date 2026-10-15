use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use super::result_rebuild::rebuild_if;
use crate::*;

/// Removed unused results of an [If] instruction
pub struct IfRemoveUnusedResults {
    info: PatternInfo,
}

impl IfRemoveUnusedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let if_op = scf_dialect.registered_name::<If>().expect("scf.if is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "if-remove-unused-results",
                PatternKind::Operation(if_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for IfRemoveUnusedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for IfRemoveUnusedResults {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let results = {
            let op = operation.borrow();
            if !op.is::<If>() {
                return Ok(false);
            }
            let Some(results) = used_results(&op) else {
                return Ok(false);
            };
            results
        };

        rebuild_if(rewriter, operation, &results)
    }
}

/// The result map (see [rebuild_if]) that keeps the results of `op` with real uses and drops the
/// others, or `None` if every result is used.
pub(super) fn used_results(op: &Operation) -> Option<SmallVec<[Option<usize>; 4]>> {
    let mut num_kept = 0;
    let results = op
        .results()
        .iter()
        .map(|result| {
            if result.borrow().has_real_uses() {
                num_kept += 1;
                Some(num_kept - 1)
            } else {
                None
            }
        })
        .collect::<SmallVec<[Option<usize>; 4]>>();
    (num_kept < results.len()).then_some(results)
}
