use core::hash::{Hash, Hasher};

use bitflags::bitflags;
use smallvec::SmallVec;

use super::Operation;
use crate::{OpOperand, Region, Value, ValueRef, traits::Commutative};

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

/// A strategy for hashing an SSA value as part of operation equivalence hashing.
///
/// # Pairing with [ValueEquivalence]
///
/// Every implementation of this trait must be mirrored by a semantically equivalent
/// [ValueEquivalence] implementation, and vice versa: whenever the paired equivalence considers
/// two values equivalent, the hasher must write identical data for both. Using mismatched
/// implementations in the `Hash` and `Eq` of a hash-map key breaks the `Hash`/`Eq` contract:
/// equal keys land in different buckets, lookups miss depending on allocation addresses, and
/// compiler output becomes non-deterministic
/// (see https://github.com/0xMiden/compiler/issues/1257).
///
/// The canonical pairs are:
///
/// - [DefaultValueHasher] ↔ [DefaultValueEquivalence]: value identity
/// - [ValueTypeHasher] ↔ [ValueTypeEquivalence]: value type only
/// - [IgnoreValueHasher] ↔ [IgnoreValueEquivalence]: values ignored entirely
pub trait ValueHasher {
    fn hash_value<H: Hasher>(&self, value: ValueRef, hasher: &mut H);
}

/// A [ValueHasher] impl that hashes a value based on its address in memory.
///
/// This is generally used with [OperationHasher] to require operands/results between two
/// operations to be exactly the same. Pairs with [DefaultValueEquivalence].
#[derive(Default)]
pub struct DefaultValueHasher;

impl ValueHasher for DefaultValueHasher {
    fn hash_value<H: Hasher>(&self, value: ValueRef, hasher: &mut H) {
        // Hash only the address, discarding the fat pointer metadata: equivalence checks compare
        // values with `core::ptr::addr_eq`, and the vtable pointer of the same value can differ
        // between codegen units, which would make equal keys hash differently.
        ValueRef::as_ptr(&value).addr().hash(hasher);
    }
}

/// A [ValueHasher] impl that hashes a value based only on its type.
///
/// Pairs with [ValueTypeEquivalence].
#[derive(Default)]
pub struct ValueTypeHasher;

impl ValueHasher for ValueTypeHasher {
    fn hash_value<H: Hasher>(&self, value: ValueRef, hasher: &mut H) {
        value.borrow().ty().hash(hasher);
    }
}

/// A [ValueHasher] impl that ignores operands/results, i.e. the hash is unchanged
///
/// Pairs with [IgnoreValueEquivalence].
#[derive(Default)]
pub struct IgnoreValueHasher;

impl ValueHasher for IgnoreValueHasher {
    fn hash_value<H: Hasher>(&self, _value: ValueRef, _hasher: &mut H) {}
}

/// A strategy for deciding whether two SSA values are equivalent as part of operation
/// equivalence checks.
///
/// # Pairing with [ValueHasher]
///
/// Every implementation of this trait must be mirrored by a semantically equivalent
/// [ValueHasher] implementation, and vice versa: whenever `is_equivalent` holds for two values,
/// the paired hasher must write identical data for both. Using mismatched implementations in
/// the `Hash` and `Eq` of a hash-map key breaks the `Hash`/`Eq` contract: equal keys land in
/// different buckets, lookups miss depending on allocation addresses, and compiler output
/// becomes non-deterministic (see https://github.com/0xMiden/compiler/issues/1257).
///
/// The canonical pairs are:
///
/// - [DefaultValueHasher] ↔ [DefaultValueEquivalence]: value identity
/// - [ValueTypeHasher] ↔ [ValueTypeEquivalence]: value type only
/// - [IgnoreValueHasher] ↔ [IgnoreValueEquivalence]: values ignored entirely
pub trait ValueEquivalence {
    fn is_equivalent(&self, lhs: &dyn Value, rhs: &dyn Value) -> bool;
}

impl<F> ValueEquivalence for F
where
    F: Fn(&dyn Value, &dyn Value) -> bool,
{
    #[inline]
    fn is_equivalent(&self, lhs: &dyn Value, rhs: &dyn Value) -> bool {
        self(lhs, rhs)
    }
}

/// A [ValueEquivalence] impl that compares values by their address in memory, i.e. two values
/// are equivalent if and only if they are the same value.
///
/// Pairs with [DefaultValueHasher].
#[derive(Default)]
pub struct DefaultValueEquivalence;

impl ValueEquivalence for DefaultValueEquivalence {
    fn is_equivalent(&self, lhs: &dyn Value, rhs: &dyn Value) -> bool {
        core::ptr::addr_eq(lhs, rhs)
    }
}

/// A [ValueEquivalence] impl under which values are equivalent if and only if they have the
/// same type, regardless of their identity.
///
/// Pairs with [ValueTypeHasher].
#[derive(Default)]
pub struct ValueTypeEquivalence;

