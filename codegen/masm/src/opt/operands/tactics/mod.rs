use core::num::NonZeroU8;

use super::{Action, Operand, SolverContext, Stack, ValueOrAlias};

mod copy_all;
mod linear;
mod move_down_and_swap;
mod move_up_and_swap;
mod swap_and_move_up;
mod two_args;

pub use self::{
    copy_all::CopyAll, linear::Linear, move_down_and_swap::MoveDownAndSwap,
    move_up_and_swap::MoveUpAndSwap, swap_and_move_up::SwapAndMoveUp, two_args::TwoArgs,
};

/// An error returned by an [OperandMovementConstraintSolver] tactic
#[derive(Debug)]
pub enum TacticError {
    /// The tactic could not be applied due to a precondition
    /// that is required for the tactic to succeed. For example,
    /// a tactic that does not handle copies will have a precondition
    /// that there are no copy constraints, and will not attempt
    /// to compute a solution if there are.
    PreconditionFailed,
    /// The tactic could not be applied because the pattern it
    /// looks for could not be found in the current context.
    NotApplicable,
}

/// The type of result produced by a [Tactic]
pub type TacticResult = Result<(), TacticError>;

/// A [Tactic] implements an algorithm for solving operand movement constraints
/// that adhere to a specific pattern or patterns.
///
/// Tactics should attempt to fail early by first recognizing whether the state
/// of the stack adheres to the pattern which the tactic is designed to solve,
/// and only then should it actually compute the specific actions needed to lay
/// out the stack as expected.
///
/// A tactic does not need to check if the result of computing a solution actually
/// solves all of the constraints, that is done by [OperandMovementConstraintSolver].
///
/// Tactics can have an associated cost, which is used when iterating over multiple
/// tactics looking for the best solution. You should strive to make the cost reflect
/// the computational complexity of the tactic to the degree possible. The default
/// cost for all tactics is 1.
pub trait Tactic {
    /// The name of this tactic to use in informational messages.
    ///
    /// The default name of each tactic is the name of the implementing type.
    fn name(&self) -> &'static str {
        let name = core::any::type_name::<Self>();
        match name.find(|c: char| c.is_ascii_uppercase()) {
            None => name,
            Some(index) => name.split_at(index).1,
        }
    }

    /// The computational cost of this tactic in units of optimization fuel.
    ///
    /// The provided context can be used to compute a cost dynamically based on
    /// the number of expected operands, the constraints, and the size of the stack.
    ///
    /// The default cost is 1.
    fn cost(&self, _context: &SolverContext) -> usize {
        1
    }

    /// Apply this tactic using the provided [SolutionBuilder].
    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult;
}

/// This struct is constructed by an [OperandMovementConstraintSolver], and provided
/// to each [Tactic] it applies in search of a solution.
///
/// The purpose of this builder is to abstract over the solver context, the pending
/// state of the stack, and to ensure that the solution computed by a [Tactic] is
/// captured accurately.
#[derive(Debug, Clone)]
pub struct SolutionBuilder<'a> {
    /// The current solver context
    context: &'a SolverContext,
    /// The state of the stack after applying `actions`
    pending: Stack,
    /// The actions that represent the solution constructed so far.
    actions: Vec<Action>,
}
impl<'a> SolutionBuilder<'a> {
    #[doc(hidden)]
    pub fn new(context: &'a SolverContext) -> Self {
        Self {
            context,
            pending: context.stack().clone(),
            actions: vec![],
        }
    }

    /// Return the number of operands expected by the current instruction being emitted
    pub fn arity(&self) -> usize {
        self.context.arity()
    }

    /// Return true if the current context requires operand copies to be made
    pub fn requires_copies(&self) -> bool {
        !self.context.copies().is_empty()
    }

    /// Return the total number of copied operands expected
    pub fn num_copies(&self) -> usize {
        self.context.copies().len()
    }

    /// Returns true if the solution must be strict
    pub fn requires_strict_solution(&self) -> bool {
        self.context.is_strict()
    }

