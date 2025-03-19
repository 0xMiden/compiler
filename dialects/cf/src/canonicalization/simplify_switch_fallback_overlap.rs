use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplify a `cf.switch` with one or more cases that overlap with the fallback case, by removing
/// those cases entirely, and relying on the fallback to catch them.
///
/// This transformation only applies if the overlapping case destinations and arguments are
/// identical.
pub struct SimplifySwitchFallbackOverlap {
    info: PatternInfo,
}

impl SimplifySwitchFallbackOverlap {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let switch_op =
            cf_dialect.registered_name::<Switch>().expect("cf.switch is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-switch-fallback-overlap",
                PatternKind::Operation(switch_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifySwitchFallbackOverlap {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifySwitchFallbackOverlap {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(switch_op) = op.downcast_ref::<Switch>() else {
            return Ok(false);
        };

        log::trace!(target: "simplify-switch-fallback-overlap", "canonicalizing {op}");

        // Check if the switch has at least one non-default case that overlaps with the fallback
        let mut non_overlapping = SmallVec::<[_; 4]>::default();
        let default_target = switch_op.fallback();
        let mut has_overlapping = false;
        {
            let cases = switch_op.cases();
            for case in cases.iter() {
                let successor = case.block();
                if successor == default_target.successor() {
                    let identical_argv = ValueRange::<2>::from(case.arguments().as_slice())
                        .into_iter()
                        .eq(ValueRange::<2>::from(default_target.arguments.as_slice()));
                    if identical_argv {
                        has_overlapping = true;
                        continue;
                    }
                }

                non_overlapping.push(SwitchCase {
                    value: *case.key().unwrap(),
                    successor,
                    arguments: ValueRange::<4>::from(case.arguments().as_slice())
                        .into_smallvec()
                        .into_vec(),
                });
            }
        }

        if !has_overlapping {
            return Ok(false);
        }

        // Create a new switch op with the new case configuration
        let selector = switch_op.selector().as_value_ref();

        let new_op = rewriter.switch(
            selector,
            non_overlapping,
            default_target.successor(),
            ValueRange::<2>::from(default_target.arguments.as_slice()),
            switch_op.span(),
        )?;

        // Replace old op
        drop(op);
        rewriter.replace_op(operation, new_op.as_operation_ref());

        Ok(true)
    }
}
