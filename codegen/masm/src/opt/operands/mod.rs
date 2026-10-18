mod context;
mod solver;
mod stack;
mod tactics;
#[cfg(test)]
mod testing;

/// A value identity and raw width local to one operand-shuffle request.
///
/// The solver can schedule HIR values or selected physical layouts without owning either IR.
/// Equal IDs must have equal nonzero widths within a request. A nonzero alias distinguishes a
/// required additional copy from occurrences of the original value; it does not change its bits.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ValueOrAlias {
    id: u32,
    alias: u8,
    width: u8,
}

impl ValueOrAlias {
    /// Describe an original value; the request boundary validates its nonzero raw-lane width.
    pub(crate) fn new(id: u32, width: u8) -> Self {
        Self {
            id,
            alias: 0,
            width,
        }
    }

    /// Number of contiguous raw stack lanes occupied by this grouped value.
    pub fn stack_size(&self) -> usize {
        usize::from(self.width)
    }

    /// Label an additional occurrence so the solver cannot satisfy a copy request by consuming
    /// an occurrence that must survive. Copy labels are scoped to this original value.
    pub fn copy(mut self, id: core::num::NonZeroU8) -> Self {
        self.alias = id.get();
        self
    }

    /// Recover the original identity when comparing values independently of copy obligations.
    pub fn unaliased(mut self) -> Self {
        self.alias = 0;
        self
    }

    /// Whether this identity denotes an explicitly required copy.
    pub fn is_alias(&self) -> bool {
        self.alias != 0
    }

    /// Return the copy label, panicking if this denotes the original occurrence.
    pub fn unwrap_alias(self) -> core::num::NonZeroU8 {
        core::num::NonZeroU8::new(self.alias).expect("expected a solver copy")
    }
}

impl core::fmt::Display for ValueOrAlias {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}", self.id, self.alias)
    }
}

/// One value occurrence in a solver dependency graph.
///
/// Position distinguishes repeated occurrences of the same value. Ordering by position first
/// gives graph traversals a deterministic stack order rather than an incidental value-ID order.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Operand {
    /// Grouped operand index from the top, not a raw-lane depth.
    pub pos: u8,
    /// Value and copy obligation occupying this position.
    pub value: ValueOrAlias,
}

pub use self::solver::{OperandMovementConstraintSolver, SolverError, SolverOptions};
use self::{context::SolverContext, stack::Stack};

/// The number of field elements addressable by MASM stack manipulation instructions.
///
/// MASM legalization bounds a call's operands by this same window — an indirect call schedules its
/// arguments together with the operands selecting the callee, the table index for
/// `hir.exec_indirect` and the root word for `hir.dyncall` — so it imports this constant rather
/// than restating the number.
pub(crate) const MASM_STACK_WINDOW_FELTS: usize = miden_core::program::MIN_STACK_DEPTH;

/// This represents a specific action that should be taken by
/// the code generator with regard to an operand on the stack.
///
/// The output of the optimizer is a sequence of these actions,
/// the effect of which is to place all of the current instruction's
/// operands exactly where they need to be, just when they are
/// needed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
