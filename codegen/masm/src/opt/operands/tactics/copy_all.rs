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

        // We generally want to copy operands right-to-left (i.e. bottom-up in the expected stack),
        // so that the order of the materialized copies matches the expected order without needing
        // additional stack manipulation.
        //
        // However, when arity is large, naively duplicating right-to-left can push the remaining
        // source operands past the top-16 addressable window (as we are inserting copies on the
        // top of the stack). When that happens, we switch to a two-phase strategy:
        //
        // 1. Copy all expected operands in deepest-first order, so each `dup` stays within the
        //    top-16 accessible region.
        // 2. Reorder the copied operands on the top of the stack to match the expected order.
        //
        // This keeps the tactic applicable even for large copy-only callsites, and avoids emitting
        // stack operations which are invalid on the VM.
        let mut used_fallback = false;
        for index in (0..(arity as u8)).rev() {
            let expected_value = builder.unwrap_expected(index);
            let current_position =
                builder.get_current_position(&expected_value.unaliased()).unwrap_or_else(|| {
                    panic!(
                        "expected {:?} on the stack, but it was not found",
                        expected_value.unaliased()
                    )
                });
            if current_position >= 16 {
                used_fallback = true;
                break;
            }

            log::trace!(
                "copying {expected_value:?} at index {index} to top of stack, shifting {:?} down \
                 one",
                builder.unwrap_current(0)
            );
            builder.dup(current_position, expected_value.unwrap_alias());
        }

        if !used_fallback {
            return Ok(());
        }

        builder.discard();

        // Phase 1: Duplicate expected operands in deepest-first order
        let mut expected_values = (0..(arity as u8))
            .map(|index| builder.unwrap_expected(index))
            .collect::<Vec<_>>();
        expected_values.sort_by_key(|expected_value| {
            let current_position =
                builder.get_current_position(&expected_value.unaliased()).unwrap_or_else(|| {
                    panic!(
                        "expected {:?} on the stack, but it was not found",
                        expected_value.unaliased()
                    )
                });
            core::cmp::Reverse(current_position)
        });
        for expected_value in expected_values.into_iter() {
            let current_position =
                builder.get_current_position(&expected_value.unaliased()).unwrap_or_else(|| {
                    panic!(
                        "expected {:?} on the stack, but it was not found",
                        expected_value.unaliased()
                    )
                });

            // If we cannot address the source operand, this tactic can't help (we need to spill).
            if current_position >= 16 {
                return Err(TacticError::NotApplicable);
            }

            builder.dup(current_position, expected_value.unwrap_alias());
        }

        // Phase 2: Reorder the copies on the top of the stack to match the expected order
        for target_index in (0..(arity as u8)).rev() {
            let expected_value = builder.unwrap_expected(target_index);
            let current_position =
                builder.get_current_position(&expected_value).unwrap_or_else(|| {
                    panic!("expected {expected_value:?} on the stack, but it was not found")
                });
            if current_position != 0 {
                builder.movup(current_position);
            }
            if target_index != 0 {
                builder.movdn(target_index);
            }
        }

        Ok(())
    }
}
