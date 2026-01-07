use super::*;

/// This tactic constructs a strict solution by placing expected operands left-to-right (top-down),
/// materializing copies as needed, and inserting each operand into its final position immediately.
///
/// This is intended as a safe fallback for large callsites with many copy constraints, where other
/// tactics may struggle to find a solution within the top-16 addressable window.
#[derive(Default)]
pub struct PlaceAll;
impl Tactic for PlaceAll {
    fn cost(&self, context: &SolverContext) -> usize {
        core::cmp::max(context.copies().len(), 1)
    }

    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult {
        let arity = builder.arity();
        if arity == 0 {
            return Ok(());
        }

        // This tactic does not currently implement spilling, so it cannot place more than 16
        // operands (the maximum depth directly addressable by stack instructions).
        if arity > 16 {
            return Err(TacticError::NotApplicable);
        }

        // We build the expected stack prefix one operand at a time. At each step `index`, the
        // prefix `[0..index)` is already correct.
        for index in 0..(arity as u8) {
            let expected_value = builder.unwrap_expected(index);

            if expected_value.is_alias() {
                // Materialize the required copy if it doesn't exist yet.
                if builder.get_current_position(&expected_value).is_none() {
                    let current_position =
                        builder.unwrap_current_position(&expected_value.unaliased());
                    if current_position >= 16 {
                        return Err(TacticError::NotApplicable);
                    }
                    builder.dup(current_position, expected_value.unwrap_alias());
                } else {
                    // The copy is already present, bring it to the top.
                    let current_position = builder.unwrap_current_position(&expected_value);
                    if current_position >= 16 {
                        return Err(TacticError::NotApplicable);
                    }
                    if current_position != 0 {
                        builder.movup(current_position);
                    }
                }
            } else {
                // Move the original value into place.
                let current_position = builder.unwrap_current_position(&expected_value);
                if current_position >= 16 {
                    return Err(TacticError::NotApplicable);
                }
                if current_position != 0 {
                    builder.movup(current_position);
                }
            }

            // Insert the operand on top into its final position, restoring the already-built
            // prefix.
            if index != 0 {
                builder.movdn(index);
            }
        }

        Ok(())
    }
}
