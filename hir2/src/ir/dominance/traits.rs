use super::{DominanceInfo, PostDominanceInfo};
use crate::{Block, BlockRef, Operation, Value};

/// This trait is implemented on a type which has a dominance relationship with `Rhs`.
pub trait Dominates<Rhs = Self> {
    /// Returns true if `self` dominates `other`.
    ///
    /// In cases where `Rhs = Self`, implementations should return true when `self == other`.
    ///
    /// For a stricter form of dominance, use [Dominates::properly_dominates].
    fn dominates(&self, other: &Rhs, dom_info: &DominanceInfo) -> bool;
    /// Returns true if `self` properly dominates `other`.
    ///
    /// In cases where `Rhs = Self`, implementations should return false when `self == other`.
    fn properly_dominates(&self, other: &Rhs, dom_info: &DominanceInfo) -> bool;
}

/// This trait is implemented on a type which has a post-dominance relationship with `Rhs`.
pub trait PostDominates<Rhs = Self> {
    /// Returns true if `self` post-dominates `other`.
    ///
    /// In cases where `Rhs = Self`, implementations should return true when `self == other`.
    ///
    /// For a stricter form of dominance, use [PostDominates::properly_dominates].
    fn post_dominates(&self, other: &Rhs, dom_info: &PostDominanceInfo) -> bool;
    /// Returns true if `self` properly post-dominates `other`.
    ///
    /// In cases where `Rhs = Self`, implementations should return false when `self == other`.
    fn properly_post_dominates(&self, other: &Rhs, dom_info: &PostDominanceInfo) -> bool;
}

/// The dominance relationship between two blocks.
impl Dominates for Block {
    /// Returns true if `a == b` or `a` properly dominates `b`.
    fn dominates(&self, other: &Self, dom_info: &DominanceInfo) -> bool {
        core::ptr::addr_eq(self, other) || self.properly_dominates(other, dom_info)
    }

    /// Returns true if `a != b` and:
    ///
    /// * `a` is an ancestor of `b`
    /// * The region containing `a` also contains `b` or some ancestor of `b`, and `a` dominates
    ///   that block in that kind of region.
    /// * In SSA regions, `a` properly dominates `b` if all control flow paths from the entry
    ///   block to `b`, flow through `a`.
    /// * In graph regions, all blocks dominate all other blocks.
    fn properly_dominates(&self, other: &Self, dom_info: &DominanceInfo) -> bool {
        dom_info.info().properly_dominates(self.as_block_ref(), other.as_block_ref())
    }
}

/// The dominance relationship between two blocks.
impl Dominates for BlockRef {
    /// Returns true if `a == b` or `a` properly dominates `b`.
    fn dominates(&self, other: &Self, dom_info: &DominanceInfo) -> bool {
        BlockRef::ptr_eq(self, other) || self.properly_dominates(other, dom_info)
    }

    /// Returns true if `a != b` and:
    ///
    /// * `a` is an ancestor of `b`
    /// * The region containing `a` also contains `b` or some ancestor of `b`, and `a` dominates
    ///   that block in that kind of region.
    /// * In SSA regions, `a` properly dominates `b` if all control flow paths from the entry
    ///   block to `b`, flow through `a`.
    /// * In graph regions, all blocks dominate all other blocks.
    fn properly_dominates(&self, other: &Self, dom_info: &DominanceInfo) -> bool {
        dom_info.info().properly_dominates(*self, *other)
    }
}

/// The post-dominance relationship between two blocks.
impl PostDominates for Block {
    fn post_dominates(&self, other: &Self, dom_info: &PostDominanceInfo) -> bool {
        core::ptr::addr_eq(self, other) || self.properly_post_dominates(other, dom_info)
    }

    /// Returns true if `a != b` and:
    ///
    /// * `a` is an ancestor of `b`
    /// * The region containing `a` also contains `b` or some ancestor of `b`, and `a` dominates
    ///   that block in that kind of region.
    /// * In SSA regions, `a` properly post-dominates `b` if all control flow paths from `b` to
    ///   an exit node, flow through `a`.
    /// * In graph regions, all blocks post-dominate all other blocks.
    fn properly_post_dominates(&self, other: &Self, dom_info: &PostDominanceInfo) -> bool {
        dom_info.info().properly_dominates(self.as_block_ref(), other.as_block_ref())
    }
}

/// The post-dominance relationship between two blocks.
impl PostDominates for BlockRef {
    fn post_dominates(&self, other: &Self, dom_info: &PostDominanceInfo) -> bool {
        BlockRef::ptr_eq(self, other) || self.properly_post_dominates(other, dom_info)
    }

    /// Returns true if `a != b` and:
    ///
    /// * `a` is an ancestor of `b`
    /// * The region containing `a` also contains `b` or some ancestor of `b`, and `a` dominates
    ///   that block in that kind of region.
    /// * In SSA regions, `a` properly post-dominates `b` if all control flow paths from `b` to
    ///   an exit node, flow through `a`.
    /// * In graph regions, all blocks post-dominate all other blocks.
    fn properly_post_dominates(&self, other: &Self, dom_info: &PostDominanceInfo) -> bool {
        dom_info.info().properly_dominates(*self, *other)
    }
}

