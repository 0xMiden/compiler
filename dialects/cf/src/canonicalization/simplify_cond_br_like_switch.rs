use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplify a 'cf.switch' that is being used like a 'cf.cond_br', by converting the former into
/// the latter predicated on a single equality check.
pub struct SimplifyCondBrLikeSwitch {
    info: PatternInfo,
}

impl SimplifyCondBrLikeSwitch {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let switch_op =
            cf_dialect.registered_name::<Switch>().expect("cf.switch is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-cond-br-like-switch",
                PatternKind::Operation(switch_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyCondBrLikeSwitch {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyCondBrLikeSwitch {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(switch_op) = op.downcast_ref::<Switch>() else {
            return Ok(false);
        };

        // Check if the switch is cond_br like
        if switch_op.num_successors() != 2 {
            return Ok(false);
        }

        // Get the conditional value we need to compare for equality
        let cases = switch_op.cases();
        let if_true_case = cases.get(0).unwrap();
        let else_case = switch_op.fallback();

        // Materialize comparison
        let selector = switch_op.selector().as_value_ref();
        let expected_value = rewriter.u32(*if_true_case.key().unwrap(), switch_op.span());
        let is_true = rewriter.eq(selector, expected_value, switch_op.span())?;

        // Rewrite as cf.cond_br
        let new_op = rewriter.cond_br(
            is_true,
            if_true_case.block(),
            ValueRange::<2>::from(if_true_case.arguments().as_slice()),
            else_case.successor(),
            ValueRange::<2>::from(else_case.arguments),
            switch_op.span(),
        )?;

        drop(op);

        rewriter.replace_op(operation, new_op.as_operation_ref());

        Ok(true)
    }
}
