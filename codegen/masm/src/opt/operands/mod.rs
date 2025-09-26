mod context;
mod solver;
mod stack;
mod tactics;
#[cfg(test)]
mod testing;

pub use midenc_hir::{StackOperand as Operand, ValueOrAlias};

pub use self::solver::{OperandMovementConstraintSolver, SolverError, SolverOptions};
use self::{context::SolverContext, stack::Stack};

/// This represents a specific action that should be taken by
/// the code generator with regard to an operand on the stack.
///
/// The output of the optimizer is a sequence of these actions,
/// the effect of which is to place all of the current instruction's
/// operands exactly where they need to be, just when they are
/// needed.
#[derive(Debug, Copy, Clone)]
pub enum Action {
    /// Copy the operand at the given index to the top of the stack
    Copy(u8),
    /// Swap the operand at the given index with the one on top of the stack
    Swap(u8),
    /// Move the operand at the given index to the top of the stack
    MoveUp(u8),
    /// Move the operand at the top of the stack to the given index
    MoveDown(u8),
}
