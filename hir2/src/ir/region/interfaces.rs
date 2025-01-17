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
    fn is_repetitive_region(&self, index: usize) -> bool {
        self.region(index).is_repetitive_region()
    }
    /// Returns `true` if there is a loop in the region branching graph.
    ///
    /// Only reachable regions (starting from the entry region) are considered.
    fn has_loop(&self) -> bool {
        self.get_successor_regions(RegionBranchPoint::Parent)
            .filter_map(|entry| entry.into_successor())
            .any(|region| {
                Region::traverse_region_graph(&region.borrow(), |r, visited| {
                    // Interrupted traversal if the region was already visited
                    visited.contains(&r.as_region_ref())
                })
            })
    }
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

/// This trait is implemented by operations which have loop-like semantics.
///
/// It provides useful helpers and access to properties of the loop represented, and is used in
/// order to perform transformations on the loop. Implementors will be considered by loop-invariant
/// code motion.
///
/// Loop-carried variables can be exposed through this interface. There are 3 components to a
/// loop-carried variable:
///
/// - The "region iter_arg" is the block argument of the entry block that represents the loop-
///   carried variable in each iteration.
/// - The "init value" is an operand of the loop op that serves as the initial region iter_arg value
///   for the first iteration (if any).
/// - The "yielded" value is the value that is forwarded from one iteration to serve as the region
///   iter_arg of the next iteration.
///
/// If one of the respective interface methods is implemented, so must the other two. The interface
/// verifier ensures that the number of types of the region iter_args, init values and yielded
/// values match.
///
/// Optionally, "loop results" can be exposed through this interface. These are the values that are
/// returned from the loop op when there are no more iterations. The number and types of the loop
/// results must match with the region iter_args. Note: Loop results are optional because some loops
/// (e.g., `scf.while`) may produce results that do match 1-to-1 with the region iter_args.
#[allow(unused_variables)]
#[allow(clippy::result_unit_err)]
pub trait LoopLikeOpInterface: Op {
    /// Returns true if the given value is defined outside of the loop.
    ///
    /// A sensible implementation could be to check whether the value's defining operation lies
    /// outside of the loops body region. If the loop uses explicit capture of dependencies, an
    /// implementation could check whether the value corresponds to a captured dependency.
    fn is_defined_outside_of_loop(&self, value: ValueRef) -> bool {
        let value = value.borrow();
        if let Some(defining_op) = value.get_defining_op() {
            self.as_operation().is_ancestor_of(&defining_op.borrow())
        } else {
            let block_arg = value
                .downcast_ref::<BlockArgument>()
                .expect("invalid value reference: defining op is orphaned");
            let defining_region = block_arg.parent_region().unwrap();
            let defining_op = defining_region.borrow().parent().unwrap();
            self.as_operation().is_ancestor_of(&defining_op.borrow())
        }
    }

    /// Returns the entry region for this loop, which is expected to also play the role of loop
    /// header.
    ///
    /// NOTE: It is expected that if the loop has iteration arguments, that the values returned
    /// from `Self::get_region_iter_args` correspond to block arguments of the header region.
    /// Additionally, it is presumed that initialization variables expected by the op are provided
    /// to the loop body via block arguments of this region.
    fn get_loop_header_region(&self) -> RegionRef;

    /// Returns the regions that make up the body of the loop, and should be inspected for loop-
    /// invariant operations.
    fn get_loop_regions(&self) -> SmallVec<[RegionRef; 2]>;

    /// Moves the given loop-invariant operation out of the loop.
    fn move_out_of_loop(&mut self, mut op: OperationRef) {
        op.borrow_mut().move_to(crate::ProgramPoint::before(self.as_operation()));
    }

    /// Promotes the loop body to its containing block if the loop is known to have a single
    /// iteration.
    ///
    /// Returns `Ok` if the promotion was successful
    fn promote_if_single_iteration(
        &mut self,
        rewriter: &mut dyn crate::Rewriter,
    ) -> Result<(), ()> {
        Err(())
    }

