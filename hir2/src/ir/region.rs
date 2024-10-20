mod branch_point;
mod interfaces;
mod invocation_bounds;
mod kind;
mod successor;
mod transforms;

use smallvec::SmallVec;

pub use self::{
    branch_point::RegionBranchPoint,
    interfaces::{
        RegionBranchOpInterface, RegionBranchTerminatorOpInterface, RegionKindInterface,
        RegionSuccessorIter,
    },
    invocation_bounds::InvocationBounds,
    kind::RegionKind,
    successor::{RegionSuccessor, RegionSuccessorInfo, RegionSuccessorMut},
    transforms::RegionTransformFailed,
};
use super::*;
use crate::RegionSimplificationLevel;

pub type RegionRef = UnsafeIntrusiveEntityRef<Region>;
/// An intrusive, doubly-linked list of [Region]s
pub type RegionList = EntityList<Region>;
/// A cursor in a [RegionList]
pub type RegionCursor<'a> = EntityCursor<'a, Region>;
/// A mutable cursor in a [RegionList]
pub type RegionCursorMut<'a> = EntityCursorMut<'a, Region>;

/// A region is a container for [Block], in one of two forms:
///
/// * Graph-like, in which the region consists of a single block, and the order of operations in
///   that block does not dictate any specific control flow semantics. It is up to the containing
///   operation to define.
/// * SSA-form, in which the region consists of one or more blocks that must obey the usual rules
///   of SSA dominance, and where operations in a block reflect the order in which those operations
///   are to be executed. Values defined by an operation must dominate any uses of those values in
///   the region.
///
/// The first block in a region is the _entry_ block, and its argument list corresponds to the
/// arguments expected by the region itself.
///
/// A region is only valid when it is attached to an [Operation], whereas the inverse is not true,
/// i.e. an operation without a parent region is a top-level operation, e.g. `Module`.
#[derive(Default)]
pub struct Region {
    /// The operation this region is attached to.
    ///
    /// If `link.is_linked() == true`, this will always be set to a valid pointer
    owner: Option<OperationRef>,
    /// The list of [Block]s that comprise this region
    body: BlockList,
}

impl Entity for Region {}
impl EntityWithParent for Region {
    type Parent = Operation;

    fn on_inserted_into_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().owner = Some(parent);
    }

    fn on_removed_from_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        _parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().owner = None;
    }

    fn on_transfered_to_new_parent(
        _from: UnsafeIntrusiveEntityRef<Self::Parent>,
        to: UnsafeIntrusiveEntityRef<Self::Parent>,
        transferred: impl IntoIterator<Item = UnsafeIntrusiveEntityRef<Self>>,
    ) {
        for mut transferred_region in transferred {
            transferred_region.borrow_mut().owner = Some(to.clone());
        }
    }
}

/// Blocks
impl Region {
    /// Returns true if this region is empty (has no blocks)
    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    /// Get a handle to the entry block for this region
    pub fn entry(&self) -> EntityRef<'_, Block> {
        self.body.front().into_borrow().unwrap()
    }

    /// Get a mutable handle to the entry block for this region
    pub fn entry_mut(&mut self) -> EntityMut<'_, Block> {
        self.body.front_mut().into_borrow_mut().unwrap()
    }

    /// Get the [BlockRef] of the entry block of this region, if it has one
    #[inline]
    pub fn entry_block_ref(&self) -> Option<BlockRef> {
        self.body.front().as_pointer()
    }

    /// Get the list of blocks comprising the body of this region
    pub fn body(&self) -> &BlockList {
        &self.body
    }

    /// Get a mutable reference to the list of blocks comprising the body of this region
    pub fn body_mut(&mut self) -> &mut BlockList {
        &mut self.body
    }
}

/// Metadata
impl Region {
    #[inline]
    pub fn as_region_ref(&self) -> RegionRef {
        unsafe { RegionRef::from_raw(self) }
    }

    /// Returns true if this region is an ancestor of `other`, i.e. it contains it.
    ///
    /// NOTE: This returns true if `self == other`, see [Self::is_proper_ancestor] if you do not
    /// want this behavior.
    pub fn is_ancestor(&self, other: &RegionRef) -> bool {
        let this = self.as_region_ref();
        &this == other || Self::is_proper_ancestor_of(&this, other)
    }

    /// Returns true if this region is a proper ancestor of `other`, i.e. `other` is contained by it
    ///
    /// NOTE: This returns false if `self == other`, see [Self::is_ancestor] if you do not want this
    /// behavior.
    pub fn is_proper_ancestor(&self, other: &RegionRef) -> bool {
        let this = self.as_region_ref();
        Self::is_proper_ancestor_of(&this, other)
    }

