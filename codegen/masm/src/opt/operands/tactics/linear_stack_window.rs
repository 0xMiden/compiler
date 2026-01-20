use super::*;
use crate::opt::operands::MASM_STACK_WINDOW_FELTS;

/// Returns the deepest addressable source index for materializing a copy of `value`.
///
/// MASM stack manipulation instructions can only directly access the first 16 field elements on
/// the operand stack. Since a single operand may consist of multiple field elements, we must
/// ensure the copy source is within that addressable window.
fn find_deepest_addressable_copy_source(stack: &Stack, value: &ValueOrAlias) -> Option<u8> {
    let target = value.unaliased();
    let mut required_depth = 0usize;
    let mut source = None;
    for (pos, operand) in stack.iter().rev().enumerate() {
        required_depth += operand.stack_size();
        if required_depth > MASM_STACK_WINDOW_FELTS {
            break;
        }
        if operand.unaliased() == target {
            source = Some(pos as u8);
        }
    }
    source
}

/// Moves move-constrained expected operands towards the top if copy materialization would push them
/// beyond the MASM addressable window.
///
/// Returns `true` if the stack state was modified.
fn preemptively_move_endangered_operands_to_top(builder: &mut SolutionBuilder) -> bool {
    let missing_copy_felts: usize = builder
        .context()
        .expected()
        .iter()
        .filter(|value| value.is_alias() && builder.get_current_position(value).is_none())
        .map(|value| value.stack_size())
        .sum();
    if missing_copy_felts == 0 {
        return false;
    }

    let mut changed = false;

    // Repeatedly move the deepest move-constrained operand that would fall out of the addressable
    // window to the top. This avoids generating solutions that require unsupported stack access
    // when copies are materialized.
    loop {
        let mut worst: Option<(u8, usize)> = None;
        for value in builder.context().expected().iter() {
            if value.is_alias() {
                continue;
            }
            let Some(pos) = builder.get_current_position(value) else {
                continue;
            };
            let current_depth: usize = builder
                .stack()
                .iter()
                .rev()
                .take(pos as usize + 1)
                .map(|v| v.stack_size())
                .sum();
            let projected_depth = current_depth + missing_copy_felts;
            if projected_depth > MASM_STACK_WINDOW_FELTS {
                match worst {
                    None => worst = Some((pos, current_depth)),
                    Some((_, best_depth)) if current_depth > best_depth => {
                        worst = Some((pos, current_depth))
                    }
                    _ => {}
                }
            }
        }

        let Some((pos, _)) = worst else {
            break;
        };
        if pos == 0 {
            break;
        }
        builder.movup(pos);
        changed = true;
    }

    changed
}

fn has_missing_expected_copies(builder: &SolutionBuilder) -> bool {
    builder
        .context()
        .expected()
        .iter()
        .any(|value| value.is_alias() && builder.get_current_position(value).is_none())
}

/// Materialize missing copies in a way which preserves addressability within the MASM stack window.
///
/// Returns `true` if any actions were emitted.
fn materialize_missing_expected_copies(builder: &mut SolutionBuilder) -> Result<bool, TacticError> {
    let mut changed = false;
    loop {
        let mut next_copy: Option<(u8, ValueOrAlias)> = None;
        for expected in builder.context().expected().iter() {
            if builder.get_current_position(expected).is_some() {
                continue;
            }
            // `expected` isn't on the stack because it is a copy we haven't materialized yet
            assert!(expected.is_alias());
            let source_at = find_deepest_addressable_copy_source(builder.stack(), expected)
                .ok_or(TacticError::NotApplicable)?;
            match next_copy {
                None => next_copy = Some((source_at, *expected)),
                Some((best_at, _)) if source_at > best_at => {
                    next_copy = Some((source_at, *expected))
                }
                _ => {}
            }
        }

        let Some((source_at, expected)) = next_copy else {
            break;
        };

        log::trace!("materializing copy of {expected:?} from index {source_at} to top of stack",);

        // When the stack contains exactly 16 field elements, we cannot emit stack manipulation
        // instructions that address index 16+.
        //
        // If the only missing piece is a copy of the operand already on top of the stack, we can
        // safely materialize it by first pushing the original below the 16-field-element addressing
        // window and then duplicating it back to the top.
        if builder.arity() == MASM_STACK_WINDOW_FELTS
            && builder.stack().iter().map(|operand| operand.stack_size()).sum::<usize>()
                == MASM_STACK_WINDOW_FELTS
            && builder.unwrap_expected_position(&expected) == 0
            && source_at == 0
            && (1..builder.arity()).all(|i| builder.is_expected(i as u8))
        {
            builder.movdn(15);
            builder.dup(15, expected.unwrap_alias());
        } else {
            builder.dup(source_at, expected.unwrap_alias());
        }

        changed = true;
    }

    Ok(changed)
}

