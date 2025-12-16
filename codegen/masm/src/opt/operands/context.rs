use core::num::NonZeroU8;

use midenc_hir::{self as hir, FxHashMap, hashbrown};
use smallvec::SmallVec;

use super::{SolverError, SolverOptions, Stack, ValueOrAlias};
use crate::{Constraint, OperandStack};

/// The context associated with an instance of [OperandMovementConstraintSolver].
///
/// Contained in this context is the current state of the stack, the expected operands, whether the
/// expected operands may be out of order, the constraints on those operands, and metadata about
/// copied operands.
#[derive(Debug)]
pub struct SolverContext {
    options: SolverOptions,
    stack: Stack,
    expected: Stack,
    copies: CopyInfo,
}
impl SolverContext {
    pub fn new(
        expected: &[hir::ValueRef],
        constraints: &[Constraint],
        stack: &OperandStack,
        options: SolverOptions,
    ) -> Result<Self, SolverError> {
        // Compute the expected output on the stack, as well as alias/copy information
        let stack = Stack::from(stack);
        let mut expected_output = Stack::default();
        let mut copies = CopyInfo::default();
        for (value, constraint) in expected.iter().rev().zip(constraints.iter().rev()) {
            let value = ValueOrAlias::from(*value);
            match constraint {
                // If we observe a value with move semantics, then it is always referencing the
                // original value
                Constraint::Move => {
                    expected_output.push(value);
                }
                // If we observe a value with copy semantics, then the expected output is always an
                // alias, because the original needs to be preserved.
                //
                // NOTE: Just because an expected operand has a copy constraint applied, does not
                // mean that there aren't multiple copies on the input stack already - it indicates
                // that all of those copies must be preserved, and a _new_ copy must be materialized
                // by the solution we produce.
                Constraint::Copy => {
                    expected_output.push(copies.push(value));
                }
            }
        }

        // Determine if the stack is already in the desired order
        let context = Self {
            options,
            stack,
            expected: expected_output,
            copies,
        };
        let is_solved = !context.copies.copies_required()
            && context.stack.len() >= context.expected.len()
            && context.is_solved(&context.stack);
        if is_solved {
            return Err(SolverError::AlreadySolved);
        }

        Ok(context)
    }

    /// Access the current [SolverOptions]
    #[allow(unused)]
    #[inline(always)]
    pub fn options(&self) -> &SolverOptions {
        &self.options
    }

    /// Returns the number of operands expected by the current instruction
    #[inline]
    pub fn arity(&self) -> usize {
        self.expected.len()
    }

    /// Get a reference to the copy analysis results
    #[inline(always)]
    pub fn copies(&self) -> &CopyInfo {
        &self.copies
    }

    /// Get a reference to the state of the stack at the current program point
    #[inline(always)]
    pub fn stack(&self) -> &Stack {
        &self.stack
    }

    /// Get a [Stack] representing the state of the stack for a valid solution.
    ///
    /// NOTE: The returned stack only contains the expected operands, not the full stack
    #[inline(always)]
    pub fn expected(&self) -> &Stack {
        &self.expected
    }

    /// Returns true if the current solver requires a strict solution
    #[inline(always)]
    pub fn is_strict(&self) -> bool {
        self.options.strict
    }

    /// Return true if the given stack matches what is expected
    /// if a solution was correctly found.
    pub fn is_solved(&self, pending: &Stack) -> bool {
        debug_assert!(pending.len() >= self.expected.len());

        let is_strict_solution =
            self.expected.iter().rev().eq(pending.iter().rev().take(self.expected.len()));

        is_strict_solution || (!self.is_strict() && self.is_non_strict_solution(pending))
    }

    /// Return whether all of the expected operands are at the top of the pending stack but in any
    /// order.
    fn is_non_strict_solution(&self, pending: &Stack) -> bool {
        let mut expected = self.expected.iter().rev().collect::<SmallVec<[_; 4]>>();
        let mut pending = pending.iter().rev().take(expected.len()).collect::<SmallVec<[_; 4]>>();

        expected.sort();
        pending.sort();

        expected == pending
    }
}

#[derive(Debug, Default)]
pub struct CopyInfo {
    copies: FxHashMap<ValueOrAlias, u8>,
    num_copies: u8,
}
impl CopyInfo {
    /// Returns the number of copies recorded in this structure
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.num_copies as usize
    }

    /// Returns true if there are no copied values
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.num_copies == 0
    }

    /// Push a new copy of `value`, returning an alias of that value
    ///
    /// NOTE: It is expected that `value` is not an alias.
    pub fn push(&mut self, value: ValueOrAlias) -> ValueOrAlias {
        use hashbrown::hash_map::Entry;

        assert!(!value.is_alias());

        self.num_copies += 1;
        match self.copies.entry(value) {
            Entry::Vacant(entry) => {
                entry.insert(1);
                value.copy(unsafe { NonZeroU8::new_unchecked(1) })
            }
            Entry::Occupied(mut entry) => {
                let next_id = entry.get_mut();
                *next_id += 1;
                value.copy(unsafe { NonZeroU8::new_unchecked(*next_id) })
            }
        }
    }

    /// Returns true if `value` has at least one copy
    pub fn has_copies(&self, value: &ValueOrAlias) -> bool {
        // Make sure we're checking for copies of the unaliased value
        let value = value.unaliased();
        self.copies.get(&value).map(|count| *count > 0).unwrap_or(false)
    }

    /// Returns true if any of the values seen so far have copies
    pub fn copies_required(&self) -> bool {
        self.copies.values().any(|count| *count > 0)
    }
}
