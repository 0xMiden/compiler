use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Replace uses of the condition of a [While] operation within its do block with true, since
/// otherwise the block would not be evaluated.
///
/// Before:
///
/// ```text,ignore
/// scf.while (..) : (i1, ...) -> ... {
///    %condition = call @evaluate_condition() : () -> i1
///    scf.condition(%condition) %condition : i1, ...
/// } do {
/// ^bb0(%arg0: i1, ...):
///    use(%arg0)
///    ...
/// ```
///
/// After:
///
/// ```text,ignore
/// scf.while (..) : (i1, ...) -> ... {
///    %condition = call @evaluate_condition() : () -> i1
///    scf.condition(%condition) %condition : i1, ...
/// } do {
/// ^bb0(%arg0: i1, ...):
///    use(%true)
///    ...
/// ```
pub struct WhileConditionTruth {
    info: PatternInfo,
}

impl WhileConditionTruth {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-condition-truth",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileConditionTruth {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileConditionTruth {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = op.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        let span = while_op.span();
        let (condition, forwarded) = {
            let condition_op = while_op.condition_op();
            let condition_op = condition_op.borrow();
            (
                condition_op.condition().as_value_ref(),
                condition_op
                    .forwarded()
                    .iter()
                    .map(|v| v.borrow().as_value_ref())
                    .collect::<SmallVec<[ValueRef; 4]>>(),
            )
        };
        let after_args = while_op
            .after()
            .entry()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[ValueRef; 4]>>();
        // Only `ValueRef`s are kept from here on: a borrow of the loop, of its after block or of
        // a block argument must not be alive while the rewriter replaces the uses of that argument.
        drop(op);

        // Prevents creating duplicate constants
        let mut constant_true = None;
        let mut replaced = false;
        for (forwarded, after_arg) in forwarded.into_iter().zip(after_args) {
            if forwarded == condition && after_arg.borrow().is_used() {
                let constant = *constant_true.get_or_insert_with(|| rewriter.i1(true, span));
                rewriter.replace_all_uses_of_value_with(after_arg, constant);
                replaced = true;
            }
        }

        Ok(replaced)
    }
}
