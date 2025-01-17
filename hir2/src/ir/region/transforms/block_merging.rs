#![allow(unused)]
use alloc::collections::BTreeMap;

use super::RegionTransformFailed;
use crate::{
    BlockArgument, BlockRef, DynHash, OpResult, Operation, OperationRef, Region, RegionRef,
    Rewriter, ValueRef,
};

bitflags::bitflags! {
    struct EquivalenceFlags: u8 {
        const IGNORE_LOCATIONS = 1;
    }
}

struct OpEquivalence<OperandHasher = DefaultValueHasher, ResultHasher = DefaultValueHasher> {
    flags: EquivalenceFlags,
    operand_hasher: OperandHasher,
    result_hasher: ResultHasher,
}

type ValueHasher = Box<dyn Fn(&ValueRef, &mut dyn core::hash::Hasher)>;

impl OpEquivalence {
    pub fn new() -> Self {
        Self {
            flags: EquivalenceFlags::empty(),
            operand_hasher: DefaultValueHasher,
            result_hasher: DefaultValueHasher,
        }
    }
}
impl<OperandHasher, ResultHasher> OpEquivalence<OperandHasher, ResultHasher> {
    #[inline]
    pub fn with_flags(mut self, flags: EquivalenceFlags) -> Self {
        self.flags.insert(flags);
        self
    }

    /// Ignore op operands when computing equivalence for operations
    pub fn ignore_operands(self) -> OpEquivalence<(), ResultHasher> {
        OpEquivalence {
            flags: self.flags,
            operand_hasher: (),
            result_hasher: self.result_hasher,
        }
    }

    /// Ignore op results when computing equivalence for operations
    pub fn ignore_results(self) -> OpEquivalence<OperandHasher, ()> {
        OpEquivalence {
            flags: self.flags,
            operand_hasher: self.operand_hasher,
            result_hasher: (),
        }
    }

    /// Specify a custom hasher for op operands
    pub fn with_operand_hasher(
        self,
        hasher: impl Fn(&ValueRef, &mut dyn core::hash::Hasher) + 'static,
    ) -> OpEquivalence<ValueHasher, ResultHasher> {
        OpEquivalence {
            flags: self.flags,
            operand_hasher: Box::new(hasher),
            result_hasher: self.result_hasher,
        }
    }

    /// Specify a custom hasher for op results
    pub fn with_result_hasher(
        self,
        hasher: impl Fn(&ValueRef, &mut dyn core::hash::Hasher) + 'static,
    ) -> OpEquivalence<OperandHasher, ValueHasher> {
        OpEquivalence {
            flags: self.flags,
            operand_hasher: self.operand_hasher,
            result_hasher: Box::new(hasher),
        }
    }

    /// Compare if two operations are equivalent using the current equivalence configuration.
    ///
    /// This is equivalent to calling [compute_equivalence] with `are_values_equivalent` set to
    /// `ValueRef::ptr_eq`, and `on_value_equivalence` to a no-op.
    #[inline]
    pub fn are_equivalent(&self, lhs: &OperationRef, rhs: &OperationRef) -> bool {
        #[inline(always)]
        fn noop(_: &ValueRef, _: &ValueRef) {}

        self.compute_equivalence(lhs, rhs, ValueRef::ptr_eq, noop)
    }

    /// Compare if two operations (and their regions) are equivalent using the current equivalence
    /// configuration.
    ///
    /// * `are_values_equivalent` is a callback used to check if two values are equivalent. For
    ///   two operations to be equivalent, their operands must be the same SSA value, or this
    ///   callback must return `true`.
    /// * `on_value_equivalence` is a callback to inform the caller that the analysis determined
    ///   that two values are equivalent.
    ///
    /// NOTE: Additional information regarding value equivalence can be injected into the analysis
    /// via `are_values_equivalent`. Typically, callers may want values that were recorded as
    /// equivalent via `on_value_equivalence` to be reflected in `are_values_equivalent`, but it
    /// depends on the exact semantics desired by the caller.
    pub fn compute_equivalence<VE, OVE>(
        &self,
        lhs: &OperationRef,
        rhs: &OperationRef,
        are_values_equivalent: VE,
        on_value_equivalence: OVE,
    ) -> bool
    where
        VE: Fn(&ValueRef, &ValueRef) -> bool,
        OVE: FnMut(&ValueRef, &ValueRef),
    {
        todo!()
    }

