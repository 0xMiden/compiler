use super::WalkResult;
use crate::{Block, Operation, OperationRef, Region, RegionRef, UnsafeIntrusiveEntityRef};

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
    #[inline]
    fn walk<F>(&self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(&T),
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
        F: FnMut(&T),
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
        F: FnMut(&T),
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
        F: FnMut(&T) -> WalkResult<B>,
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
        F: FnMut(&T) -> WalkResult<B>;

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn postwalk_interruptible<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&T) -> WalkResult<B>;
}

/// A mutable variant of [Walk], for traversal which may mutate visited entities.
pub trait WalkMut<T> {
    /// Walk all `T` in `self` in a specific order, applying the given callback to each.
    ///
    /// This is very similar to [Walkable::walk_interruptible], except the callback has no control
    /// over the traversal, and must be infallible.
    #[inline]
    fn walk_mut<F>(&mut self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(&mut T),
    {
        let _ = self.walk_mut_interruptible(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn prewalk_mut<F>(&mut self, mut callback: F)
    where
        F: FnMut(&mut T),
    {
        let _ = self.prewalk_mut_interruptible(|t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn postwalk_mut<F>(&mut self, mut callback: F)
    where
        F: FnMut(&mut T),
    {
        let _ = self.postwalk_mut_interruptible(|t| {
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
    fn walk_mut_interruptible<F, B>(&mut self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut T) -> WalkResult<B>,
    {
        match order {
            WalkOrder::PreOrder => self.prewalk_mut_interruptible(callback),
            WalkOrder::PostOrder => self.prewalk_mut_interruptible(callback),
        }
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn prewalk_mut_interruptible<F, B>(&mut self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut T) -> WalkResult<B>;

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn postwalk_mut_interruptible<F, B>(&mut self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut T) -> WalkResult<B>;
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
    #[inline]
    fn raw_walk<F>(&self, order: WalkOrder, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.raw_walk_interruptible(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn raw_prewalk<F>(&self, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.raw_prewalk_interruptible(|t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn raw_postwalk<F>(&self, mut callback: F)
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.raw_postwalk_interruptible(|t| {
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
    fn raw_walk_interruptible<F, B>(&self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>,
    {
        match order {
            WalkOrder::PreOrder => self.raw_prewalk_interruptible(callback),
            WalkOrder::PostOrder => self.raw_prewalk_interruptible(callback),
        }
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn raw_prewalk_interruptible<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>;

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    fn raw_postwalk_interruptible<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>;
}

/// Walking operations nested within an [Operation], including itself
impl RawWalk<Operation> for OperationRef {
    fn raw_prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<Operation>) -> WalkResult<B>,
    {
        raw_prewalk_operation_interruptible(*self, &mut callback)
    }

    fn raw_postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(UnsafeIntrusiveEntityRef<Operation>) -> WalkResult<B>,
    {
        raw_postwalk_operation_interruptible(*self, &mut callback)
    }
}

impl Walk<Operation> for OperationRef {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        prewalk_operation_interruptible(&self.borrow(), &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        postwalk_operation_interruptible(&self.borrow(), &mut callback)
    }
}

impl WalkMut<Operation> for OperationRef {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        prewalk_mut_operation_interruptible(&mut self.borrow_mut(), &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        postwalk_mut_operation_interruptible(&mut self.borrow_mut(), &mut callback)
    }
}

impl Walk<Operation> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        prewalk_operation_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        postwalk_operation_interruptible(self, &mut callback)
    }
}

impl WalkMut<Operation> for Operation {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        prewalk_mut_operation_interruptible(self, &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        postwalk_mut_operation_interruptible(self, &mut callback)
    }
}

fn raw_prewalk_operation_interruptible<F, B>(op: OperationRef, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    let result = callback(op);
    if !result.should_continue() {
        return result;
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

                let result = raw_prewalk_operation_interruptible(op, callback);
                if result.was_interrupted() {
                    return result;
                }
            }
        }
    }

    WalkResult::Continue(())
}

fn raw_postwalk_operation_interruptible<F, B>(op: OperationRef, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    let mut next_region = op.borrow().regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(op) = next_op.take() {
                next_op = op.next();

                let result = raw_postwalk_operation_interruptible(op, callback);
                if result.was_interrupted() {
                    return result;
                }
            }
        }
    }

    callback(op)
}

fn prewalk_operation_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&Operation) -> WalkResult<B>,
{
    let result = callback(op);
    if !result.should_continue() {
        return result;
    }

    for region in op.regions() {
        for block in region.body() {
            for op in block.body() {
                let result = prewalk_operation_interruptible(&op, callback);
                if result.was_interrupted() {
                    return result;
                }
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_operation_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&Operation) -> WalkResult<B>,
{
    for region in op.regions() {
        for block in region.body() {
            for op in block.body() {
                let result = postwalk_operation_interruptible(&op, callback);
                if result.was_interrupted() {
                    return result;
                }
            }
        }
    }

    callback(op)
}

fn prewalk_mut_operation_interruptible<F, B>(op: &mut Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&mut Operation) -> WalkResult<B>,
{
    let result = callback(op);
    if !result.should_continue() {
        return result;
    }

    let mut next_region = op.regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(mut op) = next_op.take() {
                next_op = op.next();

                prewalk_mut_operation_interruptible(&mut op.borrow_mut(), callback)?;
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_mut_operation_interruptible<F, B>(op: &mut Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&mut Operation) -> WalkResult<B>,
{
    let mut next_region = op.regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(mut op) = next_op.take() {
                next_op = op.next();

                postwalk_mut_operation_interruptible(&mut op.borrow_mut(), callback)?;
            }
        }
    }

    callback(op)
}

/// Walking regions of an [Operation], and those of all nested operations
impl RawWalk<Region> for OperationRef {
    fn raw_prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(RegionRef) -> WalkResult<B>,
    {
        raw_prewalk_regions_interruptible(*self, &mut callback)
    }

    fn raw_postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(RegionRef) -> WalkResult<B>,
    {
        raw_postwalk_regions_interruptible(*self, &mut callback)
    }
}

impl Walk<Region> for OperationRef {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Region) -> WalkResult<B>,
    {
        prewalk_regions_interruptible(&self.borrow(), &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Region) -> WalkResult<B>,
    {
        postwalk_regions_interruptible(&self.borrow(), &mut callback)
    }
}

impl WalkMut<Region> for OperationRef {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Region) -> WalkResult<B>,
    {
        prewalk_mut_regions_interruptible(&mut self.borrow_mut(), &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Region) -> WalkResult<B>,
    {
        postwalk_mut_regions_interruptible(&mut self.borrow_mut(), &mut callback)
    }
}

impl Walk<Region> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Region) -> WalkResult<B>,
    {
        prewalk_regions_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Region) -> WalkResult<B>,
    {
        postwalk_regions_interruptible(self, &mut callback)
    }
}

impl WalkMut<Region> for Operation {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Region) -> WalkResult<B>,
    {
        prewalk_mut_regions_interruptible(self, &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Region) -> WalkResult<B>,
    {
        postwalk_mut_regions_interruptible(self, &mut callback)
    }
}

fn raw_prewalk_regions_interruptible<F, B>(op: OperationRef, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(RegionRef) -> WalkResult<B>,
{
    let mut next_region = op.borrow().regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        match callback(region) {
            WalkResult::Continue(_) => {
                let mut next_block = region.borrow().body().front().as_pointer();
                while let Some(block) = next_block.take() {
                    next_block = block.next();

                    let mut next_op = block.borrow().body().front().as_pointer();
                    while let Some(op) = next_op.take() {
                        next_op = op.next();

                        let result = raw_prewalk_regions_interruptible(op, callback);
                        if result.was_interrupted() {
                            return result;
                        }
                    }
                }
            }
            WalkResult::Skip => continue,
            result @ WalkResult::Break(_) => return result,
        }
    }

    WalkResult::Continue(())
}

fn raw_postwalk_regions_interruptible<F, B>(op: OperationRef, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(RegionRef) -> WalkResult<B>,
{
    let mut next_region = op.borrow().regions().front().as_pointer();
    while let Some(region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(op) = next_op.take() {
                next_op = op.next();

                let result = raw_postwalk_regions_interruptible(op, callback);
                if result.was_interrupted() {
                    return result;
                }
            }
        }

        callback(region)?;
    }

    WalkResult::Continue(())
}

fn prewalk_regions_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&Region) -> WalkResult<B>,
{
    for region in op.regions() {
        match callback(&region) {
            WalkResult::Continue(_) => {
                for block in region.body() {
                    for op in block.body() {
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
    F: FnMut(&Region) -> WalkResult<B>,
{
    for region in op.regions() {
        for block in region.body() {
            for op in block.body() {
                postwalk_regions_interruptible(&op, callback)?;
            }
        }
        callback(&region)?;
    }

    WalkResult::Continue(())
}

fn prewalk_mut_regions_interruptible<F, B>(op: &mut Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&mut Region) -> WalkResult<B>,
{
    let mut next_region = op.regions().front().as_pointer();
    while let Some(mut region) = next_region.take() {
        next_region = region.next();

        match callback(&mut region.borrow_mut()) {
            WalkResult::Continue(_) => {
                let mut next_block = region.borrow().body().front().as_pointer();
                while let Some(block) = next_block.take() {
                    next_block = block.next();

                    let mut next_op = block.borrow().body().front().as_pointer();
                    while let Some(mut op) = next_op.take() {
                        next_op = op.next();

                        prewalk_mut_regions_interruptible(&mut op.borrow_mut(), callback)?;
                    }
                }
            }
            WalkResult::Skip => continue,
            result @ WalkResult::Break(_) => return result,
        }
    }

    WalkResult::Continue(())
}

fn postwalk_mut_regions_interruptible<F, B>(op: &mut Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&mut Region) -> WalkResult<B>,
{
    let mut next_region = op.regions().front().as_pointer();
    while let Some(mut region) = next_region.take() {
        next_region = region.next();

        let mut next_block = region.borrow().body().front().as_pointer();
        while let Some(block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(mut op) = next_op.take() {
                next_op = op.next();

                postwalk_mut_regions_interruptible(&mut op.borrow_mut(), callback)?;
            }
        }

        callback(&mut region.borrow_mut())?;
    }

    WalkResult::Continue(())
}

/// Walking operations nested within a [Region]
impl RawWalk<Operation> for RegionRef {
    fn raw_prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(OperationRef) -> WalkResult<B>,
    {
        raw_prewalk_region_operations_interruptible(*self, &mut callback)
    }

    fn raw_postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(OperationRef) -> WalkResult<B>,
    {
        raw_postwalk_region_operations_interruptible(*self, &mut callback)
    }
}

impl Walk<Operation> for RegionRef {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        prewalk_region_operations_interruptible(&self.borrow(), &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        postwalk_region_operations_interruptible(&self.borrow(), &mut callback)
    }
}

impl WalkMut<Operation> for RegionRef {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        prewalk_mut_region_operations_interruptible(&mut self.borrow_mut(), &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        postwalk_mut_region_operations_interruptible(&mut self.borrow_mut(), &mut callback)
    }
}

impl Walk<Operation> for Region {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        prewalk_region_operations_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        postwalk_region_operations_interruptible(self, &mut callback)
    }
}

impl WalkMut<Operation> for Region {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        prewalk_mut_region_operations_interruptible(self, &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        postwalk_mut_region_operations_interruptible(self, &mut callback)
    }
}

fn raw_prewalk_region_operations_interruptible<F, B>(
    region: RegionRef,
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

            match callback(op) {
                WalkResult::Continue(_) => {
                    let mut next_region = op.borrow().regions().front().as_pointer();
                    while let Some(region) = next_region.take() {
                        next_region = region.next();

                        raw_prewalk_region_operations_interruptible(region, callback)?;
                    }
                }
                WalkResult::Skip => continue,
                result @ WalkResult::Break(_) => return result,
            }
        }
    }

    WalkResult::Continue(())
}

fn raw_postwalk_region_operations_interruptible<F, B>(
    region: RegionRef,
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

            let mut next_region = op.borrow().regions().front().as_pointer();
            while let Some(region) = next_region.take() {
                next_region = region.next();

                raw_postwalk_region_operations_interruptible(region, callback)?;
            }

            callback(op)?;
        }
    }

    WalkResult::Continue(())
}

fn prewalk_region_operations_interruptible<F, B>(region: &Region, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&Operation) -> WalkResult<B>,
{
    for block in region.body() {
        for op in block.body() {
            match callback(&op) {
                WalkResult::Continue(_) => {
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
    F: FnMut(&Operation) -> WalkResult<B>,
{
    for block in region.body() {
        for op in block.body() {
            for region in op.regions() {
                postwalk_region_operations_interruptible(&region, callback)?;
            }
            callback(&op)?;
        }
    }

    WalkResult::Continue(())
}

fn prewalk_mut_region_operations_interruptible<F, B>(
    region: &mut Region,
    callback: &mut F,
) -> WalkResult<B>
where
    F: FnMut(&mut Operation) -> WalkResult<B>,
{
    for block in region.body() {
        let mut next_op = block.body().front().as_pointer();
        while let Some(mut op) = next_op.take() {
            next_op = op.next();

            let mut op = op.borrow_mut();
            match callback(&mut op) {
                WalkResult::Continue(_) => {
                    let mut next_region = op.regions().front().as_pointer();
                    drop(op);

                    while let Some(mut region) = next_region.take() {
                        next_region = region.next();

                        prewalk_mut_region_operations_interruptible(
                            &mut region.borrow_mut(),
                            callback,
                        )?;
                    }
                }
                WalkResult::Skip => continue,
                result @ WalkResult::Break(_) => return result,
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_mut_region_operations_interruptible<F, B>(
    region: &mut Region,
    callback: &mut F,
) -> WalkResult<B>
where
    F: FnMut(&mut Operation) -> WalkResult<B>,
{
    for block in region.body() {
        let mut next_op = block.body().front().as_pointer();
        while let Some(mut op) = next_op.take() {
            next_op = op.next();

            let mut next_region = op.borrow().regions().front().as_pointer();
            while let Some(mut region) = next_region.take() {
                next_region = region.next();

                postwalk_mut_region_operations_interruptible(&mut region.borrow_mut(), callback)?;
            }

            callback(&mut op.borrow_mut())?;
        }
    }

    WalkResult::Continue(())
}

/// Walking blocks of an [Operation], and those of all nested operations
impl Walk<Block> for OperationRef {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Block) -> WalkResult<B>,
    {
        prewalk_blocks_interruptible(&self.borrow(), &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Block) -> WalkResult<B>,
    {
        postwalk_blocks_interruptible(&self.borrow(), &mut callback)
    }
}

impl WalkMut<Block> for OperationRef {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Block) -> WalkResult<B>,
    {
        prewalk_mut_blocks_interruptible(&mut self.borrow_mut(), &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Block) -> WalkResult<B>,
    {
        postwalk_mut_blocks_interruptible(&mut self.borrow_mut(), &mut callback)
    }
}

impl Walk<Block> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Block) -> WalkResult<B>,
    {
        prewalk_blocks_interruptible(self, &mut callback)
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&Block) -> WalkResult<B>,
    {
        postwalk_blocks_interruptible(self, &mut callback)
    }
}

impl WalkMut<Block> for Operation {
    fn prewalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Block) -> WalkResult<B>,
    {
        prewalk_mut_blocks_interruptible(self, &mut callback)
    }

    fn postwalk_mut_interruptible<F, B>(&mut self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(&mut Block) -> WalkResult<B>,
    {
        postwalk_mut_blocks_interruptible(self, &mut callback)
    }
}

fn prewalk_blocks_interruptible<F, B>(op: &Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&Block) -> WalkResult<B>,
{
    for region in op.regions() {
        for block in region.body() {
            match callback(&block) {
                WalkResult::Continue(_) => {
                    for op in block.body() {
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
    F: FnMut(&Block) -> WalkResult<B>,
{
    for region in op.regions() {
        for block in region.body() {
            for op in block.body() {
                postwalk_blocks_interruptible(&op, callback)?;
            }

            callback(&block)?;
        }
    }

    WalkResult::Continue(())
}

fn prewalk_mut_blocks_interruptible<F, B>(op: &mut Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&mut Block) -> WalkResult<B>,
{
    for region in op.regions() {
        let mut next_block = region.body().front().as_pointer();
        while let Some(mut block) = next_block.take() {
            next_block = block.next();

            let mut block = block.borrow_mut();
            match callback(&mut block) {
                WalkResult::Continue(_) => {
                    let mut next_op = block.body().front().as_pointer();
                    drop(block);

                    while let Some(mut op) = next_op.take() {
                        next_op = op.next();

                        prewalk_mut_blocks_interruptible(&mut op.borrow_mut(), callback)?;
                    }
                }
                WalkResult::Skip => continue,
                result @ WalkResult::Break(_) => return result,
            }
        }
    }

    WalkResult::Continue(())
}

fn postwalk_mut_blocks_interruptible<F, B>(op: &mut Operation, callback: &mut F) -> WalkResult<B>
where
    F: FnMut(&mut Block) -> WalkResult<B>,
{
    for region in op.regions() {
        let mut next_block = region.body().front().as_pointer();
        while let Some(mut block) = next_block.take() {
            next_block = block.next();

            let mut next_op = block.borrow().body().front().as_pointer();
            while let Some(mut op) = next_op.take() {
                next_op = op.next();

                postwalk_mut_blocks_interruptible(&mut op.borrow_mut(), callback)?;
            }

            callback(&mut block.borrow_mut())?;
        }
    }

    WalkResult::Continue(())
}
