pub use core::ops::ControlFlow;

use crate::{
    Block, BlockRef, Op, Operation, OperationRef, Region, RegionRef, Report, Symbol,
    UnsafeIntrusiveEntityRef,
};

/// A generic trait that describes visitors for all kinds
pub trait Visitor<T: ?Sized> {
    /// The type of output produced by visiting an item.
    type Output;

    /// The function which is applied to each `T` as it is visited.
    fn visit(&mut self, current: &T) -> WalkResult<Self::Output>;
}

/// We can automatically convert any closure of appropriate type to a `Visitor`
impl<T: ?Sized, U, F> Visitor<T> for F
where
    F: FnMut(&T) -> WalkResult<U>,
{
    type Output = U;

    #[inline]
    fn visit(&mut self, op: &T) -> WalkResult<Self::Output> {
        self(op)
    }
}

/// Represents a visitor over [Operation]
pub trait OperationVisitor: Visitor<Operation> {}
impl<V> OperationVisitor for V where V: Visitor<Operation> {}

/// Represents a visitor over [Op] of type `T`
pub trait OpVisitor<T: Op>: Visitor<T> {}
impl<T: Op, V> OpVisitor<T> for V where V: Visitor<T> {}

/// Represents a visitor over [Symbol]
pub trait SymbolVisitor: Visitor<dyn Symbol> {}
impl<V> SymbolVisitor for V where V: Visitor<dyn Symbol> {}

/// A result-like type used to control traversals of a [Walkable] entity.
///
/// It is comparable to [core::ops::ControlFlow], with an additional option to continue traversal,
/// but with a sibling, rather than visiting any further children of the current item.
///
/// It is compatible with the `?` operator, however doing so will exit early on _both_ `Skip` and
/// `Break`, so you should be aware of that when using the try operator.
#[derive(Clone)]
#[must_use]
pub enum WalkResult<B = Report, C = ()> {
    /// Continue the traversal normally, optionally producing a value for the current item.
    Continue(C),
    /// Skip traversing the current item and any children that have not been visited yet, and
    /// continue the traversal with the next sibling of the current item.
    Skip,
    /// Stop the traversal entirely, and optionally returning a value associated with the break.
    //
    /// This can be used to represent both errors, and the successful result of a search.
    Break(B),
}
impl<B, C> WalkResult<B, C> {
    /// Returns true if the walk should continue
    pub fn should_continue(&self) -> bool {
        matches!(self, Self::Continue(_))
    }

    /// Returns true if the walk was interrupted
    pub fn was_interrupted(&self) -> bool {
        matches!(self, Self::Break(_))
    }

    /// Returns true if the walk was skipped
    pub fn was_skipped(&self) -> bool {
        matches!(self, Self::Skip)
    }
}
impl<B> WalkResult<B, ()> {
    /// Convert this [WalkResult] into an equivalent [Result]
    #[inline]
    pub fn into_result(self) -> Result<(), B> {
        match self {
            Self::Break(err) => Err(err),
            Self::Skip | Self::Continue(_) => Ok(()),
        }
    }
}
impl<B> From<Result<(), B>> for WalkResult<B, ()> {
    fn from(value: Result<(), B>) -> Self {
        match value {
            Ok(_) => WalkResult::Continue(()),
            Err(err) => WalkResult::Break(err),
        }
    }
}
impl<B> From<WalkResult<B, ()>> for Result<(), B> {
    #[inline(always)]
    fn from(value: WalkResult<B, ()>) -> Self {
        value.into_result()
    }
}
impl<B, C> core::ops::FromResidual for WalkResult<B, C> {
    fn from_residual(residual: <Self as core::ops::Try>::Residual) -> Self {
        match residual {
            WalkResult::Break(b) => WalkResult::Break(b),
            _ => unreachable!(),
        }
    }
}
impl<B, C> core::ops::Residual<C> for WalkResult<B, core::convert::Infallible> {
    type TryType = WalkResult<B, C>;
}
impl<B, C> core::ops::Try for WalkResult<B, C> {
    type Output = C;
    type Residual = WalkResult<B, core::convert::Infallible>;

    #[inline]
    fn from_output(output: Self::Output) -> Self {
        WalkResult::Continue(output)
    }

