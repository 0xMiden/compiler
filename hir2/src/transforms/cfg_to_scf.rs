//! This code is an implementation of the algorithm described in _Perfect Reconstructability of
//! Control Flow from Demand Dependence Graphs_, by Bahmann, Reismann, Jahre, and Meyer. 2015.
//! See https://doi.org/10.1145/2693261.
//!
//! It defines an algorithm to translate any control flow graph with a single entry and single exit
//! block into structured control flow operations consisting of regions of do-while loops and
//! operations conditionally dispatching to one out of multiple regions before continuing after the
//! operation. This includes control flow graphs containing irreducible control flow.
//!
//! The implementation here additionally supports the transformation on regions with multiple exit
//! blocks. This is implemented by first transforming all occurrences of return-like operations to
//! branch to a single exit block containing an instance of that return-like operation. If there are
//! multiple kinds of return-like operations, multiple exit blocks are created. In that case the
//! transformation leaves behind a conditional control flow graph operation that dispatches to the
//! given regions terminating with different kinds of return-like operations each.
//!
//! If the function only contains a single kind of return-like operations, it is guaranteed that all
//! control flow graph ops will be lifted to structured control flow, and that no more control flow
//! graph ops remain after the operation.
//!
//! The algorithm to lift CFGs consists of two transformations applied after each other on any
//! single-entry, single-exit region:
//!
//! 1. Lifting cycles to structured control flow loops
//! 2. Lifting conditional branches to structured control flow branches
//!
//! These are then applied recursively on any new single-entry single-exit regions created by the
//! transformation until no more CFG operations remain.
//!
//! The first part of cycle lifting is to detect any cycles in the CFG. This is done using an
//! algorithm for iterating over SCCs. Every SCC representing a cycle is then transformed into a
//! structured loop with a single entry block and a single latch containing the only back edge to
//! the entry block and the only edge to an exit block outside the loop. Rerouting control flow to
//! create single entry and exit blocks is achieved via a multiplexer construct that can be
//! visualized as follows:
//!
//! ```text,ignore
//! +-----+ +-----+   +-----+
//! | bb0 | | bb1 |...| bbN |
//! +--+--+ +--+--+   +-+---+
//!    |       |        |
//!    |       v        |
//!    |  +------+      |
//!    | ++      ++<----+
//!    | | Region |
//!    +>|        |<----+
//!      ++      ++     |
//!       +------+------+
//! ```
//!
//! The above transforms to:
//!
//! ```text,ignore
//! +-----+ +-----+   +-----+
//! | bb0 | | bb1 |...| bbN |
//! +-----+ +--|--+   ++----+
//!      |     v       |
//!      +->+-----+<---+
//!         | bbM |<-------+
//!         +---+-+        |
//!     +---+   | +----+   |
//!     |       v      |   |
//!     |   +------+   |   |
//!     |  ++      ++<-+   |
//!     +->| Region |      |
//!        ++      ++      |
//!         +------+-------+
//! ```
//!
//! bbM in the above is the multiplexer block, and any block previously branching to an entry block
//! of the region are redirected to it. This includes any branches from within the region. Using a
//! block argument, bbM then dispatches to the correct entry block of the region dependent on the
//! predecessor.
//!
//! A similar transformation is done to create the latch block with the single back edge and loop
//! exit edge.
//!
//! The above form has the advantage that bbM now acts as the loop header of the loop body. After
//! the transformation on the latch, this results in a structured loop that can then be lifted to
//! structured control flow. The conditional branches created in bbM are later lifted to conditional
//! branches.
//!
//! Lifting conditional branches is done by analyzing the *first* conditional branch encountered in
//! the entry region. The algorithm then identifies all blocks that are dominated by a specific
//! control flow edge and the region where control flow continues:
//!
//! ```text,ignore
//!                  +-----+
//!            +-----+ bb0 +----+
//!            v     +-----+    v
//! Region 1 +-+-+    ...     +-+-+ Region n
//!          +---+            +---+
//!           ...              ...
//!            |                |
//!            |      +---+     |
//!            +---->++   ++<---+
//!                  |     |
//!                  ++   ++ Region T
//!                   +---+
//! ```
//!
//! Every region following bb0 consists of 0 or more blocks that eventually branch to Region T. If
//! there are multiple entry blocks into Region T, a single entry block is created using a
//! multiplexer block as shown above. Region 1 to Region n are then lifted together with the
//! conditional control flow operation terminating bb0 into a structured conditional operation
//! followed by the operations of the entry block of Region T.
mod edges;
mod transform;

use smallvec::SmallVec;

use self::transform::TransformationContext;
use crate::{
    adt::SmallSet, dominance::DominanceInfo, traits::BranchOpInterface, BlockRef, Builder,
    OpBuilder, OpOperand, Operation, OperationRef, Region, RegionRef, Report, SourceSpan, Type,
    Value, ValueRef, WalkResult,
};