    /// Compare if two regions are equivalent using the current equivalence configuration.
    ///
    /// See [compute_equivalence] for more details.
    pub fn compute_region_equivalence<VE, OVE>(
        &self,
        lhs: &RegionRef,
        rhs: &RegionRef,
        are_values_equivalent: VE,
        on_value_equivalence: OVE,
    ) -> bool
    where
        VE: Fn(&ValueRef, &ValueRef) -> bool,
        OVE: FnMut(&ValueRef, &ValueRef),
    {
        todo!()
    }

    /// Hashes an operation based on:
    ///
    /// * OperationName
    /// * Attributes
    /// * Result types
    fn hash_operation(&self, op: &Operation, hasher: &mut impl core::hash::Hasher) {
        use core::hash::Hash;

        use crate::Value;

        op.name().hash(hasher);
        for attr in op.attributes().iter() {
            attr.hash(hasher);
        }
        for result in op.results().iter() {
            result.borrow().ty().hash(hasher);
        }
    }
}

#[inline(always)]
pub fn ignore_value_equivalence(_lhs: &ValueRef, _rhs: &ValueRef) -> bool {
    true
}

struct DefaultValueHasher;
impl FnOnce<(&ValueRef, &mut dyn core::hash::Hasher)> for DefaultValueHasher {
    type Output = ();

    extern "rust-call" fn call_once(
        self,
        args: (&ValueRef, &mut dyn core::hash::Hasher),
    ) -> Self::Output {
        use core::hash::Hash;

        let (value, hasher) = args;
        value.dyn_hash(hasher);
    }
}
impl FnMut<(&ValueRef, &mut dyn core::hash::Hasher)> for DefaultValueHasher {
    extern "rust-call" fn call_mut(
        &mut self,
        args: (&ValueRef, &mut dyn core::hash::Hasher),
    ) -> Self::Output {
        use core::hash::Hash;

        let (value, hasher) = args;
        value.dyn_hash(hasher);
    }
}
impl Fn<(&ValueRef, &mut dyn core::hash::Hasher)> for DefaultValueHasher {
    extern "rust-call" fn call(
        &self,
        args: (&ValueRef, &mut dyn core::hash::Hasher),
    ) -> Self::Output {
        use core::hash::Hash;

        let (value, hasher) = args;
        value.dyn_hash(hasher);
    }
}

struct BlockEquivalenceData {
    /// The block this data refers to
    block: BlockRef,
    /// The hash for this block
    hash: u64,
    /// A map of result producing operations to their relative orders within this block. The order
    /// of an operation is the number of defined values that are produced within the block before
    /// this operation.
    op_order_index: BTreeMap<OperationRef, u32>,
}
impl BlockEquivalenceData {
    pub fn new(block: BlockRef) -> Self {
        use core::hash::Hasher;

        let mut op_order_index = BTreeMap::default();

        let b = block.borrow();
        let mut order = b.num_arguments() as u32;
        let mut op_equivalence = OpEquivalence::new()
            .with_flags(EquivalenceFlags::IGNORE_LOCATIONS)
            .ignore_operands()
            .ignore_results();

        let mut hasher = rustc_hash::FxHasher::default();
        for op in b.body() {
            let num_results = op.num_results() as u32;
            if num_results > 0 {
                op_order_index.insert(op.as_operation_ref(), order);
                order += num_results;
            }
            op_equivalence.hash_operation(&op, &mut hasher);
        }

        Self {
            block,
            hash: hasher.finish(),
            op_order_index,
        }
    }

    fn get_order_of(&self, value: &ValueRef) -> usize {
        let value = value.borrow();
        assert!(value.parent_block().unwrap() == self.block, "expected value of this block");

        if let Some(block_arg) = value.downcast_ref::<BlockArgument>() {
            return block_arg.index();
        }

        let result = value.downcast_ref::<OpResult>().unwrap();
        let order =
            *self.op_order_index.get(&result.owner()).expect("expected op to have an order");
        result.index() + (order as usize)
    }
}

impl Region {
    // TODO(pauls)
    pub(in crate::ir::region) fn merge_identical_blocks(
        _regions: &[RegionRef],
        _rewriter: &mut dyn Rewriter,
    ) -> Result<(), RegionTransformFailed> {
        Err(RegionTransformFailed)
    }
}