    fn is_proper_ancestor_of(this: &RegionRef, other: &RegionRef) -> bool {
        if this == other {
            return false;
        }

        let mut parent = other.borrow().parent_region();
        while let Some(parent_region) = parent.take() {
            if this == &parent_region {
                return true;
            }
            parent = parent_region.borrow().parent_region();
        }

        false
    }

    /// Returns true if this region may be a graph region without SSA dominance
    pub fn may_be_graph_region(&self) -> bool {
        if let Some(owner) = self.owner.as_ref() {
            owner
                .borrow()
                .as_trait::<dyn RegionKindInterface>()
                .is_some_and(|rki| rki.has_graph_regions())
        } else {
            true
        }
    }

    /// Returns true if this region has only one block
    pub fn has_one_block(&self) -> bool {
        !self.body.is_empty()
            && BlockRef::ptr_eq(
                &self.body.front().as_pointer().unwrap(),
                &self.body.back().as_pointer().unwrap(),
            )
    }

    /// Get the defining [Operation] for this region, if the region is attached to one.
    pub fn parent(&self) -> Option<OperationRef> {
        self.owner.clone()
    }

    /// Get the region which contains the parent operation of this region, if there is one.
    pub fn parent_region(&self) -> Option<RegionRef> {
        self.owner.as_ref().and_then(|op| op.borrow().parent_region())
    }

    /// Set the owner of this region.
    ///
    /// Returns the previous owner.
    ///
    /// # Safety
    ///
    /// It is dangerous to set this field unless doing so as part of allocating the [Region] or
    /// moving the [Region] from one op to another. If it is set to a different entity than actually
    /// owns the region, it will result in undefined behavior or panics when we attempt to access
    /// the owner via the region.
    ///
    /// You must ensure that the owner given _actually_ owns the region. Similarly, if you are
    /// unsetting the owner, you must ensure that no entity _thinks_ it owns this region.
    pub unsafe fn set_owner(&mut self, owner: Option<OperationRef>) -> Option<OperationRef> {
        match owner {
            None => self.owner.take(),
            Some(owner) => self.owner.replace(owner),
        }
    }
}

/// Mutation
impl Region {
    /// Push `block` to the start of this region
    #[inline]
    pub fn push_front(&mut self, block: BlockRef) {
        self.body.push_front(block);
    }

    /// Push `block` to the end of this region
    #[inline]
    pub fn push_back(&mut self, block: BlockRef) {
        self.body.push_back(block);
    }

    /// Drop any references to blocks in this region - this is used to break cycles when cleaning
    /// up regions.
    pub fn drop_all_references(&mut self) {
        let mut cursor = self.body_mut().front_mut();
        while let Some(mut op) = cursor.as_pointer() {
            op.borrow_mut().drop_all_references();
            cursor.move_next();
        }
    }
}

/// Values
impl Region {
    /// Check if every value in `values` is defined above this region, i.e. they are defined in a
    /// region which is a proper ancestor of `self`.
    pub fn values_are_defined_above(&self, values: &[ValueRef]) -> bool {
        let this = self.as_region_ref();
        for value in values {
            if !value
                .borrow()
                .parent_region()
                .is_some_and(|value_region| Self::is_proper_ancestor_of(&value_region, &this))
            {
                return false;
            }
        }
        true
    }

    /// Replace all uses of `value` with `replacement`, within this region.
    pub fn replace_all_uses_in_region_with(&mut self, _value: ValueRef, _replacement: ValueRef) {
        todo!("RegionUtils.h")
    }

    /// Visit each use of a value in this region (and its descendants), where that value was defined
    /// in an ancestor of `limit`.
    pub fn visit_used_values_defined_above<F>(&self, _limit: &RegionRef, _callback: F)
    where
        F: FnMut(OpOperand),
    {
        todo!("RegionUtils.h")
    }

    /// Visit each use of a value in any of the provided regions (or their descendants), where that
    /// value was defined in an ancestor of that region.
    pub fn visit_used_values_defined_above_any<F>(_regions: &[RegionRef], _callback: F)
    where
        F: FnMut(OpOperand),
    {
        todo!("RegionUtils.h")
    }

    /// Return a vector of values used in this region (and its descendants), and defined in an
    /// ancestor of the `limit` region.
    pub fn get_used_values_defined_above(&self, _limit: &RegionRef) -> SmallVec<[ValueRef; 1]> {
        todo!("RegionUtils.h")
    }

    /// Return a vector of values used in any of the provided regions, but defined in an ancestor.
    pub fn get_used_values_defined_above_any(_regions: &[RegionRef]) -> SmallVec<[ValueRef; 1]> {
        todo!("RegionUtils.h")
    }

