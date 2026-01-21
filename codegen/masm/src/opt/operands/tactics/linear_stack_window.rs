use super::*;
use crate::opt::operands::MASM_STACK_WINDOW_FELTS;

/// Represents the portion of the operand stack which is directly addressable by MASM stack
/// manipulation instructions.
#[derive(Debug, Copy, Clone)]
struct AddressableStackWindow {
    /// The deepest stack operand index (0-based from the top) which remains addressable.
    deepest_index: u8,
    /// The total number of field elements contained in the addressable portion of the stack.
    depth_felts: usize,
}
impl AddressableStackWindow {
    /// Compute the addressable window for `stack`.
    ///
    /// Returns `None` if the top operand itself exceeds the MASM stack window.
    fn for_stack(stack: &Stack) -> Option<Self> {
        let mut depth_felts = 0usize;
        let mut deepest_index = None;
        for (pos, operand) in stack.iter().rev().enumerate() {
            depth_felts += operand.stack_size();
            if depth_felts > MASM_STACK_WINDOW_FELTS {
                break;
            }
            deepest_index = Some(pos as u8);
        }

        deepest_index.map(|deepest_index| Self {
            deepest_index,
            depth_felts,
        })
    }
}

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

/// Materialize a missing expected copy with a bias towards keeping the top-of-stack window
/// addressable.
fn materialize_copy(builder: &mut SolutionBuilder, source_at: u8, expected: ValueOrAlias) {
    let expected_felts: usize =
        builder.context().expected().iter().map(|value| value.stack_size()).sum();

    // When the expected operands occupy the full MASM stack window, any materialized copy implies
    // that at least one preserved source operand must be pushed out of the addressable window.
    //
    // We do this by moving the chosen source operand to the deepest addressable position before
    // duplicating it. This ensures the source is pushed below the 16-felt window by the dup itself
    // and avoids emitting `movdn(16)` / `movup(16)` patterns which MASM cannot encode.
    if expected_felts == MASM_STACK_WINDOW_FELTS
        && let Some(window) = AddressableStackWindow::for_stack(builder.stack())
        && window.depth_felts == MASM_STACK_WINDOW_FELTS
    {
        if source_at > 0 {
            builder.movup(source_at);
        }
        if window.deepest_index > 0 {
            builder.movdn(window.deepest_index);
        }
        builder.dup(window.deepest_index, expected.unwrap_alias());
        return;
    }

    builder.dup(source_at, expected.unwrap_alias());
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

        materialize_copy(builder, source_at, expected);

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
    use std::rc::Rc;

    use midenc_hir::{self as hir, Type};

    use super::*;
    use crate::{
        Constraint, OperandStack,
        opt::{
            OperandMovementConstraintSolver,
            operands::{Action, SolverOptions},
        },
    };

    /// Apply `actions` to the operand stack and assert that the expected prefix matches.
    fn assert_actions_place_expected_on_top(
        stack: &OperandStack,
        expected: &[hir::ValueRef],
        actions: &[Action],
    ) {
        let mut stack = stack.clone();
        for action in actions.iter().copied() {
            match action {
                Action::Copy(index) => stack.dup(index as usize),
                Action::Swap(index) => stack.swap(index as usize),
                Action::MoveUp(index) => stack.movup(index as usize),
                Action::MoveDown(index) => stack.movdn(index as usize),
            }
        }

        for (index, expected) in expected.iter().copied().enumerate() {
            assert_eq!(
                &stack[index], &expected,
                "solution did not place {} at the correct location on the stack",
                expected
            );
        }
    }

    /// Demonstrates the full-window copy materialization pattern.
    ///
    /// When the expected operands occupy the entire 16-felt MASM stack window, copy
    /// materialization can push operands out of the addressable window. The tactic avoids this by
    /// moving the copy source to the deepest addressable position before duplicating.
    #[test]
    fn linear_stack_window_full_window_copy_materialization_moves_source_to_deepest_before_dup() {
        let hir_ctx = Rc::new(hir::Context::default());
        let block = hir_ctx.create_block_with_params(core::iter::repeat_n(Type::I128, 4));
        let block = block.borrow();
        let block_args = block.arguments();

        let mut stack = OperandStack::default();
        // Stack top is `[v1, v2, v3, v0]`.
        for value in [
            block_args[0] as hir::ValueRef,
            block_args[3] as hir::ValueRef,
            block_args[2] as hir::ValueRef,
            block_args[1] as hir::ValueRef,
        ] {
            stack.push(value);
        }

        let expected = vec![
            block_args[0] as hir::ValueRef,
            block_args[1] as hir::ValueRef,
            block_args[2] as hir::ValueRef,
            block_args[3] as hir::ValueRef,
        ];
        let constraints =
            vec![Constraint::Copy, Constraint::Move, Constraint::Move, Constraint::Move];

        let actions = OperandMovementConstraintSolver::new_with_options(
            &expected,
            &constraints,
            &stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid")
        .solve_with_tactic::<LinearStackWindow>()
        .expect("expected tactic to be applicable")
        .expect("expected tactic to produce a full solution");

        assert_eq!(actions, vec![Action::MoveUp(3), Action::MoveDown(3), Action::Copy(3)]);
        assert_actions_place_expected_on_top(&stack, &expected, &actions);
    }

    /// Demonstrates how the tactic preemptively moves endangered move-constrained operands.
    ///
    /// Here, the copy source starts on top of the stack, but materializing it would push the
    /// deepest move-constrained expected operand out of MASM's 16-felt addressing window. The
    /// tactic moves endangered operands to the top before materializing the copy.
    #[test]
    fn linear_stack_window_full_window_preemptively_moves_endangered_operands() {
        let hir_ctx = Rc::new(hir::Context::default());
        let block = hir_ctx.create_block_with_params(core::iter::repeat_n(Type::I128, 4));
        let block = block.borrow();
        let block_args = block.arguments();

        let mut stack = OperandStack::default();
        // Stack top is `[v0, v1, v2, v3]`.
        for value in block_args.iter().copied().rev() {
            stack.push(value as hir::ValueRef);
        }

        let expected = vec![
            block_args[0] as hir::ValueRef,
            block_args[1] as hir::ValueRef,
            block_args[2] as hir::ValueRef,
            block_args[3] as hir::ValueRef,
        ];
        let constraints =
            vec![Constraint::Copy, Constraint::Move, Constraint::Move, Constraint::Move];

        let actions = OperandMovementConstraintSolver::new_with_options(
            &expected,
            &constraints,
            &stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid")
        .solve_with_tactic::<LinearStackWindow>()
        .expect("expected tactic to be applicable")
        .expect("expected tactic to produce a full solution");

        assert_eq!(
            actions,
            vec![
                Action::MoveUp(3),
                Action::MoveUp(3),
                Action::MoveUp(3),
                Action::MoveUp(3),
                Action::MoveDown(3),
                Action::Copy(3),
            ]
        );
        assert_actions_place_expected_on_top(&stack, &expected, &actions);
    }
}