/// This trait is used to abstract over the dialect-specific aspects of the control flow lifting
/// transformation performed by [transform_cfg_to_scf].
///
/// Implementations must be able to create switch-like control flow operations in order to
/// facilitate intermediate transformations; as well as the various structured control flow ops
/// represented by each method (e.g. `scf.if`, `scf.while`).
pub trait CFGToSCFInterface {
    /// Creates a structured control flow operation branching to one of `regions`.
    ///
    /// It replaces `control_flow_cond_op` and must produce `result_types` as results.
    ///
    /// `regions` contains the list of branch regions corresponding to each successor of
    /// `control_flow_cond_op`. Their bodies must simply be taken and left as is.
    ///
    /// Returns `Err` if incapable of converting the control flow graph operation.
    fn create_structured_branch_region_op(
        &self,
        builder: &mut OpBuilder,
        control_flow_cond_op: OperationRef,
        result_types: &[Type],
        regions: &mut SmallVec<[RegionRef; 2]>,
    ) -> Result<OperationRef, Report>;

    /// Creates a return-like terminator for a branch region of the op returned by
    /// [CFGToSCFInterface::create_structured_branch_region_op].
    ///
    /// * `branch_region_op` is the operation returned by `create_structured_branch_region_op`.
    /// * `replaced_control_flow_op` is the control flow op being replaced by the terminator or
    ///   `None` if the terminator is not replacing any existing control flow op.
    /// * `results` are the values that should be returned by the branch region.
    fn create_structured_branch_region_terminator_op(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        branch_region_op: OperationRef,
        replaced_control_flow_op: Option<OperationRef>,
        results: &[ValueRef],
    ) -> Result<(), Report>;

    /// Creates a structured control flow operation representing a do-while loop.
    ///
    /// The do-while loop is expected to have the exact same result types as the types of the
    /// iteration values. `loop_body` is the body of the loop.
    ///
    /// Implementations must create a suitable terminator op at the end of the last block in
    /// `loop_body` which continues the loop if `condition` is 1, and exits the loop if 0.
    ///
    /// `loop_values_next_iter` are the values that have to be passed as the iteration values for
    /// the next iteration if continuing, or the result of the loop if exiting.
    ///
    /// `condition` is guaranteed to be of the same type as values returned by
    /// `get_cfg_switch_value` with either 0 or 1 as value.
    ///
    /// `loop_values_init` are the values used to initialize the iteration values of the loop.
    ///
    /// Returns `Err` if incapable of creating a loop op.
    fn create_structured_do_while_loop_op(
        &self,
        builder: &mut OpBuilder,
        replaced_op: OperationRef,
        loop_values_init: &[ValueRef],
        condition: ValueRef,
        loop_values_next_iter: &[ValueRef],
        loop_body: RegionRef,
    ) -> Result<OperationRef, Report>;

    /// Creates a constant operation with a result representing `value` that is suitable as flag
    /// for `create_cfg_switch_op`.
    fn get_cfg_switch_value(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        value: u32,
    ) -> ValueRef;

    /// Creates a switch-like unstructured branch operation, branching to one of `case_destinations`
    /// or `default_dest`.
    ///
    /// This is used by [transform_cfg_to_scfg] for intermediate transformations before lifting to
    /// structured control flow.
    ///
    /// The switch op branches based on `flag` which is guaranteed to be of the same type as values
    /// returned by `get_cfg_switch_value`. The insertion block of the builder is guaranteed to have
    /// its predecessors already set to create an equivalent CFG after this operation.
    ///
    /// NOTE: `case_values` and other related slices may be empty to represent an unconditional
    /// branch.
    #[allow(clippy::too_many_arguments)]
    fn create_cfg_switch_op(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        flag: ValueRef,
        case_values: &[u32],
        case_destinations: &[BlockRef],
        case_arguments: &[&[ValueRef]],
        default_dest: BlockRef,
        default_args: &[ValueRef],
    ) -> Result<(), Report>;

    /// Creates a constant operation returning an undefined instance of `type`.
    ///
    /// This is required by the transformation as the lifting process might create control flow
    /// paths where an SSA-value is undefined.
    fn get_undef_value(&self, span: SourceSpan, builder: &mut OpBuilder, ty: Type) -> ValueRef;

    /// Creates a return-like terminator indicating unreachable.
    ///
    /// This is required when the transformation encounters a statically known infinite loop. Since
    /// structured control flow ops are not terminators, after lifting an infinite loop, a
    /// terminator has to be placed after to possibly satisfy the terminator requirement of the
    /// region originally passed to [transform_cfg_to_scf].
    ///
    /// `region` is guaranteed to be the region originally passed to [transform_cfg_to_scf] and the
    /// op is guaranteed to always be an op in a block directly nested under `region` after the
    /// transformation.
    ///
    /// Returns `Err` if incapable of creating an unreachable terminator.
    fn create_unreachable_terminator(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        region: RegionRef,
    ) -> Result<OperationRef, Report>;

    /// Helper function to create an unconditional branch using [create_cfg_switch_op].
    fn create_single_destination_branch(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        dummy_flag: ValueRef,
        destination: BlockRef,
        arguments: &[ValueRef],
    ) -> Result<(), Report> {
        self.create_cfg_switch_op(span, builder, dummy_flag, &[], &[], &[], destination, arguments)
    }

