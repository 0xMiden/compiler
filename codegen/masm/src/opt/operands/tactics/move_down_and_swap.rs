use super::*;

/// This tactic is applied if moving the top value down the
/// stack presents an opportunity for a subsequent swap to
/// fix the order of the stack.
///
/// The criteria for this succeeding is:
///
/// 0. There must be no copies required, and at least two operands expected
/// 1. The value on top of the stack should either be evicted, or should be moved to its expected
///    position, and if applicable, past any remaining operands which belong before it in the final
///    ordering
/// 2. After moving the value on top, one of the following is true:
///   * The stack is now ordered, and we're done
///   * The operand on top of the stack is out of place, and the operand in its place is either:
///     1. Expected to be on top of the stack, so we can swap
///     2. Will be moved into place if we move the top of the stack immediately past it, so we can
///        move
///
/// If we apply these steps, and the stack ends up in the
/// desired order, then this tactic was successful.
#[derive(Default)]
pub struct MoveDownAndSwap;
impl Tactic for MoveDownAndSwap {
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

        // If the top operand is already in position, this tactic cannot be applied
        if builder.is_expected(0) {
            log::trace!("abandoning tactic: operand at index 0 is already in position");
            return Err(TacticError::NotApplicable);
        }

        let actual0 = builder.unwrap_current(0);
        if let Some(target_pos) = builder.get_expected_position(&actual0) {
            // Extend the target position past any operands which should
            // come before the operand currently on top of the stack
            log::trace!(
                "{actual0:?} expects to be at index {target_pos}, but is on top of the stack"
            );
            log::trace!(
                "looking for operands after index {target_pos} which need to come before \
                 {actual0:?} on the stack"
            );
            let target_offset = builder
                .stack()
                .iter()
                .rev()
                .skip(1 + target_pos as usize)
                .copied()
                .enumerate()
                .fold(0, |acc, (offset, operand)| {
                    builder
                        .get_expected_position(&operand)
                        .and_then(|operand_expected_at| {
                            if target_pos >= operand_expected_at {
                                Some(offset + 1)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(acc)
                }) as u8;
            // Move the operand to the position we've identified
            log::trace!(
                "moving {actual0:?} to {}, shifting {:?} to the top of stack",
                target_pos + target_offset,
                builder.unwrap_current(1)
            );
            builder.movdn(target_pos + target_offset);
        } else {
            let expected0 = builder.unwrap_expected(0);
            let expected0_at = builder.unwrap_current_position(&expected0);
            log::trace!(
                "{actual0:?} is not an expected operand, but is occupying index {expected0_at}, \
                 where we expect {expected0:?}, evicting.."
            );
            builder.evict();
        }

        // Is the item now on top of the stack misplaced?
        if builder.is_expected(0) {
            // If the item on top of the stack is in place, we cannot
            // succeed without introducing at least one extra move.
            //
            // Nevertheless, we return Ok here in case we actually
            // have a solution already, and also so that we can
            // potentially try combining this tactic with another
            // to find a solution
            log::trace!(
                "item on top of the stack is now in position, so we cannot proceed further, \
                 returning possible solution"
            );
            return Ok(());
        }

        // Where does it belong?
        let actual0 = builder.unwrap_current(0);
        if let Some(target_pos) = builder.get_expected_position(&actual0) {
            // Find the index where the operand at `target_pos` belongs
            let target_expected_pos =
                builder.get_expected_position(&builder.unwrap_current(target_pos));
            match target_expected_pos {
                Some(0) => {
                    // The target expects to be on top, so we can swap
                    log::trace!(
                        "{actual0:?} is expected at {target_pos}, the occupant of which is \
                         expected on top of the stack, swapping.."
                    );
                    builder.swap(target_pos);
                }
                Some(pos) if pos == target_pos - 1 => {
                    // The target would be moved into place if we move the top down
                    log::trace!(
                        "moving {actual0:?} to {target_pos}, the occupant of which is expected at \
                         {pos}"
                    );
                    builder.movdn(target_pos);
                }
                Some(_) | None => {
                    log::trace!("unable to apply tactic, operands do not match expected pattern");
                    return Err(TacticError::NotApplicable);
                }
            }
        } else {
            // We marked this operand for eviction, so moving
            // it past the end of the expected operands is all
            // that is needed here
            let expected0 = builder.unwrap_expected(0);
            let expected0_at = builder.unwrap_expected_position(&expected0);
            log::trace!(
                "{actual0:?} is not an expected operand, but is occupying index {expected0_at}, \
                 where we expect {expected0:?}, evicting.."
            );
            builder.evict();
        }

        Ok(())
    }
}