    /// Make this region isolated from above.
    ///
    /// * Capture the values that are defined above the region and used within it.
    /// * Append block arguments to the entry block that represent each captured value.
    /// * Replace all uses of the captured values within the region, with the new block arguments
    /// * `clone_into_region` is called with the defining op of a captured value. If it returns
    ///   true, it indicates that the op needs to be cloned into the region. As a result, the
    ///   operands of that operation become part of the captured value set (unless the operations
    ///   that define the operand values themselves are to be cloned). The cloned operations are
    ///   added to the entry block of the region.
    ///
    /// Returns the set of captured values.
    pub fn make_isolated_from_above<R, F>(
        &mut self,
        _rewriter: &mut R,
        _clone_into_region: F,
    ) -> SmallVec<[ValueRef; 1]>
    where
        R: crate::Rewriter,
        F: Fn(&Operation) -> bool,
    {
        todo!("RegionUtils.h")
    }
}

/// Queries
impl Region {
    pub fn find_common_ancestor(ops: &[OperationRef]) -> Option<RegionRef> {
        use bitvec::prelude::*;

        match ops.len() {
            0 => None,
            1 => unsafe { ops.get_unchecked(0) }.borrow().parent_region(),
            num_ops => {
                let (first, rest) = unsafe { ops.split_first().unwrap_unchecked() };
                let mut region = first.borrow().parent_region();
                let mut remaining_ops = bitvec![1; num_ops - 1];
                while let Some(r) = region.take() {
                    while let Some(index) = remaining_ops.first_one() {
                        // Is this op contained in `region`?
                        if r.borrow().find_ancestor_op(&rest[index]).is_some() {
                            unsafe {
                                remaining_ops.set_unchecked(index, false);
                            }
                        }
                    }
                    if remaining_ops.not_any() {
                        break;
                    }
                    region = r.borrow().parent_region();
                }
                region
            }
        }
    }

    /// Returns `block` if `block` lies in this region, or otherwise finds the ancestor of `block`
    /// that lies in this region.
    ///
    /// Returns `None` if the latter fails.
    pub fn find_ancestor_block(&self, block: &BlockRef) -> Option<BlockRef> {
        let this = self.as_region_ref();
        let mut current = Some(block.clone());
        while let Some(current_block) = current.take() {
            let parent = current_block.borrow().parent()?;
            if parent == this {
                return Some(current_block);
            }
            current =
                parent.borrow().owner.as_ref().and_then(|parent_op| parent_op.borrow().parent());
        }
        current
    }

    /// Returns `op` if `op` lies in this region, or otherwise finds the ancestor of `op` that lies
    /// in this region.
    ///
    /// Returns `None` if the latter fails.
    pub fn find_ancestor_op(&self, op: &OperationRef) -> Option<OperationRef> {
        let this = self.as_region_ref();
        let mut current = Some(op.clone());
        while let Some(current_op) = current.take() {
            let parent = current_op.borrow().parent_region()?;
            if parent == this {
                return Some(current_op);
            }
            current = parent.borrow().parent();
        }
        current
    }
}

/// Transforms
impl Region {
    /// Run a set of structural simplifications over the regions in `regions`.
    ///
    /// This includes transformations like unreachable block elimination, dead argument elimination,
    /// as well as some other DCE.
    ///
    /// This function returns `Ok` if any of the regions were simplified, `Err` otherwise.
    ///
    /// The provided rewriter is used to notify callers of operation and block deletion.
    ///
    /// The provided [RegionSimplificationLevel] will be used to determine whether to apply more
    /// aggressive simplifications, namely block merging. Note that when block merging is enabled,
    /// this can lead to merged blocks with extra arguments.
    pub fn simplify_all(
        regions: &[RegionRef],
        rewriter: &mut dyn crate::Rewriter,
        simplification_level: RegionSimplificationLevel,
    ) -> Result<(), RegionTransformFailed> {
        let merge_blocks = matches!(simplification_level, RegionSimplificationLevel::Aggressive);

        let eliminated_blocks = Self::erase_unreachable_blocks(regions, rewriter).is_ok();
        let eliminated_ops_or_args = Self::dead_code_elimination(regions, rewriter).is_ok();

        let mut merged_identical_blocks = false;
        let mut dropped_redundant_arguments = false;
        if merge_blocks {
            merged_identical_blocks = Self::merge_identical_blocks(regions, rewriter).is_ok();
            dropped_redundant_arguments = Self::drop_redundant_arguments(regions, rewriter).is_ok();
        }

        if eliminated_blocks
            || eliminated_ops_or_args
            || merged_identical_blocks
            || dropped_redundant_arguments
        {
            Ok(())
        } else {
            Err(RegionTransformFailed)
        }
    }
}
