use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    matchers::Matcher,
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplifies a `cf.select` between the boolean constants into the condition itself:
///
/// * `select(c, true, false)` becomes `c`
/// * `select(c, false, true)` becomes `not c`
///
/// Such selects are produced when control flow that merely forwards a flag is converted to
/// selects, and each one costs several VM ops where the condition is already at hand.
pub struct SimplifySelectOfBools {
    info: PatternInfo,
}

impl SimplifySelectOfBools {
    pub fn new(context: Rc<Context>) -> Self {
        let select_op = context
            .get_or_register_dialect::<ControlFlowDialect>()
            .expect_registered_name::<Select>();
        Self {
            info: PatternInfo::new(
                context,
                "simplify-select-of-bools",
                PatternKind::Operation(select_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifySelectOfBools {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifySelectOfBools {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let (span, cond, first, second) = {
            let op = operation.borrow();
            let Some(select) = op.downcast_ref::<Select>() else {
                return Ok(false);
            };
            if select.result().ty() != &Type::I1 {
                return Ok(false);
            }
            let constant_bool = |value: ValueRef| -> Option<bool> {
                let defining_op = value.borrow().get_defining_op()?;
                matchers::constant_of::<Immediate>()
                    .matches(&*defining_op.borrow())
                    .and_then(|imm| imm.as_bool())
            };
            let (Some(first), Some(second)) = (
                constant_bool(select.first().as_value_ref()),
                constant_bool(select.second().as_value_ref()),
            ) else {
                return Ok(false);
            };
            (op.span(), select.cond().as_value_ref(), first, second)
        };

        let replacement = match (first, second) {
            (true, false) => cond,
            (false, true) => {
                rewriter.set_insertion_point_before(operation);
                rewriter.not(cond, span)?
            }
            // Equal constant arms are folded by `Select::fold`.
            _ => return Ok(false),
        };
        rewriter.replace_op_with_values(operation, &[Some(replacement)]);
        Ok(true)
    }
}
