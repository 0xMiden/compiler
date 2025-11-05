use super::*;

/// This tactic is applied if swapping a value to the top of the stack
/// presents an opportunity for subsequent move to fix the order of the stack.
///
/// The criteria for this succeeding is:
///
/// 0. There must be no copies required and at least two operands expected
/// 1. The value on top of the stack should either be:
///   * Evicted, and the value at the back of the constrained stack is an expected operand, thus
///     swapping them accomplishes both goals at once
///   * Swapped into its expected position, putting an operand at the top which is in position, can
///     be moved into position, or is to be evicted.
///
/// If after performing those steps, the stack is in the correct order,
/// the tactic was successful, otherwise the operand ordering cannot be
/// solved with this tactic (alone anyway).
#[derive(Default)]
pub struct SwapAndMoveUp;
impl Tactic for SwapAndMoveUp {
    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult {
        if builder.requires_copies() || builder.arity() < 2 {
            log::trace!(
                "cannot apply tactic when there are required copies ({}) or fewer than 2 operands \
                 ({})",
                builder.requires_copies(),
                builder.arity()
            );
            return Err(TacticError::PreconditionFailed);
        }

        // Find the operand that should be at index 1 and swap the top element
        // with it; then move up the value that should be at index 0
        let Some(expected1) = builder.get_expected(1) else {
            log::trace!("abandoning tactic because operand at index 1 is already in position");
            return Err(TacticError::NotApplicable);
        };
        let expected1_pos = builder.unwrap_current_position(&expected1);
        if expected1_pos == 0 {
            log::trace!(
                "swapping {expected1:?} from top of the stack, with {:?} at index 1",
                builder.stack()[1]
            );
            builder.swap(1);
        } else {
            log::trace!(
                "swapping {expected1:?} at index {expected1_pos} to the top of the stack, with \
                 {:?}",
                builder.stack()[0]
            );
            builder.swap(expected1_pos);
        }

        // Find the operand that should be at index 0 and move it into position
        let expected0 = builder.unwrap_expected(0);
        let expected0_pos = builder.unwrap_current_position(&expected0);
        if expected0_pos > 0 {
            log::trace!(
                "moving {expected0:?} from index {expected0_pos} to the top of stack, shifting \
                 {:?} down by one",
                builder.stack()[0]
            );
            builder.movup(expected0_pos);
        }

        Ok(())
    }
}
