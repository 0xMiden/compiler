use super::*;
use crate::{
    attributes::AttributeValue, traits::Terminator, Op, SuccessorOperandRange,
    SuccessorOperandRangeMut, Type,
};

/// An op interface that indicates what types of regions it holds
pub trait RegionKindInterface {
    /// Get the [RegionKind] for this operation
    fn kind(&self) -> RegionKind;
    /// Returns true if the kind of this operation's regions requires SSA dominance
    #[inline]
    fn has_ssa_dominance(&self) -> bool {
        matches!(self.kind(), RegionKind::SSA)
    }
    #[inline]
    fn has_graph_regions(&self) -> bool {
        matches!(self.kind(), RegionKind::Graph)
    }
}

// TODO(pauls): Implement verifier
/// This interface provides information for region operations that exhibit branching behavior
/// between held regions. I.e., this interface allows for expressing control flow information for
/// region holding operations.
///
/// This interface is meant to model well-defined cases of control-flow and value propagation,
/// where what occurs along control-flow edges is assumed to be side-effect free.
///
/// A "region branch point" indicates a point from which a branch originates. It can indicate either
/// a region of this op or [RegionBranchPoint::Parent]. In the latter case, the branch originates
/// from outside of the op, i.e., when first executing this op.
///
/// A "region successor" indicates the target of a branch. It can indicate either a region of this
/// op or this op. In the former case, the region successor is a region pointer and a range of block
/// arguments to which the "successor operands" are forwarded to. In the latter case, the control
/// flow leaves this op and the region successor is a range of results of this op to which the
/// successor operands are forwarded to.
///
/// By default, successor operands and successor block arguments/successor results must have the
/// same type. `areTypesCompatible` can be implemented to allow non-equal types.
///
/// ## Example
///
/// ```hir,ignore
/// %r = scf.for %iv = %lb to %ub step %step iter_args(%a = %b)
///     -> tensor<5xf32> {
///     ...
///     scf.yield %c : tensor<5xf32>
/// }
/// ```
///
/// `scf.for` has one region. The region has two region successors: the region itself and the
/// `scf.for` op. `%b` is an entry successor operand. `%c` is a successor operand. `%a` is a
/// successor block argument. `%r` is a successor result.
pub trait RegionBranchOpInterface: Op {
    /// Returns the operands of this operation that are forwarded to the region successor's block
    /// arguments or this operation's results when branching to `point`. `point` is guaranteed to
    /// be among the successors that are returned by `get_entry_succcessor_regions` or
    /// `get_successor_regions(parent_op())`.
    ///
    /// ## Example
    ///
    /// In the example in the top-level docs of this trait, this function returns the operand `%b`
    /// of the `scf.for` op, regardless of the value of `point`, i.e. this op always forwards the
    /// same operands, regardless of whether the loop has 0 or more iterations.
    #[inline]
    #[allow(unused_variables)]
    fn get_entry_successor_operands(&self, point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        crate::SuccessorOperandRange::empty()
    }
    /// Returns the potential region successors when first executing the op.
    ///
    /// Unlike [get_successor_regions], this method also passes along the constant operands of this
    /// op. Based on these, the implementation may filter out certain successors. By default, it
    /// simply dispatches to `get_successor_regions`. `operands` contains an entry for every operand
    /// of this op, with `None` representing if the operand is non-constant.
    ///
    /// NOTE: The control flow does not necessarily have to enter any region of this op.
    ///
    /// ## Example
    ///
    /// In the example in the top-level docs of this trait, this function may return two region
    /// successors: the single region of the `scf.for` op and the `scf.for` operation (that
    /// implements this interface). If `%lb`, `%ub`, `%step` are constants and it can be determined
    /// the loop does not have any iterations, this function may choose to return only this
    /// operation. Similarly, if it can be determined that the loop has at least one iteration, this
    /// function may choose to return only the region of the loop.
    #[inline]
    #[allow(unused_variables)]
    fn get_entry_successor_regions(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> RegionSuccessorIter<'_> {
        self.get_successor_regions(RegionBranchPoint::Parent)
    }
    /// Returns the potential region successors when branching from `point`.
    ///
    /// These are the regions that may be selected during the flow of control.
    ///
    /// When `point` is [RegionBranchPoint::Parent], this function returns the region successors
    /// when entering the operation. Otherwise, this method returns the successor regions when
    /// branching from the region indicated by `point`.
    ///
    /// ## Example
    ///
    /// In the example in the top-level docs of this trait, this function returns the region of the
    /// `scf.for` and this operation for either region branch point (`parent` and the region of the
    /// `scf.for`). An implementation may choose to filter out region successors when it is
    /// statically known (e.g., by examining the operands of this op) that those successors are not
    /// branched to.
    fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_>;
    /// Returns a set of invocation bounds, representing the minimum and maximum number of times
    /// this operation will invoke each attached region (assuming the regions yield normally, i.e.
    /// do not abort or invoke an infinite loop). The minimum number of invocations is at least 0.
    /// If the maximum number of invocations cannot be statically determined, then it will be set to
    /// [InvocationBounds::unknown].
    ///
    /// This function also passes along the constant operands of this op. `operands` contains an
    /// entry for every operand of this op, with `None` representing if the operand is non-constant.
    ///
    /// This function may be called speculatively on operations where the provided operands are not
    /// necessarily the same as the operation's current operands. This may occur in analyses that
    /// wish to determine "what would be the region invocations if these were the operands?"
    #[inline]
    #[allow(unused_variables)]
    fn get_region_invocation_bounds(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        use smallvec::smallvec;

        smallvec![InvocationBounds::Unknown; self.num_regions()]
    }
    /// This function is called to compare types along control-flow edges.
    ///
    /// By default, the types are check for exact equality.
    #[inline]
    fn are_types_compatible(&self, lhs: &Type, rhs: &Type) -> bool {
        lhs == rhs
    }
    /// Returns `true` if control flow originating from the region at `index` may eventually branch
    /// back to the same region, either from itself, or after passing through other regions first.
    fn is_repetitive_region(&self, index: usize) -> bool;
    /// Returns `true` if there is a loop in the region branching graph.
    ///
    /// Only reachable regions (starting from the entry region) are considered.
    fn has_loop(&self) -> bool;
}

