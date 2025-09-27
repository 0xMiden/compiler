use super::*;

/// This tactic is applied if moving a value to the top of the stack
/// presents an opportunity for subsequent swap to fix the order of the
/// stack.
///
/// The criteria for this succeeding is:
///
/// 0. There must be no copies required and at least one operand expected
/// 1. The value expected on top of the stack should be one of:
///   * On top, but the next element expected behind it is off-by-one. In which case we search for
///     the element that goes there, move it up, and swap.
///   * Not on top, but in a cycle with another misplaced operand, such that moving the latter to
///     the top of the stack and swapping it with the expected top operand puts them both into place
///   * Not on top, but once moved to the top, is a valid solution already
///
/// If after performing those steps, the stack is in the correct order,
/// the tactic was successful, otherwise the operand ordering cannot be
/// solved with this tactic (alone anyway).
#[derive(Default)]
pub struct MoveUpAndSwap;
impl Tactic for MoveUpAndSwap {
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

        let expected0 = builder.unwrap_expected(0);
        let actual0 = builder.unwrap_current(0);
        if actual0 == expected0 {
            let Some(expected1) = builder.get_expected(1) else {
                log::trace!(
                    "top two operands on the stack are already in position, returning possible \
                     solution"
                );
                return Ok(());
            };
            let move_from = builder.unwrap_current_position(&expected1);
            if move_from == 1 {
                log::trace!("abandoning tactic because operand at index 1 is already in position");
                return Err(TacticError::NotApplicable);
            }
            log::trace!(
                "moving {expected1:?} to top of stack from index {move_from}, then swapping with \
                 {expected0:?}"
            );
            builder.movup(move_from);
            builder.swap(1);

            // The tactic was successfully applied, but it is
            // up to the solver to determine if a solution was
            // found
            log::trace!("returning possible solution");
            return Ok(());
        }

        // Handle cases such as the following:
        //
        // Assume the stack is in the following order:
        //
        //     [b, d, c, a, e]
        //
        // The movup + swap pattern can solve this as follows:
        //
        // 1. `a` is expected on top, but is down stack
        // 2. If we traverse the stack between the top and `a`, until we find a pair of operands
        //    where the expected positions of the pair are not in descending order, we will find
        //    that `d` and `c` are such a pair. If no such pair is found, we consider this tactic
        //    failed.
        //
        //    * NOTE: If the pair includes `a` itself, then we simply move `a` directly to the top.
        //
        // 3. If we move the larger of the two operands to the top of the stack, we will obtain the
        //    following order:
        //
        //    [d, b, c, a, e]
        //
        // 4. We then identify where `a` is on the stack, and swap with the top operand `d`, leaving
        //    us with:
        //
        //    [a, b, c, d, e]
        let mut descending_pair = None;
        let mut last_pos = None;
        for (operand_pos, operand) in builder.stack().iter().rev().enumerate() {
            if let Some(expected_pos) = builder.get_expected_position(operand) {
                let current = (operand_pos as u8, expected_pos);
                let last_operand_pos = last_pos.replace(current);
                if let Some(last @ (_, last_expected_pos)) = last_operand_pos {
                    if expected_pos >= last_expected_pos {
                        continue;
                    }
                    descending_pair = Some((last, current));
                    break;
                }
            }
        }

        // We found a pair of operands where the expected position of the two
        // operands is in descending order, e.g. `b` before `a`. We use those
        // names here to help keep track of which item is which, but keep in
        // mind that we aren't implying that the pair is expected to appear
        // consecutively, just that relative to one another they are out of order
        if let Some((b, a)) = descending_pair {
            let (b_actual, b_expected) = b;
            let (a_actual, a_expected) = a;
            debug_assert!(b_expected > a_expected);
            // If the pair includes `a` itself, then just move `a` to the top
            if a_expected == 0 {
                log::trace!(
                    "moving {:?} to the top of stack, shifting {:?} down",
                    builder.stack()[a_actual as usize],
                    builder.stack()[0]
                );
                builder.movup(a_actual);
            } else {
                if b_actual > 0 {
                    log::trace!(
                        "moving {:?} to the top of stack, shifting {:?} down",
                        builder.stack()[b_actual as usize],
                        builder.stack()[0]
                    );
                    builder.movup(b_actual);
                }
                let expected0_at = builder.unwrap_current_position(&expected0);
                log::trace!(
                    "moving {:?} to the top of stack, shifting {:?} down",
                    builder.stack()[expected0_at as usize],
                    builder.stack()[0]
                );
                builder.movup(expected0_at);
            }
            Ok(())
        } else {
            // If this branch is reached, it implies that all
            // of the operands on the stack are in order,
            // but there is at least one unused operand to be evicted
            // from the top of the stack, and possibly others that
            // are interspersed between expected operands. We
            // do not attempt to solve that in this tactic, and
            // instead defer to MoveDownAndSwap or fallback, both
            // of which focus on moving elements from the top first,
            // and handle the various patterns that might arise there.
            log::trace!(
                "abandoning tactic because by implication, operands are in order, and an unused \
                 operand must need eviction"
            );
            Err(TacticError::NotApplicable)
        }
    }
}
