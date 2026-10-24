use alloc::collections::BTreeMap;
use core::num::NonZeroU8;

#[cfg(test)]
use midenc_hir as hir;
use midenc_hir::{FxHashMap, hashbrown};
use smallvec::SmallVec;

use super::{SolverError, SolverOptions, Stack, ValueOrAlias};
use crate::Constraint;
#[cfg(test)]
use crate::OperandStack;

/// The stack and copy obligations for one operand-shuffle search.
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
    #[cfg(test)]
    pub fn new(
        expected: &[hir::ValueRef],
        constraints: &[Constraint],
        stack: &OperandStack,
        options: SolverOptions,
    ) -> Result<Self, SolverError> {
        // Preserve HIR identity ordering while removing all HIR references at this boundary.
        let values: BTreeMap<_, _> = stack
            .iter()
            .rev()
            .map(|operand| {
                let value = operand.as_value().expect("expected a value operand");
                (value.borrow().id(), value)
            })
            .chain(expected.iter().map(|value| (value.borrow().id(), *value)))
            .collect();
        let identities: BTreeMap<_, _> = values
            .into_iter()
            .enumerate()
            .map(|(id, (key, value))| {
                (key, ValueOrAlias::new(id as u32, value.borrow().ty().size_in_felts() as u8))
            })
            .collect();
        let expected: Vec<_> =
            expected.iter().map(|value| identities[&value.borrow().id()]).collect();
        let stack: Vec<_> = stack
            .iter()
            .rev()
            .map(|operand| {
                identities[&operand.as_value().expect("expected a value operand").borrow().id()]
            })
            .collect();
        Self::build(&expected, constraints, &stack, options)
    }

    /// Build a request from immutable grouped layouts, without creating temporary HIR values.
    ///
    /// `expected` and `stack` are ordered from the top; each expected value has one move/copy
    /// constraint. Input identities must be unaliased, present in the stack, and use consistent
    /// nonzero widths. Returns `NoSolution` if these conditions or the solver's index limits fail.
    pub fn from_layouts(
        expected: &[ValueOrAlias],
        constraints: &[Constraint],
        stack: &[ValueOrAlias],
        options: SolverOptions,
    ) -> Result<Self, SolverError> {
        if expected.len() != constraints.len()
            || expected.len() > u8::MAX as usize
            || stack.len() > u8::MAX as usize
            || stack.len() + constraints.iter().filter(|c| matches!(c, Constraint::Copy)).count()
                > u8::MAX as usize
            || expected
                .iter()
                .chain(stack)
                .any(|value| value.stack_size() == 0 || value.is_alias())
        {
            return Err(SolverError::NoSolution);
        }
        let mut widths = BTreeMap::new();
        for value in stack.iter().chain(expected) {
            if widths.insert(value.id, value.width).is_some_and(|width| width != value.width) {
                return Err(SolverError::NoSolution);
            }
        }
        if expected.iter().any(|value| !stack.contains(value)) {
            return Err(SolverError::NoSolution);
        }
        Self::build(expected, constraints, stack, options)
    }

    /// Both source forms use the same tactics and copy analysis. The selected-view
    /// representability checks belong to its boundary, not to the retained HIR API.
    fn build(
        expected: &[ValueOrAlias],
        constraints: &[Constraint],
        stack: &[ValueOrAlias],
        options: SolverOptions,
    ) -> Result<Self, SolverError> {
        // Compute the expected output on the stack, as well as alias/copy information
        let stack = Stack::from_iter(stack.iter().copied());
        let mut expected_output = Stack::default();
        let mut copies = CopyInfo::default();
        for (value, constraint) in expected.iter().rev().zip(constraints.iter().rev()) {
            let value = *value;
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

    /// Returns true if any of the values seen so far have copies
    pub fn copies_required(&self) -> bool {
        self.copies.values().any(|count| *count > 0)
    }
}