    /// Get a reference to the underlying context of the solver
    #[inline(always)]
    pub fn context(&self) -> &'a SolverContext {
        self.context
    }

    /// Get a reference to the state of the stack after applying the pending solution
    #[inline(always)]
    pub fn stack(&self) -> &Stack {
        &self.pending
    }

    /// Take the current solution and reset the builder
    pub fn take(&mut self) -> Vec<Action> {
        let actions = core::mem::take(&mut self.actions);
        self.pending.reset_to(self.context.stack());
        actions
    }

    /// Discard the current solution, and reset back to the initial state
    pub fn discard(&mut self) {
        self.actions.clear();
        self.pending.reset_to(self.context.stack());
    }

    /// Check if the pending solution is a valid solution
    pub fn is_valid(&self) -> bool {
        self.context.is_solved(&self.pending)
    }

    /// Get the value expected at `index`
    pub fn get_expected(&self, index: u8) -> Option<ValueOrAlias> {
        self.context.expected().get(index as usize).copied()
    }

    /// Get the value expected at `index` or panic
    #[track_caller]
    pub fn unwrap_expected(&self, index: u8) -> ValueOrAlias {
        match self.get_expected(index) {
            Some(value) => value,
            None => panic!(
                "expected operand {index} does not exist: there are only {} expected operands",
                self.context.arity()
            ),
        }
    }

    /// Get the value currently at `index` in this solution
    #[allow(unused)]
    pub fn get_current(&self, index: u8) -> Option<ValueOrAlias> {
        self.pending.get(index as usize).copied()
    }

    /// Get the value currently at `index` in this solution
    #[track_caller]
    pub fn unwrap_current(&self, index: u8) -> ValueOrAlias {
        self.pending.get(index as usize).copied().unwrap_or_else(|| {
            panic!(
                "operand {index} does not exist: the stack contains only {} operands",
                self.pending.len()
            )
        })
    }

    /// Get the position at which `value` is expected
    pub fn get_expected_position(&self, value: &ValueOrAlias) -> Option<u8> {
        self.context.expected().position(value).map(|index| index as u8)
    }

    #[track_caller]
    pub fn unwrap_expected_position(&self, value: &ValueOrAlias) -> u8 {
        match self.get_expected_position(value) {
            Some(pos) => pos,
            None => panic!("value {value:?} is not an expected operand"),
        }
    }

    /// Get the current position of `value` in this solution
    #[inline]
    pub fn get_current_position(&self, value: &ValueOrAlias) -> Option<u8> {
        self.pending.position(value).map(|index| index as u8)
    }

    pub fn get_current_position_skip(&self, start_index: u8, value: &ValueOrAlias) -> Option<u8> {
        self.pending.position_skip(start_index as usize, value).map(|index| index as u8)
    }

    /// Get the current position of `value` in this solution, or panic
    #[track_caller]
    pub fn unwrap_current_position(&self, value: &ValueOrAlias) -> u8 {
        match self.get_current_position(value) {
            Some(pos) => pos,
            None => panic!("value {value:?} not found on operand stack"),
        }
    }

    /// Returns true if the value expected at `index` is currently at that index
    pub fn is_expected(&self, index: u8) -> bool {
        self.get_expected(index)
            .map(|v| v == self.pending[index as usize])
            .unwrap_or(false)
    }

    /// Duplicate the operand at `index` to the top of the stack
    ///
    /// This records a `Copy` action, and updates the state of the stack
    pub fn dup(&mut self, index: u8, alias_id: NonZeroU8) {
        self.pending.dup(index as usize, alias_id);
        self.actions.push(Action::Copy(index));
    }

    /// Swap the operands at `index` and the top of the stack
    ///
    /// This records a `Swap` action, and updates the state of the stack
    pub fn swap(&mut self, index: u8) {
        self.pending.swap(index as usize);
        self.actions.push(Action::Swap(index));
    }

    /// Move the operand at `index` to the top of the stack
    ///
    /// This records a `MoveUp` action, and updates the state of the stack
    #[track_caller]
    pub fn movup(&mut self, index: u8) {
        assert_ne!(index, 0);
        if index == 1 {
            self.swap(index);
        } else {
            self.pending.movup(index as usize);
            self.actions.push(Action::MoveUp(index));
        }
    }

    /// Move the operand at the top of the stack to `index`
    ///
    /// This records a `MoveDown` action, and updates the state of the stack
    #[track_caller]
    pub fn movdn(&mut self, index: u8) {
        assert_ne!(index, 0);
        if index == 1 {
            self.swap(index);
        } else {
            self.pending.movdn(index as usize);
            self.actions.push(Action::MoveDown(index));
        }
    }

    /// Evicts the operand on top of the stack by moving it down past the last expected operand.
    pub fn evict(&mut self) {
        self.evict_from(0)
    }

    /// Same as `evict`, but assumes that we're evicting an operand at `index`
    #[inline]
    pub fn evict_from(&mut self, index: u8) {
        if index > 0 {
            self.movup(index);
        }
        self.movdn(self.context.arity() as u8);
    }
}