    /// Helper function to create a conditional branch using [create_cfg_switch_op].
    #[allow(clippy::too_many_arguments)]
    fn create_conditional_branch(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        condition: ValueRef,
        true_dest: BlockRef,
        true_args: &[ValueRef],
        false_dest: BlockRef,
        false_args: &[ValueRef],
    ) -> Result<(), Report> {
        self.create_cfg_switch_op(
            span,
            builder,
            condition,
            &[0],
            &[false_dest],
            &[false_args],
            true_dest,
            true_args,
        )
    }
}

/// This function transforms all unstructured control flow operations within `region`, to equivalent
/// structured control flow operations.
///
/// This transformation is dialect-agnostic, facilitated by delegating the dialect-specific aspects
/// of lifting operations, to the implementation of [CFGToSCFInterface] that is provided.
///
/// If the region contains only a single kind of return-like operation, all control flow graph
/// operations will be converted successfully. Otherwise a single control flow graph operation
/// branching to one block per return-like operation kind remains.
///
/// The transformation currently requires that all control flow graph operations have no side
/// effects, implement the [crate::traits::BranchOpInterface], and do not have any operation-
/// produced successor operands.
///
/// Returns `Err` if any of the preconditions are violated or if any of the methods of `interface`
/// failed. The IR is left in an unspecified state in such cases.
///
/// If successful, returns a boolean indicating whether the IR was changed.
pub fn transform_cfg_to_scf(
    region: RegionRef,
    interface: &mut dyn CFGToSCFInterface,
    dominance_info: &mut DominanceInfo,
) -> Result<bool, Report> {
    {
        let region = region.borrow();
        if region.is_empty() || region.has_one_block() {
            return Ok(false);
        }

        check_transformation_preconditions(&region)?;
    }

    let mut transform_ctx = TransformationContext::new(region, interface, dominance_info)?;

    let mut worklist = SmallVec::<[BlockRef; 4]>::from_slice(&[transform_ctx.entry()]);
    while let Some(current) = worklist.pop() {
        // Turn all top-level cycles in the CFG to structured control flow first.
        // After this transformation, the remaining CFG ops form a DAG.
        let mut new_regions = transform_ctx.transform_cycles_to_scf_loops(current)?;

        // Add the newly created subregions to the worklist. These are the bodies of the loops.
        worklist.extend(new_regions.iter().copied());
        // Invalidate the dominance tree as blocks have been moved, created and added during the
        // cycle to structured loop transformation.
        if !new_regions.is_empty() {
            let current = current.borrow();
            let parent_region = current.parent().unwrap();
            transform_ctx.invalidate_dominance_info_for_region(parent_region);
        }
        new_regions = transform_ctx.transform_to_structured_cf_branches(current)?;
        // Invalidating the dominance tree is generally not required by the transformation above as
        // the new region entries correspond to unaffected subtrees in the dominator tree. Only its
        // parent nodes have changed but won't be visited again.
        worklist.extend(new_regions);
    }

    // Clean up garbage we may have created during the transformation
    //
    // NOTE: This is not guaranteed to clean up _everything_ that may be garbage, only things we
    // have accounted for. Canonicalization and other optimization passes can take care of anything
    // else that may remain
    transform_ctx.garbage_collect();

    Ok(true)
}

/// Checks all preconditions of the transformation prior to any transformations.
///
/// Returns failure if any precondition is violated.
fn check_transformation_preconditions(region: &Region) -> Result<(), Report> {
    use crate::{SuccessorOperands, Walk};

    for block in region.body() {
        if !block.has_predecessors() && !block.is_entry_block() {
            return Err(Report::msg("transformation does not support unreachable blocks"));
        }
    }

    let walk_result = region.prewalk_interruptible(|op: &Operation| {
        if !op.has_successors() {
            return WalkResult::Skip;
        }

        // This transformation requires all ops with successors to implement the branch op interface.
        // It is impossible to adjust their block arguments otherwise.
        let branch_op_interface = match op.as_trait::<dyn BranchOpInterface>().ok_or_else(|| {
            Report::msg(
                "transformation does not support terminators with successors not implementing \
                 BranchOpInterface",
            )
        }) {
            Ok(boi) => boi,
            Err(err) => return WalkResult::Break(err),
        };

        // Branch operations must have no side effects. Replacing them would not be valid otherwise.
        if !op.is_memory_effect_free() {
            return WalkResult::Break(Report::msg(
                "transformation does not support terminators with side effects",
            ));
        }

        for index in 0..op.num_successors() {
            let succ_ops = branch_op_interface.get_successor_operands(index);

            // We cannot support operations with operation-produced successor operands as it is
            // currently not possible to pass them to any block arguments other than the first. This
            // breaks creating multiplexer blocks and would likely need special handling elsewhere
            // too.
            if succ_ops.num_produced() == 0 {
                continue;
            }

            return WalkResult::Break(Report::msg(
                "transformation does not support operations with operation-produced successor \
                 operands",
            ));
        }

        WalkResult::Continue(())
    });

    walk_result.into_result()
}
