use super::WalkResult;
use crate::{BlockRef, Operation, OperationRef, Region, RegionRef, UnsafeIntrusiveEntityRef};

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
    next_region: Option<RegionRef>,
}
impl WalkStage {
    pub fn new(op: OperationRef) -> Self {
        let op = op.borrow();
        Self {
            num_regions: op.num_regions(),
            next_region: op.regions().front().as_pointer(),
        }
    }

    /// Returns true if the parent operation is being visited before all regions.
    #[inline]
    pub fn is_before_all_regions(&self) -> bool {
        self.next_region.is_some_and(|r| r.prev().is_none())
    }

    /// Returns true if the parent operation is being visited just before visiting `region`
    #[inline]
    pub fn is_before_region(&self, region: RegionRef) -> bool {
        self.next_region.is_some_and(|r| r.next().is_some_and(|next| next == region))
    }

    /// Returns true if the parent operation is being visited just after visiting `region`
    #[inline]
    pub fn is_after_region(&self, region: RegionRef) -> bool {
        self.next_region.is_some_and(|r| r.prev().is_some_and(|prev| prev == region))
    }

    /// Returns true if the parent operation is being visited after all regions.
    #[inline]
    pub fn is_after_all_regions(&self) -> bool {
        self.next_region.is_none()
    }

    /// Advance the walk stage
    #[inline]
    pub fn advance(&mut self) {
        if let Some(next_region) = self.next_region.take() {
            self.next_region = next_region.next();
        }
    }

    /// Returns the next region that will be visited
    #[inline(always)]
    pub const fn next_region(&self) -> Option<RegionRef> {
        self.next_region
    }
}