/// The dominance relationship for operations
impl Dominates for Operation {
    fn dominates(&self, other: &Self, dom_info: &DominanceInfo) -> bool {
        core::ptr::addr_eq(self, other) || self.properly_dominates(other, dom_info)
    }

    /// Returns true if `a != b`, and:
    ///
    /// * `a` and `b` are in the same block, and `a` properly dominates `b` within the block, or
    /// * the block that contains `a` properly dominates the block that contains `b`.
    /// * `b` is enclosed in a region of `a`
    ///
    /// In any SSA region, `a` dominates `b` in the same block if `a` precedes `b`. In a graph
    /// region all operations in a block dominate all other operations in the same block.
    fn properly_dominates(&self, other: &Self, dom_info: &DominanceInfo) -> bool {
        let a = self.as_operation_ref();
        let b = other.as_operation_ref();
        dom_info.properly_dominates_with_options(a, b, /*enclosing_op_ok= */ true)
    }
}

/// The post-dominance relationship for operations
impl PostDominates for Operation {
    fn post_dominates(&self, other: &Self, dom_info: &PostDominanceInfo) -> bool {
        core::ptr::addr_eq(self, other) || self.properly_post_dominates(other, dom_info)
    }

    /// Returns true if `a != b`, and:
    ///
    /// * `a` and `b` are in the same block, and `a` properly post-dominates `b` within the block
    /// * the block that contains `a` properly post-dominates the block that contains `b`.
    /// * `b` is enclosed in a region of `a`
    ///
    /// In any SSA region, `a` post-dominates `b` in the same block if `b` precedes `a`. In a graph
    /// region all operations in a block post-dominate all other operations in the same block.
    fn properly_post_dominates(&self, other: &Self, dom_info: &PostDominanceInfo) -> bool {
        let a_block = self.parent().expect("`self` must be in a block");
        let mut b_block = other.parent().expect("`other` must be in a block");

        // An instruction post dominates, but does not properly post-dominate itself unless this is
        // a graph region.
        if core::ptr::addr_eq(self, other) {
            return !a_block.borrow().has_ssa_dominance();
        }

        // If these ops are in different regions, then normalize one into the other.
        let a_region = a_block.parent();
        let b_region = b_block.parent();
        let a = self.as_operation_ref();
        let mut b = other.as_operation_ref();
        if a_region != b_region {
            // Walk up `b`'s region tree until we find an operation in `a`'s region that encloses
            // it. If this fails, then we know there is no post-dominance relation.
            let Some(found) = a_region.as_ref().and_then(|r| r.borrow().find_ancestor_op(b)) else {
                return false;
            };
            b = found;
            b_block = b.parent().unwrap();
            assert!(b_block.parent() == a_region);

            // If `a` encloses `b`, then we consider it to post-dominate.
            if a == b {
                return true;
            }
        }

        // Ok, they are in the same region. If they are in the same block, check if `b` is before
        // `a` in the block.
        if a_block == b_block {
            // Dominance changes based on the region type
            return if a_block.borrow().has_ssa_dominance() {
                // If the blocks are the same, then check if `b` is before `a` in the block.
                b.borrow().is_before_in_block(&a)
            } else {
                true
            };
        }

        // If the blocks are different, check if `a`'s block post-dominates `b`'s
        dom_info
            .info()
            .dominance(a_region.unwrap())
            .properly_dominates(Some(a_block), Some(b_block))
    }
}

/// The dominance relationship between a value and an operation, e.g. between a definition of a
/// value and a user of that same value.
impl Dominates<Operation> for dyn Value {
    /// Return true if the definition of `self` dominates a use by operation `other`.
    fn dominates(&self, other: &Operation, dom_info: &DominanceInfo) -> bool {
        self.get_defining_op().is_some_and(|op| op == other.as_operation_ref())
            || self.properly_dominates(other, dom_info)
    }

    /// Returns true if the definition of `self` properly dominates `other`.
    ///
    /// This requires the value to either be a block argument, where the block containing `other`
    /// is dominated by the block defining `self`, OR that the value is an operation result, and
    /// the defining op of `self` properly dominates `other`.
    ///
    /// If the defining op of `self` encloses `b` in one of its regions, `a` does not dominate `b`.
    fn properly_dominates(&self, other: &Operation, dom_info: &DominanceInfo) -> bool {
        // Block arguments properly dominate all operations in their own block, so we use a
        // dominates check here, not a properly_dominates check.
        if let Some(block_arg) = self.downcast_ref::<crate::BlockArgument>() {
            return block_arg
                .owner()
                .borrow()
                .dominates(&other.parent().unwrap().borrow(), dom_info);
        }

        // `a` properly dominates `b` if the operation defining `a` properly dominates `b`, but `a`
        // does not itself enclose `b` in one of its regions.
        let defining_op = self.get_defining_op().unwrap();
        dom_info.properly_dominates_with_options(
            defining_op,
            other.as_operation_ref(),
            /*enclosing_op_ok= */ false,
        )
    }
}
