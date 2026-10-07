use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use super::while_rebuild::{IterArg, rebuild_while};
use crate::*;

/// Remove duplicated [crate::ops::Condition] args in a [While] loop.
pub struct WhileRemoveDuplicatedResults {
    info: PatternInfo,
}

impl WhileRemoveDuplicatedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-remove-duplicated-results",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileRemoveDuplicatedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileRemoveDuplicatedResults {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        // Results that are forwarded the same value are all replaced with the first of them.
        let mut unique = SmallVec::<[ValueRef; 4]>::default();
        let mut results = SmallVec::<[Option<usize>; 4]>::default();
        for forwarded in while_op.condition_op().borrow().forwarded().iter() {
            let value = forwarded.borrow().as_value_ref();
            let index = unique.iter().position(|v| *v == value).unwrap_or(unique.len());
            if index == unique.len() {
                unique.push(value);
            }
            results.push(Some(index));
        }
        if unique.len() == results.len() {
            return Ok(false);
        }
        let iter_args = SmallVec::<[_; 4]>::from_elem(IterArg::Keep, while_op.inits().len());
        drop(op);

        rebuild_while(rewriter, operation, &iter_args, &results)
    }
}
