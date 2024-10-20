use super::WalkResult;
use crate::{
    Block, BlockRef, Operation, OperationRef, Region, RegionRef, UnsafeIntrusiveEntityRef,
};

/// The traversal order for a walk of a region, block, or operation
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WalkOrder {
    PreOrder,
    PostOrder,
}

/// Encodes the current walk stage for generic walkers.
///
/// When walking an operation, we can either choose a pre- or post-traversal walker which invokes
/// the callback on an operation before/after all its attached regions have been visited, or choose
/// a generic walker where the callback is invoked on the operation N+1 times, where N is the number
/// of regions attached to that operation. [WalkStage] encodes the current stage of the walk, i.e.
/// which regions have already been visited, and the callback accepts an additional argument for
/// the current stage. Such generic walkers that accept stage-aware callbacks are only applicable
/// when the callback operations on an operation (i.e. doesn't apply to callbacks on blocks or
/// regions).
#[derive(Clone, PartialEq, Eq)]
pub struct WalkStage {
    /// The number of regions in the operation
    num_regions: usize,
    /// The next region to visit in the operation
    next_region: usize,
}
impl WalkStage {
    pub fn new(op: OperationRef) -> Self {
        let op = op.borrow();
        Self {
            num_regions: op.num_regions(),
            next_region: 0,
        }
    }

    /// Returns true if the parent operation is being visited before all regions.
    #[inline]
    pub fn is_before_all_regions(&self) -> bool {
        self.next_region == 0
    }

    /// Returns true if the parent operation is being visited just before visiting `region`
    #[inline]
    pub fn is_before_region(&self, region: usize) -> bool {
        self.next_region == region
    }

    /// Returns true if the parent operation is being visited just after visiting `region`
    #[inline]
    pub fn is_after_region(&self, region: usize) -> bool {
        self.next_region == region + 1
    }

    /// Returns true if the parent operation is being visited after all regions.
    #[inline]
    pub fn is_after_all_regions(&self) -> bool {
        self.next_region == self.num_regions
    }

    /// Advance the walk stage
    #[inline]
    pub fn advance(&mut self) {
        self.next_region += 1;
    }

    /// Returns the next region that will be visited
    #[inline(always)]
    pub const fn next_region(&self) -> usize {
        self.next_region
    }
}