// TODO(pauls): Implement verifier (should have no results and no successors)
/// This interface provides information for branching terminator operations in the presence of a
/// parent [RegionBranchOpInterface] implementation. It specifies which operands are passed to which
/// successor region.
pub trait RegionBranchTerminatorOpInterface: Op + Terminator {
    /// Get a range of operands corresponding to values that are semantically "returned" by passing
    /// them to the region successor indicated by `point`.
    fn get_successor_operands(&self, point: RegionBranchPoint) -> SuccessorOperandRange<'_>;
    /// Get a mutable range of operands corresponding to values that are semantically "returned" by
    /// passing them to the region successor indicated by `point`.
    fn get_mutable_successor_operands(
        &mut self,
        point: RegionBranchPoint,
    ) -> SuccessorOperandRangeMut<'_>;
    /// Returns the potential region successors that are branched to after this terminator based on
    /// the given constant operands.
    ///
    /// This method also passes along the constant operands of this op. `operands` contains an entry
    /// for every operand of this op, with `None` representing non-constant values.
    ///
    /// The default implementation simply dispatches to the parent `RegionBranchOpInterface`'s
    /// `get_successor_regions` implementation.
    #[allow(unused_variables)]
    fn get_successor_regions(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        let parent_region =
            self.parent_region().expect("expected operation to have a parent region");
        let parent_op =
            parent_region.borrow().parent().expect("expected operation to have a parent op");
        parent_op
            .borrow()
            .as_trait::<dyn RegionBranchOpInterface>()
            .expect("invalid region terminator parent: must implement RegionBranchOpInterface")
            .get_successor_regions(RegionBranchPoint::Child(parent_region))
            .into_successor_infos()
    }
}

pub struct RegionSuccessorIter<'a> {
    op: &'a Operation,
    successors: SmallVec<[RegionSuccessorInfo; 2]>,
    index: usize,
}
impl<'a> RegionSuccessorIter<'a> {
    pub fn new(
        op: &'a Operation,
        successors: impl IntoIterator<Item = RegionSuccessorInfo>,
    ) -> Self {
        Self {
            op,
            successors: SmallVec::from_iter(successors),
            index: 0,
        }
    }

    pub fn empty(op: &'a Operation) -> Self {
        Self {
            op,
            successors: Default::default(),
            index: 0,
        }
    }

    pub fn get(&self, index: usize) -> Option<RegionSuccessor<'a>> {
        self.successors.get(index).map(|info| RegionSuccessor {
            dest: info.successor.clone(),
            arguments: self.op.operands().group(info.operand_group as usize),
        })
    }

    pub fn into_successor_infos(self) -> SmallVec<[RegionSuccessorInfo; 2]> {
        self.successors
    }
}
impl core::iter::FusedIterator for RegionSuccessorIter<'_> {}
impl<'a> ExactSizeIterator for RegionSuccessorIter<'a> {
    fn len(&self) -> usize {
        self.successors.len()
    }
}
impl<'a> Iterator for RegionSuccessorIter<'a> {
    type Item = RegionSuccessor<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.successors.len() {
            return None;
        }

        let info = &self.successors[self.index];
        Some(RegionSuccessor {
            dest: info.successor.clone(),
            arguments: self.op.operands().group(info.operand_group as usize),
        })
    }
}
