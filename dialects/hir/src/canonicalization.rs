//! Canonicalization patterns for the `hir` dialect.

use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::{HirDialect, ops::Assertz};

/// Removes an `hir.assertz` whose operand was already asserted by an earlier `hir.assertz` in the
/// same block.
///
/// The operand is an SSA value, so once an assertion on it has passed, a later assertion on the
/// same value can never fail. The pattern only looks for an earlier assertion within the same
/// block rather than in dominating blocks, which covers the common case of several accesses
/// through one pointer.
pub struct RemoveRepeatedAssertz {
    info: PatternInfo,
}

impl RemoveRepeatedAssertz {
    /// Creates the pattern, registering the `hir` dialect in `context` if needed.
    pub fn new(context: Rc<Context>) -> Self {
        let assertz_op = context
            .get_or_register_dialect::<HirDialect>()
            .expect_registered_name::<Assertz>();
        Self {
            info: PatternInfo::new(
                context,
                "remove-repeated-assertz",
                PatternKind::Operation(assertz_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for RemoveRepeatedAssertz {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for RemoveRepeatedAssertz {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let (value, block) = {
            let op = operation.borrow();
            let Some(assertz) = op.downcast_ref::<Assertz>() else {
                return Ok(false);
            };
            let Some(block) = op.parent() else {
                return Ok(false);
            };
            (assertz.value().as_value_ref(), block)
        };

        let earlier_result = value.borrow().iter_uses().find_map(|user| {
            let owner = user.owner;
            if owner == operation {
                return None;
            }
            let op = owner.borrow();
            let earlier = op.downcast_ref::<Assertz>()?;
            let in_same_block = op.parent().is_some_and(|parent| parent == block);
            if in_same_block && op.is_before_in_block(&operation) {
                Some(earlier.result().as_value_ref())
            } else {
                None
            }
        });
        let Some(earlier_result) = earlier_result else {
            return Ok(false);
        };

        rewriter.replace_op_with_values_and_debug(
            operation,
            &[Some(earlier_result)],
            midenc_hir::dialects::debuginfo::DebugInfoUpdate::Equivalent,
        );
        Ok(true)
    }
}
