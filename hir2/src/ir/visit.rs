mod blocks;
mod searcher;
mod visitor;
mod walkable;

pub use core::ops::ControlFlow;

pub use self::{
    blocks::{BlockIter, PostOrderBlockIter},
    searcher::Searcher,
    visitor::{OpVisitor, OperationVisitor, SymbolVisitor, Visitor},
    walkable::{WalkOrder, WalkStage, Walkable},
};
use crate::Report;

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