    #[inline]
    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            WalkResult::Continue(c) => ControlFlow::Continue(c),
            WalkResult::Skip => ControlFlow::Break(WalkResult::Skip),
            WalkResult::Break(b) => ControlFlow::Break(WalkResult::Break(b)),
        }
    }
}

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

/// Walking regions of an [Operation], and those of all nested operations
impl Walkable<Region> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(RegionRef) -> WalkResult<B>,
    {
        let mut regions = self.regions().front();
        while let Some(region) = regions.as_pointer() {
            regions.move_next();
            match callback(region.clone()) {
                WalkResult::Continue(_) => {
                    let region = region.borrow();
                    for block in region.body().iter() {
                        for op in block.body().iter() {
                            op.prewalk_interruptible(&mut callback)?;
                        }
                    }
                }
                WalkResult::Skip => continue,
                result @ WalkResult::Break(_) => return result,
            }
        }

        WalkResult::Continue(())
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(RegionRef) -> WalkResult<B>,
    {
        let mut regions = self.regions().front();
        while let Some(region) = regions.as_pointer() {
            regions.move_next();
            {
                let region = region.borrow();
                for block in region.body().iter() {
                    for op in block.body().iter() {
                        op.postwalk_interruptible(&mut callback)?;
                    }
                }
            }
            callback(region.clone())?;
        }

        WalkResult::Continue(())
    }
}

/// Walking blocks of an [Operation], and those of all nested operations
impl Walkable<Block> for Operation {
    fn prewalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(BlockRef) -> WalkResult<B>,
    {
        for region in self.regions().iter() {
            let mut blocks = region.body().front();
            while let Some(block) = blocks.as_pointer() {
                blocks.move_next();
                match callback(block.clone()) {
                    WalkResult::Continue(_) => {
                        let block = block.borrow();
                        for op in block.body().iter() {
                            op.prewalk_interruptible(&mut callback)?;
                        }
                    }
                    WalkResult::Skip => continue,
                    result @ WalkResult::Break(_) => return result,
                }
            }
        }

        WalkResult::Continue(())
    }

    fn postwalk_interruptible<F, B>(&self, mut callback: F) -> WalkResult<B>
    where
        F: FnMut(BlockRef) -> WalkResult<B>,
    {
        for region in self.regions().iter() {
            let mut blocks = region.body().front();
            while let Some(block) = blocks.as_pointer() {
                blocks.move_next();
                {
                    let block = block.borrow();
                    for op in block.body().iter() {
                        op.postwalk_interruptible(&mut callback)?;
                    }
                }
                callback(block.clone())?;
            }
        }

        WalkResult::Continue(())
    }
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

/// [Searcher] is a driver for [Visitor] impls as applied to some root [Operation].
///
/// The searcher traverses the object graph in depth-first preorder, from operations to regions to
/// blocks to operations, etc. All nested items of an entity are visited before its siblings, i.e.
/// a region is fully visited before the next region of the same containing operation.
///
/// This is effectively control-flow order, from an abstract interpretation perspective, i.e. an
/// actual program might only execute one region of a multi-region op, but this traversal will visit
/// all of them unless otherwise directed by a `WalkResult`.
pub struct Searcher<V, T: ?Sized> {
    visitor: V,
    root: OperationRef,
    _marker: core::marker::PhantomData<T>,
}
impl<T: ?Sized, V: Visitor<T>> Searcher<V, T> {
    pub fn new(root: OperationRef, visitor: V) -> Self {
        Self {
            visitor,
            root,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<V: OperationVisitor> Searcher<V, Operation> {
    pub fn visit(&mut self) -> WalkResult<<V as Visitor<Operation>>::Output> {
        self.root.borrow().prewalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            self.visitor.visit(&op)
        })
    }
}

impl<T: Op, V: OpVisitor<T>> Searcher<V, T> {
    pub fn visit(&mut self) -> WalkResult<<V as Visitor<T>>::Output> {
        self.root.borrow().prewalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            if let Some(op) = op.downcast_ref::<T>() {
                self.visitor.visit(op)
            } else {
                WalkResult::Continue(())
            }
        })
    }
}

impl<V: SymbolVisitor> Searcher<V, dyn Symbol> {
    pub fn visit(&mut self) -> WalkResult<<V as Visitor<dyn Symbol>>::Output> {
        self.root.borrow().prewalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            if let Some(sym) = op.as_symbol() {
                self.visitor.visit(sym)
            } else {
                WalkResult::Continue(())
            }
        })
    }
}