    /// Return all induction variables, if they exist.
    ///
    /// If the op has no notion of induction variable, then return `None`. If it does have a notion
    /// but an instance doesn't have induction variables, then return an empty vector.
    fn get_loop_induction_vars(&self) -> Option<SmallVec<[ValueRef; 2]>> {
        None
    }

    /// Return all lower bounds, if they exist.
    ///
    /// If the op has no notion of lower bounds, then return `None`. If it does have a notion but an
    /// instance doesn't have lower bounds, then return an empty vector.
    fn get_loop_lower_bounds(&self) -> Option<SmallVec<[OpFoldResult; 2]>> {
        None
    }

    /// Return all upper bounds, if they exist.
    ///
    /// If the op has no notion of upper bounds, then return `None`. If it does have a notion but an
    /// instance doesn't have upper bounds, then return an empty vector.
    fn get_loop_upper_bounds(&self) -> Option<SmallVec<[OpFoldResult; 2]>> {
        None
    }

    /// Return all steps, if they exist.
    ///
    /// If the op has no notion of steps, then return `None`. If it does have a notion but an
    /// instance doesn't have steps, then return an empty vector.
    fn get_loop_steps(&self) -> Option<SmallVec<[OpFoldResult; 2]>> {
        None
    }

    /// Return the mutable "init" operands that are used as initialization values for the region
    /// "iter_args" of this loop.
    fn get_inits_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().empty_mut()
    }

    /// Return the region "iter_args" (block arguments) that correspond to the "init" operands.
    ///
    /// If the op has multiple regions, return the corresponding block arguments of the entry region.
    fn get_region_iter_args(&self) -> Option<EntityRef<'_, [BlockArgumentRef]>> {
        None
    }

    /// Return the mutable operand range of values that are yielded to the next iteration by the
    /// loop terminator.
    ///
    /// For loop operations that dont yield a value, this should return `None`.
    fn get_yielded_values_mut(&mut self) -> Option<EntityProjectionMut<'_, OpOperandRangeMut<'_>>> {
        None
    }

    /// Return the range of results that are return from this loop and correspond to the "init"
    /// operands.
    ///
    /// Note: This interface method is optional. If loop results are not exposed via this interface,
    /// `None` should be returned.
    ///
    /// Otherwise, the number and types of results must match with the region iter_args, inits and
    /// yielded values that are exposed via this interface. If loop results are exposed but this
    /// loop op has no loop-carried variables, an empty result range (and not `None`) should be
    /// returned.
    fn get_loop_results(&self) -> Option<OpResultRange<'_>> {
        None
    }
}

impl dyn LoopLikeOpInterface {
    /// If there is a single induction variable return it, otherwise return `None`
    pub fn get_single_induction_var(&self) -> Option<ValueRef> {
        let vars = self.get_loop_induction_vars();
        if let Some([var]) = vars.as_deref() {
            return Some(*var);
        }
        None
    }

    /// Return the single lower bound value or attribute if it exists, otherwise return `None`
    pub fn get_single_lower_bound(&self) -> Option<OpFoldResult> {
        let mut lower_bounds = self.get_loop_lower_bounds()?;
        if lower_bounds.len() == 1 {
            lower_bounds.pop()
        } else {
            None
        }
    }

    /// Return the single upper bound value or attribute if it exists, otherwise return `None`
    pub fn get_single_upper_bound(&self) -> Option<OpFoldResult> {
        let mut upper_bounds = self.get_loop_upper_bounds()?;
        if upper_bounds.len() == 1 {
            upper_bounds.pop()
        } else {
            None
        }
    }

    /// Return the single step value or attribute if it exists, otherwise return `None`
    pub fn get_single_step(&self) -> Option<OpFoldResult> {
        let mut steps = self.get_loop_steps()?;
        if steps.len() == 1 {
            steps.pop()
        } else {
            None
        }
    }
}
