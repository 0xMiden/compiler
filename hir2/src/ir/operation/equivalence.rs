use core::hash::Hasher;

use bitflags::bitflags;
use smallvec::SmallVec;

use super::Operation;
use crate::{traits::Commutative, OpOperand, Region, Value, ValueRef};

bitflags! {
    #[derive(Copy, Clone)]
    pub struct OperationEquivalenceFlags : u8 {
        const NONE = 0;
        const IGNORE_LOCATIONS = 1;
    }
}

impl Default for OperationEquivalenceFlags {
    fn default() -> Self {
        Self::NONE
    }
}

pub trait OperationHasher {
    fn hash_operation<H: Hasher>(&self, op: &Operation, hasher: &mut H);
}

#[derive(Default)]
pub struct DefaultOperationHasher;

impl OperationHasher for DefaultOperationHasher {
    fn hash_operation<H: Hasher>(&self, op: &Operation, hasher: &mut H) {
        op.hash_with_options(
            OperationEquivalenceFlags::default(),
            DefaultValueHasher,
            DefaultValueHasher,
            hasher,
        );
    }
}

#[derive(Default)]
pub struct IgnoreValueEquivalenceOperationHasher;

impl OperationHasher for IgnoreValueEquivalenceOperationHasher {
    fn hash_operation<H: Hasher>(&self, op: &Operation, hasher: &mut H) {
        op.hash_with_options(
            OperationEquivalenceFlags::IGNORE_LOCATIONS,
            IgnoreValueHasher,
            IgnoreValueHasher,
            hasher,
        );
    }
}

pub trait ValueHasher {
    fn hash_value<H: Hasher>(&self, value: ValueRef, hasher: &mut H);
}

/// A [ValueHasher] impl that hashes a value based on its address in memory.
///
/// This is generally used with [OperationEquivalence] to require operands/results between two
/// operations to be exactly the same.
#[derive(Default)]
pub struct DefaultValueHasher;

impl ValueHasher for DefaultValueHasher {
    fn hash_value<H: Hasher>(&self, value: ValueRef, hasher: &mut H) {
        core::ptr::hash(ValueRef::as_ptr(&value), hasher);
    }
}

/// A [ValueHasher] impl that ignores operands/results, i.e. the hash is unchanged
#[derive(Default)]
pub struct IgnoreValueHasher;

impl ValueHasher for IgnoreValueHasher {
    fn hash_value<H: Hasher>(&self, _value: ValueRef, _hasher: &mut H) {}
}

impl Operation {
    pub fn hash_with_options<H>(
        &self,
        flags: OperationEquivalenceFlags,
        operand_hasher: impl ValueHasher,
        result_hasher: impl ValueHasher,
        hasher: &mut H,
    ) where
        H: core::hash::Hasher,
    {
        use core::hash::Hash;

        // Hash operations based upon their:
        //
        // - Operation name
        // - Attributes
        // - Result types
        self.name.hash(hasher);
        self.attrs.hash(hasher);
        for result in self.results().iter() {
            let result = result.borrow();
            result.ty().hash(hasher);
        }

        if !flags.contains(OperationEquivalenceFlags::IGNORE_LOCATIONS) {
            self.span.hash(hasher);
        }

        // Operands
        for operand in self.operands().iter() {
            let operand = operand.borrow();
            operand_hasher.hash_value(operand.as_value_ref(), hasher);
        }

        // Results
        for result in self.results().iter() {
            let result = result.borrow();
            result_hasher.hash_value(result.as_value_ref(), hasher);
        }
    }

    pub fn is_equivalent(&self, rhs: &Operation, flags: OperationEquivalenceFlags) -> bool {
        self.is_equivalent_with_options(rhs, flags, |l, r| core::ptr::addr_eq(l, r))
    }

    pub fn is_equivalent_with_options<F>(
        &self,
        rhs: &Operation,
        flags: OperationEquivalenceFlags,
        check_equivalent: F,
    ) -> bool
    where
        F: Fn(&dyn Value, &dyn Value) -> bool,
    {
        if core::ptr::addr_eq(self, rhs) {
            return true;
        }

        // 1. Compare operation properties
        if self.name != rhs.name
            || self.num_regions() != rhs.num_regions()
            || self.num_successors() != rhs.num_successors()
            || self.num_operands() != rhs.num_operands()
            || self.num_results() != rhs.num_results()
            || self.attrs != rhs.attrs
        {
            return false;
        }

        if !flags.contains(OperationEquivalenceFlags::IGNORE_LOCATIONS) && self.span != rhs.span {
            return false;
        }

        // 2. Compare operands
        if self.implements::<dyn Commutative>() {
            let lhs_operands = self.operands().all();
            let rhs_operands = rhs.operands().all();
            let mut lhs_operands = SmallVec::<[_; 2]>::from_slice(lhs_operands.as_slice());
            lhs_operands.sort_by(sort_operands);
            let mut rhs_operands = SmallVec::<[_; 2]>::from_slice(rhs_operands.as_slice());
            rhs_operands.sort_by(sort_operands);
            if !are_operands_equivalent(&lhs_operands, &rhs_operands, &check_equivalent) {
                return false;
            }
        } else {
            // Check pair-wise for equivalence
            let lhs = self.operands.all();
            let rhs = rhs.operands.all();
            if !are_operands_equivalent(lhs.as_slice(), rhs.as_slice(), &check_equivalent) {
                return false;
            }
        }

        // 3. Compare result types
        for (lhs_r, rhs_r) in
            self.results().all().iter().copied().zip(rhs.results().all().iter().copied())
        {
            let lhs_r = lhs_r.borrow();
            let rhs_r = rhs_r.borrow();
            if lhs_r.ty() != rhs_r.ty() {
                return false;
            }
        }

        // 4. Compare regions
        for (lhs_region, rhs_region) in self.regions().iter().zip(rhs.regions().iter()) {
            if !is_region_equivalent_to(&lhs_region, &rhs_region, flags, &check_equivalent) {
                return false;
            }
        }

        true
    }
}

pub fn ignore_value_equivalence(_lhs: &dyn Value, _rhs: &dyn Value) -> bool {
    true
}

pub fn exact_value_match(lhs: &dyn Value, rhs: &dyn Value) -> bool {
    core::ptr::addr_eq(lhs, rhs)
}

fn is_region_equivalent_to<F>(
    _lhs: &Region,
    _rhs: &Region,
    _flags: OperationEquivalenceFlags,
    _check_equivalent: F,
) -> bool
where
    F: Fn(&dyn Value, &dyn Value) -> bool,
{
    todo!()
}

fn sort_operands(a: &OpOperand, b: &OpOperand) -> core::cmp::Ordering {
    let a = a.borrow().as_value_ref();
    let b = b.borrow().as_value_ref();
    let a = ValueRef::as_ptr(&a).addr();
    let b = ValueRef::as_ptr(&b).addr();
    a.cmp(&b)
}

fn are_operands_equivalent<F>(a: &[OpOperand], b: &[OpOperand], check_equivalent: F) -> bool
where
    F: Fn(&dyn Value, &dyn Value) -> bool,
{
    // Check pair-wise for equivalence
    for (a, b) in a.iter().copied().zip(b.iter().copied()) {
        let a = a.borrow();
        let b = b.borrow();
        let a = a.value();
        let b = b.value();
        if core::ptr::addr_eq(&*a, &*b) {
            continue;
        }
        if a.ty() != b.ty() {
            return false;
        }
        if !check_equivalent(&*a, &*b) {
            return false;
        }
    }

    true
}
