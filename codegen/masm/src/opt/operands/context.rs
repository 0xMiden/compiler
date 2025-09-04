use core::num::NonZeroU8;

use midenc_hir::{self as hir, hashbrown, FxHashMap};

use super::{SolverError, Stack, ValueOrAlias};
use crate::{Constraint, OperandStack};

/// The context associated with an instance of [OperandMovementConstraintSolver].
///
/// Contained in this context is the current state of the stack, the expected operands, whether the
/// expected operands may be out of order, the constraints on those operands, and metadata about
/// copied operands.
#[derive(Debug)]
pub struct SolverContext {
    stack: Stack,
    expected: Stack,
    allow_unordered: bool,
    copies: CopyInfo,
}
impl SolverContext {
    pub fn new(
        expected: &[hir::ValueRef],
        allow_unordered: bool,
        constraints: &[Constraint],
        stack: &OperandStack,
    ) -> Result<Self, SolverError> {
        // Compute the expected output on the stack, as well as alias/copy information
        let mut stack = Stack::from(stack);
        let mut expected_output = Stack::default();
        let mut copies = CopyInfo::default();
        for (value, constraint) in expected.iter().rev().zip(constraints.iter().rev()) {
            let value = ValueOrAlias::from(*value);
            match constraint {
                // If we observe a value with move semantics, then it is
                // always referencing the original value
                Constraint::Move => {
                    expected_output.push(value);
                }
                // If we observe a value with copy semantics, then the expected
                // output is always an alias, because the original would need to
                // be preserved
                Constraint::Copy => {
                    expected_output.push(copies.push(value));
                }
            }
        }

        // Rename multiple occurrences of the same value on the operand stack, if present
        let mut dupes = CopyInfo::default();
        for value in stack.iter_mut().rev() {
            *value = dupes.push_if_duplicate(*value);
        }

        // Determine if the stack is already in the desired order
        let requires_copies = copies.copies_required();
        let is_solved = !requires_copies
            && stack.len() >= expected_output.len()
            && expected_output
                .iter()
                .eq(stack.iter().skip(stack.len() - expected_output.len()));
        if is_solved {
            return Err(SolverError::AlreadySolved);
        }

        Ok(Self {
            stack,
            expected: expected_output,
            allow_unordered,
            copies,
        })
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

    pub fn unordered_allowed(&self) -> bool {
        self.allow_unordered
    }

    /// Return true if the given stack matches what is expected
    /// if a solution was correctly found.
    pub fn is_solved(&self, pending: &Stack) -> bool {
        debug_assert!(pending.len() >= self.expected.len());

        let is_solved_exactly = self
            .expected
            .iter()
            .eq(pending.iter().skip(pending.len() - self.expected.len()));

        let both_same_value =
            self.expected.len() == 2 && self.expected[0].value() == self.expected[1].value();

        is_solved_exactly
            || ((self.allow_unordered || both_same_value) && self.is_solved_unordered(pending))
    }

    /// Return whether all of the expected operands are at the top of the pending stack but in any
    /// order.
    fn is_solved_unordered(&self, pending: &Stack) -> bool {
        // This is effectively a multiset comparison.  Use a map from value to count as the set.

        fn make_set<'a, VI>(vals_iter: VI) -> FxHashMap<ValueOrAlias, usize>
        where
            VI: Iterator<Item = &'a ValueOrAlias>,
        {
            let mut set = FxHashMap::default();
            for val in vals_iter {
                set.entry(*val).and_modify(|c| *c += 1).or_insert(1);
            }
            set
        }

        let expected_set = make_set(self.expected.iter());
        let pending_set = make_set(pending.iter().skip(pending.len() - self.expected.len()));

        pending_set == expected_set
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

    /// Push a copy of `value`, but only if `value` has already been seen
    /// at least once, i.e. `value` is a duplicate.
    ///
    /// NOTE: It is expected that `value` is not an alias.
    pub fn push_if_duplicate(&mut self, value: ValueOrAlias) -> ValueOrAlias {
        use hashbrown::hash_map::Entry;

        assert!(!value.is_alias());

        match self.copies.entry(value) {
            // `value` is not a duplicate
            Entry::Vacant(entry) => {
                entry.insert(0);
                value
            }
            // `value` is a duplicate, record it as such
            Entry::Occupied(mut entry) => {
                self.num_copies += 1;
                let next_id = entry.get_mut();
                *next_id += 1;
                value.copy(unsafe { NonZeroU8::new_unchecked(*next_id) })
            }
        }
    }

    /// Returns true if `value` has at least one copy
    pub fn has_copies(&self, value: &ValueOrAlias) -> bool {
        self.copies.get(value).map(|count| *count > 0).unwrap_or(false)
    }

    /// Returns true if any of the values seen so far have copies
    pub fn copies_required(&self) -> bool {
        self.copies.values().any(|count| *count > 0)
    }
}
