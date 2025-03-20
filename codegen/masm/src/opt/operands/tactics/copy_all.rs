use super::*;

/// This tactic simply copies all expected operands right-to-left.
///
/// As a precondition, this tactic requires that all expected operands are copies.
#[derive(Default)]
pub struct CopyAll;
impl Tactic for CopyAll {
    fn cost(&self, context: &SolverContext) -> usize {
        core::cmp::max(context.copies().len(), 1)
    }

    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult {
        // We can't apply this tactic if any values should be moved
        let arity = builder.arity();
        if builder.num_copies() != arity {
            log::trace!(
                "expected all operands to require copying; but only {} out of {} operands are \
                 copied",
                builder.num_copies(),
                arity
            );
            return Err(TacticError::PreconditionFailed);
        }

        // Visit the expected operands in bottom-up order copying them as we go
        for index in (0..(arity as u8)).rev() {
            let expected_value = builder.unwrap_expected(index);
            let current_position =
                builder.get_current_position(&expected_value.unaliased()).unwrap_or_else(|| {
                    panic!(
                        "expected {:?} on the stack, but it was not found",
                        expected_value.unaliased()
                    )
                });
            // A copy already exists, so use it
            log::trace!(
                "copying {expected_value:?} at index {index} to top of stack, shifting {:?} down \
                 one",
                builder.unwrap_current(0)
            );
            builder.dup(current_position, expected_value.unwrap_alias());
        }

        Ok(())
    }
}