/// A [Walkable] is an entity which can be traversed depth-first in either pre- or post-order
///
/// An implementation of this trait specifies a type, `T`, corresponding to the type of item being
/// walked, while `Self` is the root entity, possibly of the same type, which may contain `T`. Thus
/// traversing from the root to all of the leaves, we will visit all reachable `T` nested within
/// `Self`, possibly including itself.
pub trait Walkable<T> {
    /// Walk all `T` in `self` in a specific order, applying the given callback to each.
    ///
    /// This is very similar to [Walkable::walk_interruptible], except the callback has no control
    /// over the traversal, and must be infallible.
    #[inline]
    fn walk<F>(&self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.walk_interruptible(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn prewalk<F>(&self, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.prewalk_interruptible(|t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn postwalk<F>(&self, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.postwalk_interruptible(|t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    #[inline]
    fn walk_interruptible<F, B>(&self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>,
    {
        match order {
            WalkOrder::PreOrder => self.prewalk_interruptible(callback),
            WalkOrder::PostOrder => self.prewalk_interruptible(callback),
        }
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn prewalk_interruptible<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>;

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn postwalk_interruptible<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>;
}

/// Walking operations nested within an [Operation], including itself
impl Walkable<Operation> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(OperationRef) -> WalkResult<B>,
    {
        prewalk_operation_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(OperationRef) -> WalkResult<B>,
    {
        postwalk_operation_interruptible(self, &mut callback)
    }
}

fn prewalk_operation_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    let result = callback(op.as_operation_ref());
    if !result.should_continue() {
        return result;
    }

    for region in op.regions().iter() {
        for block in region.body().iter() {
            let mut ops = block.body().front();
            while let Some(op) = ops.as_pointer() {
                ops.move_next();
                let op = op.borrow();
                prewalk_operation_interruptible(&op, callback)?;
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_operation_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    for region in op.regions().iter() {
        for block in region.body().iter() {
            let mut ops = block.body().front();
            while let Some(op) = ops.as_pointer() {
                ops.move_next();
                let op = op.borrow();
                postwalk_operation_interruptible(&op, callback)?;
            }
        }
    }

    callback(op.as_operation_ref())
}

/// Walking regions of an [Operation], and those of all nested operations
impl Walkable<Region> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(RegionRef) -> WalkResult<B>,
    {
        prewalk_regions_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(RegionRef) -> WalkResult<B>,
    {
        postwalk_regions_interruptible(self, &mut callback)
    }
}

fn prewalk_regions_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(RegionRef) -> WalkResult<B>,
{
    let mut regions = op.regions().front();
    while let Some(region) = regions.as_pointer() {
        regions.move_next();
        match callback(region.clone()) {
            WalkResult::Continue(_) => {
                let region = region.borrow();
                for block in region.body().iter() {
                    for op in block.body().iter() {
                        prewalk_regions_interruptible(&op, callback)?;
                    }
                }
            }
            WalkResult::Skip => continue,
            result @ WalkResult::Break(_) => return result,
        }
    }

    WalkResult::Continue(())
}

fn postwalk_regions_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(RegionRef) -> WalkResult<B>,
{
    let mut regions = op.regions().front();
    while let Some(region) = regions.as_pointer() {
        regions.move_next();
        {
            let region = region.borrow();
            for block in region.body().iter() {
                for op in block.body().iter() {
                    postwalk_regions_interruptible(&op, callback)?;
                }
            }
        }
        callback(region)?;
    }

    WalkResult::Continue(())
}

/// Walking operations nested within a [Region]
impl Walkable<Operation> for Region {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(OperationRef) -> WalkResult<B>,
    {
        prewalk_region_operations_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(OperationRef) -> WalkResult<B>,
    {
        postwalk_region_operations_interruptible(self, &mut callback)
    }
}

fn prewalk_region_operations_interruptible<F, B>(region: &Region, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    for block in region.body().iter() {
        let mut cursor = block.body().front();
        while let Some(op) = cursor.as_pointer() {
            cursor.move_next();
            match callback(op.clone()) {
                WalkResult::Continue(_) => {
                    let op = op.borrow();
                    for region in op.regions() {
                        prewalk_region_operations_interruptible(&region, callback)?;
                    }
                }
                WalkResult::Skip => continue,
                result @ WalkResult::Break(_) => return result,
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_region_operations_interruptible<F, B>(
    region: &Region,
    callback: &mut F,
) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    for block in region.body().iter() {
        let mut cursor = block.body().front();
        while let Some(op) = cursor.as_pointer() {
            cursor.move_next();
            {
                let op = op.borrow();
                for region in op.regions() {
                    postwalk_region_operations_interruptible(&region, callback)?;
                }
            }
            callback(op)?;
        }
    }

    WalkResult::Continue(())
}

/// Walking blocks of an [Operation], and those of all nested operations
impl Walkable<Block> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(BlockRef) -> WalkResult<B>,
    {
        prewalk_blocks_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(BlockRef) -> WalkResult<B>,
    {
        postwalk_blocks_interruptible(self, &mut callback)
    }
}

fn prewalk_blocks_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(BlockRef) -> WalkResult<B>,
{
    for region in op.regions().iter() {
        let mut blocks = region.body().front();
        while let Some(block) = blocks.as_pointer() {
            blocks.move_next();
            match callback(block.clone()) {
                WalkResult::Continue(_) => {
                    let block = block.borrow();
                    for op in block.body().iter() {
                        prewalk_blocks_interruptible(&op, callback)?;
                    }
                }
                WalkResult::Skip => continue,
                result @ WalkResult::Break(_) => return result,
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_blocks_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(BlockRef) -> WalkResult<B>,
{
    for region in op.regions().iter() {
        let mut blocks = region.body().front();
        while let Some(block) = blocks.as_pointer() {
            blocks.move_next();
            {
                let block = block.borrow();
                for op in block.body().iter() {
                    postwalk_blocks_interruptible(&op, callback)?;
                }
            }
            callback(block.clone())?;
        }
    }

    WalkResult::Continue(())
}