/// A wrapper around [Linear] that avoids copy materialization patterns which can exceed MASM's
/// 16-field-element addressing window.
///
/// This tactic is intended as a fallback after [Linear] when expected copies are missing and copy
/// materialization risks pushing required operands beyond the MASM addressable window.
#[derive(Default)]
pub struct LinearStackWindow;
impl Tactic for LinearStackWindow {
    fn cost(&self, context: &SolverContext) -> usize {
        core::cmp::max(context.copies().len(), 1)
    }

    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult {
        if !has_missing_expected_copies(builder) {
            return Err(TacticError::NotApplicable);
        }

        preemptively_move_endangered_operands_to_top(builder);
        materialize_missing_expected_copies(builder)?;

        if builder.is_valid() {
            Ok(())
        } else {
            let mut linear = Linear;
            linear.apply(builder)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::opt::operands::{
        Action, OperandMovementConstraintSolver, SolverContext, SolverOptions,
        tactics::LinearStackWindow, testing,
    };

    fn apply_actions(mut stack: crate::OperandStack, actions: &[Action]) -> crate::OperandStack {
        for action in actions.iter().copied() {
            match action {
                Action::Copy(index) => stack.dup(index as usize),
                Action::Swap(index) => stack.swap(index as usize),
                Action::MoveUp(index) => stack.movup(index as usize),
                Action::MoveDown(index) => stack.movdn(index as usize),
            }
        }
        stack
    }

    /// Demonstrates the MASM 16-felt addressing edge case for copy materialization.
    ///
    /// When the stack contains exactly 16 field elements and the only missing operand is a copy of
    /// the value already on top of stack, the tactic must avoid producing a solution which would
    /// require addressing beyond the MASM stack window.
    #[test]
    fn linear_stack_window_full_window_top_copy_does_not_require_unsupported_stack_access() {
        let problem = testing::make_problem_inputs((0..16).collect(), 16, 0b0000_0000_0000_0001);
        let context = SolverContext::new(
            &problem.expected,
            &problem.constraints,
            &problem.stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid");

        let actions = OperandMovementConstraintSolver::new_with_options(
            &problem.expected,
            &problem.constraints,
            &problem.stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid")
        .solve_with_tactic::<LinearStackWindow>()
        .expect("expected tactic to be applicable")
        .expect("expected tactic to produce a full solution");

        let pending = apply_actions(problem.stack.clone(), &actions);
        for (index, expected) in problem.expected.iter().copied().enumerate() {
            assert_eq!(&pending[index], &expected);
        }
        assert!(
            !OperandMovementConstraintSolver::solution_requires_unsupported_stack_access(
                &actions,
                context.stack(),
            ),
            "linear stack window tactic produced a solution requiring unsupported stack access: \
             {problem:#?}"
        );
    }

    #[test]
    fn linear_stack_window_regression_case_does_not_require_unsupported_stack_access() {
        // Regression test: copy materialization can increase stack depth, but the tactic must
        // still never require addressing beyond the MASM stack window (16 field elements).
        let problem = testing::make_problem_inputs(
            vec![0, 6, 9, 3, 2, 11, 5, 12, 10, 4, 1, 7, 8, 13],
            8,
            0b1111_1000,
        );

        let context = SolverContext::new(
            &problem.expected,
            &problem.constraints,
            &problem.stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid");

        let actions = OperandMovementConstraintSolver::new_with_options(
            &problem.expected,
            &problem.constraints,
            &problem.stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid")
        .solve_with_tactic::<LinearStackWindow>()
        .expect("expected tactic to be applicable");
        assert!(
            actions.is_some(),
            "linear stack window tactic produced a partial solution for regression case: \
             {problem:#?}"
        );
        let actions = actions.unwrap();
        let pending = apply_actions(problem.stack.clone(), &actions);
        for (index, expected) in problem.expected.iter().copied().enumerate() {
            assert_eq!(&pending[index], &expected);
        }
        assert!(
            !OperandMovementConstraintSolver::solution_requires_unsupported_stack_access(
                &actions,
                context.stack(),
            ),
            "linear stack window tactic produced a solution requiring unsupported stack access: \
             {problem:#?}"
        );
    }
}