/// [Walk] represents an implementation of a depth-first traversal (pre- or post-order) from some
/// root object in the entity graph, to children of a given entity type.
///
/// An implementation of this trait specifies a type, `T`, corresponding to the type of item being
/// walked, while `Self` is the root entity, possibly of the same type, which may contain `T`. Thus
/// traversing from the root to all of the leaves, we will visit all reachable `T` nested within
/// `Self`, possibly including itself.
///
/// In cases where the root entity and the entity type being visited are the same, callbacks given
/// to this trait's methods are invoked on both the root entity and any children of that type. This
/// would require re-borrowing the root entity, so to distinguish between immutable and mutable
/// visits, this trait has a mutable variant, [WalkMut], which ensures that the root entity is not
/// borrowed during the traversal, and thus can be mutably borrowed by the visitor if needed.
pub trait Walk<T> {
    /// Walk all `T` in `self` in a specific order, applying the given callback to each.
    ///
    /// This is very similar to [Walkable::walk_interruptible], except the callback has no control
    /// over the traversal, and must be infallible.
    fn walk_all<F>(&self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(&T),
    {
        let _ = self.walk(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn prewalk_all<F>(&self, callback: F)
    where
        F: FnMut(&T),
    {
        self.walk_all(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn postwalk_all<F>(&self, callback: F)
    where
        F: FnMut(&T),
    {
        self.walk_all(WalkOrder::PostOrder, callback)
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    fn walk<F, B>(&self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        F: FnMut(&T) -> WalkResult<B>;

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn prewalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&T) -> WalkResult<B>,
    {
        self.walk(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn postwalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&T) -> WalkResult<B>,
    {
        self.walk(WalkOrder::PostOrder, callback)
    }
}

/// A mutable variant of [Walk], for traversal which may mutate visited entities.
pub trait WalkMut<T> {
    /// Walk all `T` in `self` in a specific order, applying the given callback to each.
    ///
    /// This is very similar to [Walkable::walk_interruptible], except the callback has no control
    /// over the traversal, and must be infallible.
    fn walk_all_mut<F>(&mut self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(&mut T),
    {
        let _ = self.walk_mut(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn prewalk_all_mut<F>(&mut self, callback: F)
    where
        F: FnMut(&mut T),
    {
        self.walk_all_mut(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn postwalk_all_mut<F>(&mut self, callback: F)
    where
        F: FnMut(&mut T),
    {
        self.walk_all_mut(WalkOrder::PostOrder, callback)
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    fn walk_mut<F, B>(&mut self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut T) -> WalkResult<B>;

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn prewalk_mut_interruptible<F, B>(&mut self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut T) -> WalkResult<B>,
    {
        self.walk_mut(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn postwalk_mut_interruptible<F, B>(&mut self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut T) -> WalkResult<B>,
    {
        self.walk_mut(WalkOrder::PostOrder, callback)
    }
}

/// [RawWalk] is a variation of [Walk]/[WalkMut] that performs the traversal while ensuring that
/// no entity is borrowed when visitor callbacks are invoked. This allows the visitor to freely
/// obtain mutable/immutable borrows without having to worry if the traversal is holding a borrow
/// somewhere.
pub trait RawWalk<T> {
    /// Walk all `T` in `self` in a specific order, applying the given callback to each.
    ///
    /// This is very similar to [Walkable::walk_interruptible], except the callback has no control
    /// over the traversal, and must be infallible.
    fn raw_walk_all<F>(&self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.raw_walk(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn raw_prewalk_all<F>(&self, callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        self.raw_walk_all(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn raw_postwalk_all<F>(&self, callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        self.raw_walk_all(WalkOrder::PostOrder, callback)
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    fn raw_walk<F, B>(&self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>;

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn raw_prewalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>,
    {
        self.raw_walk(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn raw_postwalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>,
    {
        self.raw_walk(WalkOrder::PostOrder, callback)
    }
}

/// Walking operations nested within an [Operation], including itself
impl RawWalk<Operation> for OperationRef {
    fn raw_walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<Operation>) -> WalkResult<B>,
    {
        raw_walk_operations(*self, order, &mut callback)
    }
}

impl Walk<Operation> for OperationRef {
    fn walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_operations(*self, order, &mut wrapper)
    }
}

impl WalkMut<Operation> for OperationRef {
    fn walk_mut<F, B>(&mut self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        let mut wrapper = |mut op: OperationRef| callback(&mut op.borrow_mut());
        raw_walk_operations(*self, order, &mut wrapper)
    }
}

impl Walk<Operation> for Operation {
    fn walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_operations(self.as_operation_ref(), order, &mut wrapper)
    }
}

/// Walking regions of an [Operation], and those of all nested operations
impl RawWalk<Region> for OperationRef {
    fn raw_walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<Region>) -> WalkResult<B>,
    {
        raw_walk_regions(*self, order, &mut callback)
    }
}

impl Walk<Region> for OperationRef {
    fn walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Region) -> WalkResult<B>,
    {
        let mut wrapper = |region: RegionRef| callback(&region.borrow());
        raw_walk_regions(*self, order, &mut wrapper)
    }
}

impl WalkMut<Region> for OperationRef {
    fn walk_mut<F, B>(&mut self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Region) -> WalkResult<B>,
    {
        let mut wrapper = |mut region: RegionRef| callback(&mut region.borrow_mut());
        raw_walk_regions(*self, order, &mut wrapper)
    }
}

impl Walk<Region> for Operation {
    fn walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Region) -> WalkResult<B>,
    {
        let mut wrapper = |region: RegionRef| callback(&region.borrow());
        raw_walk_regions(self.as_operation_ref(), order, &mut wrapper)
    }
}

#[allow(unused)]
pub fn raw_walk<F, B>(op: OperationRef, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef, &WalkStage) -> WalkResult<B>,
{
    let mut stage = WalkStage::new(op);

    let mut next_region = stage.next_region();
    while let Some(region) = next_region.take() {
        // Invoke callback on the parent op before visiting each child region
        let result = callback(op, &stage);

        match result {
            WalkResult::Skip => return WalkResult::Continue(()),
            err @ WalkResult::Break(_) => return err,
            WalkResult::Continue(_) => {
                stage.advance();

                let mut next_block = region.borrow().body().front().as_pointer();
                while let Some(block) = next_block.take() {
                    next_block = block.next();

                    let mut next_op = block.borrow().body().front().as_pointer();
                    while let Some(op) = next_op.take() {
                        next_op = op.next();

                        raw_walk(op, callback)?;
                    }
                }
            }
        }
    }

    // Invoke callback after all regions have been visited
    callback(op, &stage)
}

fn raw_walk_regions<F, B>(op: OperationRef, order: WalkOrder, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(RegionRef) -> WalkResult<B>,
{
    let mut next_region = op.borrow().regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        if matches!(order, WalkOrder::PreOrder) {
            let result = callback(region);
            match result {
                WalkResult::Skip => continue,
                err @ WalkResult::Break(_) => return err,
                _ => (),
            }
        }

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(op) = next_op.take() {
                next_op = op.next();

                raw_walk_regions(op, order, callback)?;
            }
        }

        if matches!(order, WalkOrder::PostOrder) {
            callback(region)?;
        }
    }

    WalkResult::Continue(())
}

#[allow(unused)]
fn raw_walk_blocks<F, B>(op: OperationRef, order: WalkOrder, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(BlockRef) -> WalkResult<B>,
{
    let mut next_region = op.borrow().regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            if matches!(order, WalkOrder::PreOrder) {
                let result = callback(block);
                match result {
                    WalkResult::Skip => continue,
                    err @ WalkResult::Break(_) => return err,
                    _ => (),
                }
            }

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(op) = next_op.take() {
                next_op = op.next();

                raw_walk_blocks(op, order, callback)?;
            }

            if matches!(order, WalkOrder::PostOrder) {
                callback(block)?;
            }
        }
    }

    WalkResult::Continue(())
}

fn raw_walk_operations<F, B>(op: OperationRef, order: WalkOrder, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    if matches!(order, WalkOrder::PreOrder) {
        let result = callback(op);
        match result {
            WalkResult::Skip => return WalkResult::Continue(()),
            err @ WalkResult::Break(_) => return err,
            _ => (),
        }
    }

    let mut next_region = op.borrow().regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(op) = next_op.take() {
                next_op = op.next();

                raw_walk_operations(op, order, callback)?;
            }
        }
    }

    if matches!(order, WalkOrder::PostOrder) {
        callback(op)?;
    }

    WalkResult::Continue(())
}

fn raw_walk_region_operations<F, B>(
    region: RegionRef,
    order: WalkOrder,
    callback: &mut F,
) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    let mut next_block = region.borrow().body().front().as_pointer();
    while let Some(block) = next_block.take() {
        next_block = block.next();

        let mut next_op = block.borrow().body().front().as_pointer();
        while let Some(op) = next_op.take() {
            next_op = op.next();

            raw_walk_operations(op, order, callback)?;
        }
    }

    WalkResult::Continue(())
}

/// Walking operations nested within a [Region]
impl RawWalk<Operation> for RegionRef {
    fn raw_walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<Operation>) -> WalkResult<B>,
    {
        raw_walk_region_operations(*self, order, &mut callback)
    }
}

impl Walk<Operation> for RegionRef {
    fn walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_region_operations(*self, order, &mut wrapper)
    }
}

impl WalkMut<Operation> for RegionRef {
    fn walk_mut<F, B>(&mut self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        let mut wrapper = |mut op: OperationRef| callback(&mut op.borrow_mut());
        raw_walk_region_operations(*self, order, &mut wrapper)
    }
}

impl Walk<Operation> for Region {
    fn walk<F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_region_operations(self.as_region_ref(), order, &mut wrapper)
    }
}
