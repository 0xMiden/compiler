use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir2::*;

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

        let condition_operation = while_op.condition_op();

        // These variables serve to prevent creating duplicate constants and hold constant true or
        // false values
        let mut constant_true = None;

        let mut replaced = false;

        let span = while_op.span();
        let condition_op = condition_operation.borrow();
        let condition = condition_op.condition().as_value_ref();

        let forwarded = condition_op.forwarded();
        let after_region = while_op.after();
        let after_block = after_region.entry();
        for (yielded, block_arg) in forwarded.iter().zip(after_block.arguments()) {
            let yielded = yielded.borrow().as_value_ref();
            if yielded == condition && block_arg.borrow().is_used() {
                let constant = *constant_true.get_or_insert_with(|| rewriter.i1(true, span));

                rewriter
                    .replace_all_uses_of_value_with(block_arg.borrow().as_value_ref(), constant);
                replaced = true;
            }
        }

        Ok(replaced)
    }
}