impl ValueEquivalence for ValueTypeEquivalence {
    fn is_equivalent(&self, lhs: &dyn Value, rhs: &dyn Value) -> bool {
        lhs.ty() == rhs.ty()
    }
}

/// A [ValueEquivalence] impl that considers all values equivalent, regardless of their identity
/// or even their type.
///
/// Pairs with [IgnoreValueHasher].
#[derive(Default)]
pub struct IgnoreValueEquivalence;

impl ValueEquivalence for IgnoreValueEquivalence {
    fn is_equivalent(&self, _lhs: &dyn Value, _rhs: &dyn Value) -> bool {
        true
    }
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
        // Hash operations based upon their:
        //
        // - Operation name
        // - Result types
        // - Properties
        // - Attributes
        self.name.hash(hasher);
        for result in self.results().iter() {
            let result = result.borrow();
            result.ty().hash(hasher);
        }
        for prop in self.properties() {
            prop.hash(hasher);
        }
        self.attrs.hash(hasher);

        if !flags.contains(OperationEquivalenceFlags::IGNORE_LOCATIONS) {
            self.span.hash(hasher);
        }

        // Operands
        //
        // Commutative operations are equivalent regardless of operand order, so their operands
        // must be hashed in the same canonical order used by `is_equivalent_with_options`, or
        // equal keys could hash differently.
        self.operands().len().hash(hasher);
        if self.implements::<dyn Commutative>() {
            let operands = self.operands().all();
            let mut operands = SmallVec::<[_; 2]>::from_slice(operands.as_slice());
            operands.sort_by(sort_operands);
            for operand in operands {
                let operand = operand.borrow();
                operand_hasher.hash_value(operand.as_value_ref(), hasher);
            }
        } else {
            for operand in self.operands().iter() {
                let operand = operand.borrow();
                operand_hasher.hash_value(operand.as_value_ref(), hasher);
            }
        }

        // Results
        self.results().len().hash(hasher);
        for result in self.results().iter() {
            let result = result.borrow();
            result_hasher.hash_value(result.as_value_ref(), hasher);
        }
    }

    pub fn is_equivalent(&self, rhs: &Operation, flags: OperationEquivalenceFlags) -> bool {
        self.is_equivalent_with_options(rhs, flags, DefaultValueEquivalence)
    }

    pub fn is_equivalent_with_options(
        &self,
        rhs: &Operation,
        flags: OperationEquivalenceFlags,
        value_equivalence: impl ValueEquivalence,
    ) -> bool {
        if core::ptr::addr_eq(self, rhs) {
            return true;
        }

        // 1. Compare operation properties
        if self.name != rhs.name
            || self.num_regions() != rhs.num_regions()
            || self.num_successors() != rhs.num_successors()
            || self.num_operands() != rhs.num_operands()
            || self.num_results() != rhs.num_results()
            || !self.properties().eq(rhs.properties())
            || self.attributes() != rhs.attributes()
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
            if !are_operands_equivalent(&lhs_operands, &rhs_operands, &value_equivalence) {
                return false;
            }
        } else {
            // Check pair-wise for equivalence
            let lhs = self.operands.all();
            let rhs = rhs.operands.all();
            if !are_operands_equivalent(lhs.as_slice(), rhs.as_slice(), &value_equivalence) {
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
            if !is_region_equivalent_to(&lhs_region, &rhs_region, flags, &value_equivalence) {
                return false;
            }
        }

        true
    }
}

fn is_region_equivalent_to<VE>(
    _lhs: &Region,
    _rhs: &Region,
    _flags: OperationEquivalenceFlags,
    _value_equivalence: &VE,
) -> bool
where
    VE: ValueEquivalence + ?Sized,
{
    todo!()
}

/// Orders operands canonically (by value address) for commutative operand comparison and
/// hashing.
///
/// NOTE: address order is only a sound canonicalization for identity-based value equivalence
/// ([DefaultValueEquivalence]/[DefaultValueHasher]), where equal operand sets sort identically
/// on both sides. A commutative operation compared under an equivalence that ignores identity
/// (e.g. [ValueTypeEquivalence]) would need a structural canonicalization instead; no such
/// caller exists today.
fn sort_operands(a: &OpOperand, b: &OpOperand) -> core::cmp::Ordering {
    let a = a.borrow().as_value_ref();
    let b = b.borrow().as_value_ref();
    let a = ValueRef::as_ptr(&a).addr();
    let b = ValueRef::as_ptr(&b).addr();
    a.cmp(&b)
}

fn are_operands_equivalent<VE>(a: &[OpOperand], b: &[OpOperand], value_equivalence: &VE) -> bool
where
    VE: ValueEquivalence + ?Sized,
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
        if !value_equivalence.is_equivalent(&*a, &*b) {
            return false;
        }
    }

    true
}
