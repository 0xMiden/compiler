use super::WalkResult;
use crate::{
    Block, BlockRef, Direction, Operation, OperationRef, Region, RegionRef,
    UnsafeIntrusiveEntityRef,
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

pub trait WalkDirection =
    Walker<Region, BlockRef> + Walker<Block, OperationRef> + Walker<Operation, RegionRef>;

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
    fn walk_all<D, F>(&self, order: WalkOrder, mut callback: F)
    where
        D: WalkDirection,
        F: FnMut(&T),
    {
        let _ = self.walk::<D, _, _>(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn prewalk_all<D, F>(&self, callback: F)
    where
        D: WalkDirection,
        F: FnMut(&T),
    {
        self.walk_all::<D, _>(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn postwalk_all<D, F>(&self, callback: F)
    where
        D: WalkDirection,
        F: FnMut(&T),
    {
        self.walk_all::<D, _>(WalkOrder::PostOrder, callback)
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    fn walk<D, F, B>(&self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&T) -> WalkResult<B>;

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn prewalk<D, F, B>(&self, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&T) -> WalkResult<B>,
    {
        self.walk::<D, _, _>(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn postwalk<D, F, B>(&self, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&T) -> WalkResult<B>,
    {
        self.walk::<D, _, _>(WalkOrder::PostOrder, callback)
    }
}

/// A mutable variant of [Walk], for traversal which may mutate visited entities.
pub trait WalkMut<T> {
    /// Walk all `T` in `self` in a specific order, applying the given callback to each.
    ///
    /// This is very similar to [Walkable::walk_interruptible], except the callback has no control
    /// over the traversal, and must be infallible.
    fn walk_all_mut<D, F>(&mut self, order: WalkOrder, mut callback: F)
    where
        D: WalkDirection,
        F: FnMut(&mut T),
    {
        let _ = self.walk_mut::<D, _, _>(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn prewalk_all_mut<D, F>(&mut self, callback: F)
    where
        D: WalkDirection,
        F: FnMut(&mut T),
    {
        self.walk_all_mut::<D, _>(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn postwalk_all_mut<D, F>(&mut self, callback: F)
    where
        D: WalkDirection,
        F: FnMut(&mut T),
    {
        self.walk_all_mut::<D, _>(WalkOrder::PostOrder, callback)
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    fn walk_mut<D, F, B>(&mut self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&mut T) -> WalkResult<B>;

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn prewalk_mut_interruptible<D, F, B>(&mut self, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&mut T) -> WalkResult<B>,
    {
        self.walk_mut::<D, _, _>(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn postwalk_mut_interruptible<D, F, B>(&mut self, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&mut T) -> WalkResult<B>,
    {
        self.walk_mut::<D, _, _>(WalkOrder::PostOrder, callback)
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
    fn raw_walk_all<D, F>(&self, order: WalkOrder, mut callback: F)
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        let _ = self.raw_walk::<D, _, _>(order, |t| {
            callback(t);

            WalkResult::<()>::Continue(())
        });
    }

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn raw_prewalk_all<D, F>(&self, callback: F)
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        self.raw_walk_all::<D, _>(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`.
    #[inline]
    fn raw_postwalk_all<D, F>(&self, callback: F)
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<T>),
    {
        self.raw_walk_all::<D, _>(WalkOrder::PostOrder, callback)
    }

    /// Walk `self` in the given order, visiting each `T` and applying the given callback to them.
    ///
    /// The given callback can control the traversal using the [WalkResult] it returns:
    ///
    /// * `WalkResult::Skip` will skip the walk of the current item and its nested elements that
    ///   have not been visited already, continuing with the next item.
    /// * `WalkResult::Break` will interrupt the walk, and no more items will be visited
    /// * `WalkResult::Continue` will continue the walk
    fn raw_walk<D, F, B>(&self, order: WalkOrder, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>;

    /// Walk all `T` in `self` using a pre-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn raw_prewalk<D, F, B>(&self, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>,
    {
        self.raw_walk::<D, _, _>(WalkOrder::PreOrder, callback)
    }

    /// Walk all `T` in `self` using a post-order, depth-first traversal, applying the given callback
    /// to each `T`, and determining how to proceed based on the returned [WalkResult].
    #[inline]
    fn raw_postwalk<D, F, B>(&self, callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<T>) -> WalkResult<B>,
    {
        self.raw_walk::<D, _, _>(WalkOrder::PostOrder, callback)
    }
}

/// Walking operations nested within an [Operation], including itself
impl RawWalk<Operation> for OperationRef {
    fn raw_walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<Operation>) -> WalkResult<B>,
    {
        raw_walk_operations::<D, _, _>(*self, order, &mut callback)
    }
}

impl Walk<Operation> for OperationRef {
    fn walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_operations::<D, _, _>(*self, order, &mut wrapper)
    }
}

impl WalkMut<Operation> for OperationRef {
    fn walk_mut<D, F, B>(&mut self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        let mut wrapper = |mut op: OperationRef| callback(&mut op.borrow_mut());
        raw_walk_operations::<D, _, _>(*self, order, &mut wrapper)
    }
}

impl Walk<Operation> for Operation {
    fn walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_operations::<D, _, _>(self.as_operation_ref(), order, &mut wrapper)
    }
}

/// Walking regions of an [Operation], and those of all nested operations
impl RawWalk<Region> for OperationRef {
    fn raw_walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<Region>) -> WalkResult<B>,
    {
        raw_walk_regions::<D, _, _>(*self, order, &mut callback)
    }
}

impl Walk<Region> for OperationRef {
    fn walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&Region) -> WalkResult<B>,
    {
        let mut wrapper = |region: RegionRef| callback(&region.borrow());
        raw_walk_regions::<D, _, _>(*self, order, &mut wrapper)
    }
}

impl WalkMut<Region> for OperationRef {
    fn walk_mut<D, F, B>(&mut self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&mut Region) -> WalkResult<B>,
    {
        let mut wrapper = |mut region: RegionRef| callback(&mut region.borrow_mut());
        raw_walk_regions::<D, _, _>(*self, order, &mut wrapper)
    }
}

impl Walk<Region> for Operation {
    fn walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&Region) -> WalkResult<B>,
    {
        let mut wrapper = |region: RegionRef| callback(&region.borrow());
        raw_walk_regions::<D, _, _>(self.as_operation_ref(), order, &mut wrapper)
    }
}

#[allow(unused)]
pub fn raw_walk<D, F, B>(op: OperationRef, callback: &mut F) -> WalkResult<B>
where
    D: WalkDirection,
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

                let mut next_block = D::start(&*region.borrow());
                while let Some(block) = next_block.take() {
                    next_block = D::continue_walk(block);

                    let mut next_op = D::start(&*block.borrow());
                    while let Some(op) = next_op.take() {
                        next_op = D::continue_walk(op);

                        raw_walk::<D, _, _>(op, callback)?;
                    }
                }
            }
        }
    }

    // Invoke callback after all regions have been visited
    callback(op, &stage)
}

fn raw_walk_regions<D, F, B>(op: OperationRef, order: WalkOrder, callback: &mut F) -> WalkResult<B>
where
    D: WalkDirection,
    F: FnMut(RegionRef) -> WalkResult<B>,
{
    let mut next_region = D::start(&*op.borrow());
    while let Some(region) = next_region.take() {
        next_region = D::continue_walk(region);

        if matches!(order, WalkOrder::PreOrder) {
            let result = callback(region);
            match result {
                WalkResult::Skip => continue,
                err @ WalkResult::Break(_) => return err,
                _ => (),
            }
        }

        let mut next_block = D::start(&*region.borrow());
        while let Some(block) = next_block.take() {
            next_block = D::continue_walk(block);

            let mut next_op = D::start(&*block.borrow());
            while let Some(op) = next_op.take() {
                next_op = D::continue_walk(op);

                raw_walk_regions::<D, _, _>(op, order, callback)?;
            }
        }

        if matches!(order, WalkOrder::PostOrder) {
            callback(region)?;
        }
    }

    WalkResult::Continue(())
}

#[allow(unused)]
fn raw_walk_blocks<D, F, B>(op: OperationRef, order: WalkOrder, callback: &mut F) -> WalkResult<B>
where
    D: WalkDirection,
    F: FnMut(BlockRef) -> WalkResult<B>,
{
    let mut next_region = D::start(&*op.borrow());
    while let Some(region) = next_region.take() {
        next_region = D::continue_walk(region);

        let mut next_block = D::start(&*region.borrow());
        while let Some(block) = next_block.take() {
            next_block = D::continue_walk(block);

            if matches!(order, WalkOrder::PreOrder) {
                let result = callback(block);
                match result {
                    WalkResult::Skip => continue,
                    err @ WalkResult::Break(_) => return err,
                    _ => (),
                }
            }

            let mut next_op = D::start(&*block.borrow());
            while let Some(op) = next_op.take() {
                next_op = D::continue_walk(op);

                raw_walk_blocks::<D, _, _>(op, order, callback)?;
            }

            if matches!(order, WalkOrder::PostOrder) {
                callback(block)?;
            }
        }
    }

    WalkResult::Continue(())
}

fn raw_walk_operations<D, F, B>(
    op: OperationRef,
    order: WalkOrder,
    callback: &mut F,
) -> WalkResult<B>
where
    D: WalkDirection,
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

    let mut next_region = D::start(&*op.borrow());
    while let Some(region) = next_region.take() {
        next_region = D::continue_walk(region);

        let mut next_block = D::start(&*region.borrow());
        while let Some(block) = next_block.take() {
            next_block = D::continue_walk(block);

            let mut next_op = D::start(&*block.borrow());
            while let Some(op) = next_op.take() {
                next_op = D::continue_walk(op);

                raw_walk_operations::<D, _, _>(op, order, callback)?;
            }
        }
    }

    if matches!(order, WalkOrder::PostOrder) {
        callback(op)?;
    }

    WalkResult::Continue(())
}

fn raw_walk_region_operations<D, F, B>(
    region: RegionRef,
    order: WalkOrder,
    callback: &mut F,
) -> WalkResult<B>
where
    D: WalkDirection,
    F: FnMut(OperationRef) -> WalkResult<B>,
{
    let mut next_block = D::start(&*region.borrow());
    while let Some(block) = next_block.take() {
        next_block = D::continue_walk(block);

        let mut next_op = D::start(&*block.borrow());
        while let Some(op) = next_op.take() {
            next_op = D::continue_walk(op);

            raw_walk_operations::<D, _, _>(op, order, callback)?;
        }
    }

    WalkResult::Continue(())
}

/// Walking operations nested within a [Region]
impl RawWalk<Operation> for RegionRef {
    fn raw_walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(UnsafeIntrusiveEntityRef<Operation>) -> WalkResult<B>,
    {
        raw_walk_region_operations::<D, _, _>(*self, order, &mut callback)
    }
}

impl Walk<Operation> for RegionRef {
    fn walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_region_operations::<D, _, _>(*self, order, &mut wrapper)
    }
}

impl WalkMut<Operation> for RegionRef {
    fn walk_mut<D, F, B>(&mut self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&mut Operation) -> WalkResult<B>,
    {
        let mut wrapper = |mut op: OperationRef| callback(&mut op.borrow_mut());
        raw_walk_region_operations::<D, _, _>(*self, order, &mut wrapper)
    }
}

impl Walk<Operation> for Region {
    fn walk<D, F, B>(&self, order: WalkOrder, mut callback: F) -> WalkResult<B>
    where
        D: WalkDirection,
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        let mut wrapper = |op: OperationRef| callback(&op.borrow());
        raw_walk_region_operations::<D, _, _>(self.as_region_ref(), order, &mut wrapper)
    }
}

pub trait Walker<Parent, Child> {
    fn start(entity: &Parent) -> Option<Child>;
    fn continue_walk(child: Child) -> Option<Child>;
}

/// A custom [WalkDirectionImpl] that is the same as [Forward], except the operations of each block
/// are visited bottom-up, i.e. as if [Backward] applied just to [Block].
pub struct ReverseBlock;

impl Walker<Region, BlockRef> for ReverseBlock {
    #[inline(always)]
    fn start(entity: &Region) -> Option<BlockRef> {
        entity.body().front().as_pointer()
    }

    #[inline(always)]
    fn continue_walk(child: BlockRef) -> Option<BlockRef> {
        child.next()
    }
}

impl Walker<Block, OperationRef> for ReverseBlock {
    #[inline(always)]
    fn start(entity: &Block) -> Option<OperationRef> {
        entity.body().back().as_pointer()
    }

    #[inline(always)]
    fn continue_walk(child: OperationRef) -> Option<OperationRef> {
        child.prev()
    }
}

impl Walker<Operation, RegionRef> for ReverseBlock {
    #[inline(always)]
    fn start(entity: &Operation) -> Option<RegionRef> {
        entity.regions().front().as_pointer()
    }

    #[inline(always)]
    fn continue_walk(child: RegionRef) -> Option<RegionRef> {
        child.next()
    }
}

impl<D: Direction> Walker<Region, BlockRef> for D {
    #[inline(always)]
    fn start(entity: &Region) -> Option<BlockRef> {
        if const { D::IS_FORWARD } {
            entity.body().front().as_pointer()
        } else {
            entity.body().back().as_pointer()
        }
    }

    #[inline(always)]
    fn continue_walk(child: BlockRef) -> Option<BlockRef> {
        if const { D::IS_FORWARD } {
            child.next()
        } else {
            child.prev()
        }
    }
}

impl<D: Direction> Walker<Block, OperationRef> for D {
    #[inline(always)]
    fn start(entity: &Block) -> Option<OperationRef> {
        if const { D::IS_FORWARD } {
            entity.body().front().as_pointer()
        } else {
            entity.body().back().as_pointer()
        }
    }

    #[inline(always)]
    fn continue_walk(child: OperationRef) -> Option<OperationRef> {
        if const { D::IS_FORWARD } {
            child.next()
        } else {
            child.prev()
        }
    }
}

impl<D: Direction> Walker<Operation, RegionRef> for D {
    #[inline(always)]
    fn start(entity: &Operation) -> Option<RegionRef> {
        if const { D::IS_FORWARD } {
            entity.regions().front().as_pointer()
        } else {
            entity.regions().back().as_pointer()
        }
    }

    #[inline(always)]
    fn continue_walk(child: RegionRef) -> Option<RegionRef> {
        if const { D::IS_FORWARD } {
            child.next()
        } else {
            child.prev()
        }
    }
}
