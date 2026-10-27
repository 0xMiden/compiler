//! This code is an implementation of the algorithm described in [_Perfect Reconstructability of
//! Control Flow from Demand Dependence Graphs_, by Bahmann, Reismann, Jahre, and Meyer. 2015.](
//! https://doi.org/10.1145/2693261).
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

use midenc_hir::{
    AsValueRange, BlockRef, Builder, OpBuilder, Operation, OperationRef, Region, RegionRef, Report,
    SmallVec, SourceSpan, Spanned, Type, Value, ValueRange, ValueRef, WalkResult, adt::SmallSet,
    dominance::DominanceInfo, traits::BranchOpInterface,
};

use self::transform::TransformationContext;

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
        results: ValueRange<'_, 2>,
    ) -> Result<(), Report>;

    /// Creates a structured control flow operation representing a do-while loop.
    ///
    /// The do-while loop is expected to have the exact same result types as the types of the
    /// iteration values. `loop_body` is the body of the loop.
    ///
    /// Implementations must create a suitable terminator op at the end of the last block in
    /// `loop_body` which continues the loop if `condition` is true, and exits the loop if false.
    ///
    /// `loop_values_next_iter` are the values that have to be passed as the iteration values for
    /// the next iteration if continuing, or the result of the loop if exiting.
    ///
    /// `condition` is an `i1`, the type of the values returned by `get_cfg_bool_value`.
    ///
    /// `loop_values_init` are the values used to initialize the iteration values of the loop.
    ///
    /// Returns `Err` if incapable of creating a loop op.
    fn create_structured_do_while_loop_op(
        &self,
        builder: &mut OpBuilder,
        replaced_op: OperationRef,
        loop_values_init: ValueRange<'_, 2>,
        condition: ValueRef,
        loop_values_next_iter: ValueRange<'_, 2>,
        loop_body: RegionRef,
    ) -> Result<OperationRef, Report>;

    /// Creates a constant boolean operation with a result representing `value`, suitable as the
    /// condition of `create_conditional_branch` and `create_structured_do_while_loop_op`.
    ///
    /// The result must be an `i1`: the transformation itself creates `i1` block arguments to carry
    /// these values.
    fn get_cfg_bool_value(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        value: bool,
    ) -> ValueRef;

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
    /// This is used by [transform_cfg_to_scf] for intermediate transformations before lifting
    /// to structured control flow.
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
        case_arguments: &[ValueRange<'_, 2>],
        default_dest: BlockRef,
        default_args: ValueRange<'_, 2>,
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

    /// Helper function to create an unconditional branch using [Self::create_cfg_switch_op].
    fn create_single_destination_branch(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        dummy_flag: ValueRef,
        destination: BlockRef,
        arguments: ValueRange<'_, 2>,
    ) -> Result<(), Report> {
        self.create_cfg_switch_op(span, builder, dummy_flag, &[], &[], &[], destination, arguments)
    }

    /// Creates a two-way conditional branch terminator at the current insertion point of
    /// `builder`, branching on `condition`, an `i1` like the values returned by
    /// [Self::get_cfg_bool_value].
    ///
    /// Control transfers to `true_dest` with `true_args` if `condition` is true, and to
    /// `false_dest` with `false_args` otherwise.
    #[allow(clippy::too_many_arguments)]
    fn create_conditional_branch(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        condition: ValueRef,
        true_dest: BlockRef,
        true_args: ValueRange<'_, 2>,
        false_dest: BlockRef,
        false_args: ValueRange<'_, 2>,
    ) -> Result<(), Report>;
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
/// effects, implement the [midenc_hir::traits::BranchOpInterface], and do not have any operation-
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
        if region.is_empty()
            || (region.has_one_block()
                && !region.entry_block_ref().unwrap().borrow().has_predecessors())
        {
            return Ok(false);
        }

        check_transformation_preconditions(&region)?;
    }

    // Make the implicit initial activation explicit before discovering cycles. The former
    // entry remains the target of back edges and retains its loop-carried arguments.
    let entry = region.borrow().entry_block_ref().unwrap();
    if entry.borrow().has_predecessors() {
        let parent = region.parent().unwrap();
        let parent = parent.borrow();
        let context = parent.context_rc();
        let span = parent.span();
        drop(parent);
        let mut preheader = context.create_block();
        for argument in entry.borrow().arguments() {
            let argument = argument.borrow();
            context.append_block_argument(preheader, argument.ty().clone(), argument.span());
        }
        preheader.borrow_mut().insert_before(entry);
        let arguments = preheader.borrow().arguments().as_value_range().into_owned();
        let mut builder = OpBuilder::new(context);
        builder.set_insertion_point_to_end(preheader);
        let flag = interface.get_cfg_switch_value(span, &mut builder, 0);
        interface.create_single_destination_branch(span, &mut builder, flag, entry, arguments)?;
        dominance_info.invalidate_region(region);
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
    use midenc_hir::SuccessorOperands;

    for block in region.body() {
        if !block.has_predecessors() && !block.is_entry_block() {
            return Err(Report::msg("transformation does not support unreachable blocks"));
        }
    }

    let walk_result = region.prewalk(|op: &Operation| {
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

#[cfg(test)]
mod entry_cycle_tests {
    use alloc::{boxed::Box, rc::Rc};

    use midenc_hir::{
        Context, Forward, OperationRef, RawWalk, Report,
        diagnostics::Uri,
        dialects::builtin::Function,
        parse::{self, ParserConfig},
        pass::{Nesting, PassManager},
    };

    fn lift(source: &str, entry_placeholder: bool) -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("entry-cycle.hir"),
            source,
        )?;
        if entry_placeholder {
            // Named function parameters require an unlabeled entry in textual HIR. Retarget
            // the typed placeholder edges to construct the corresponding in-memory CFG.
            let entry = function.borrow().body().entry_block_ref().unwrap();
            let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
            function
                .as_operation_ref()
                .raw_prewalk_all::<Forward, _>(|mut op: OperationRef| {
                    let mut op = op.borrow_mut();
                    for index in 0..op.num_successors() {
                        if op.successor(index).successor() == placeholder {
                            op.successor_mut(index).set(entry);
                        }
                    }
                });
            placeholder.borrow_mut().erase();
        }
        let signature = function.borrow().signature().clone();
        let mut passes = PassManager::on::<Function>(context, Nesting::Implicit);
        passes.add_pass(Box::new(midenc_dialect_scf::transforms::LiftControlFlowToSCF));
        passes.run(function.as_operation_ref())?;
        assert_eq!(*function.borrow().signature(), signature);
        function.as_operation_ref().borrow().recursively_verify()?;
        function.as_operation_ref().raw_prewalk_all::<Forward, _>(|op: OperationRef| {
            assert!(!op.borrow().has_successors(), "unstructured branch remains");
        });
        Ok(())
    }

    #[test]
    fn entry_cycle_self_loop_without_arguments() -> Result<(), Report> {
        lift(
            r#"builtin.function public extern("C") @cycle() { ^entry: cf.br ^entry; };"#,
            false,
        )
    }

    #[test]
    fn entry_cycle_self_loop_with_arguments() -> Result<(), Report> {
        lift(
            r#"builtin.function public extern("C") @cycle(%a: u32, %b: u64) {
            cf.br ^back(%a, %b : u32, u64);
            ^back(%x: u32, %y: u64): ub.unreachable;
        };"#,
            true,
        )
    }

    #[test]
    fn entry_cycle_multiple_blocks() -> Result<(), Report> {
        lift(
            r#"builtin.function public extern("C") @cycle() {
            ^entry: cf.br ^next;
            ^next: cf.br ^entry;
        };"#,
            false,
        )
    }

    #[test]
    fn entry_cycle_conditional_exit_with_mixed_arguments() -> Result<(), Report> {
        lift(
            r#"builtin.function public extern("C") @cycle(%a: u32, %b: u64) -> (u32, u64) {
                %one = arith.constant 1 : u32;
                %wide = arith.constant 7 : u64;
                %next_a = arith.add %a, %one <{ overflow = #builtin.overflow<unchecked> }>;
                %next_b = arith.add %b, %wide <{ overflow = #builtin.overflow<unchecked> }>;
                %limit = arith.constant 4 : u32;
                %done = arith.eq %next_a, %limit;
                cf.cond_br %done ^exit, ^again : (i1);
            ^again: cf.br ^back(%next_a, %next_b : u32, u64);
            ^exit: builtin.ret %next_a, %next_b : (u32, u64);
            ^back(%x: u32, %y: u64): ub.unreachable;
        };"#,
            true,
        )
    }

    #[test]
    fn entry_cycle_irreducible_region() -> Result<(), Report> {
        lift(
            r#"builtin.function public extern("C") @cycle(%c: i1) {
                cf.cond_br %c ^left, ^right : (i1);
            ^left: cf.cond_br %c ^right, ^exit : (i1);
            ^right: cf.cond_br %c ^left, ^exit : (i1);
            ^exit: builtin.ret;
        };"#,
            false,
        )
    }

    #[test]
    fn entry_cycle_acyclic_single_block() -> Result<(), Report> {
        lift(
            r#"builtin.function public extern("C") @cycle(%a: u32) -> u32 {
            builtin.ret %a : u32;
        };"#,
            false,
        )
    }
}

#[cfg(test)]
mod canonical_entry_tests {
    use alloc::rc::Rc;

    use midenc_hir::{
        AsValueRange, Context, Region, Report, Spanned, Value,
        diagnostics::Uri,
        dialects::builtin::Function,
        parse::{self, ParserConfig},
        patterns::{RegionSimplificationLevel, RewriterImpl},
    };

    #[test]
    fn canonical_redundant_two_arguments() -> Result<(), Report> {
        redundant_arguments(false)
    }

    #[test]
    fn canonical_redundant_mixed_arguments() -> Result<(), Report> {
        redundant_arguments(true)
    }

    fn redundant_arguments(mixed: bool) -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = if mixed {
            r#"builtin.function public extern("C") @args(%c: i1, %a: u32, %b: u64, %d: u32, %e: u32) -> (u32, u32, u64) {
            cf.cond_br %c ^join(%a, %d, %b : u32, u32, u64), ^join(%a, %e, %b : u32, u32, u64) : (i1);
        ^join(%x: u32, %retained: u32, %y: u64): builtin.ret %x, %retained, %y : (u32, u32, u64);
        };"#
        } else {
            r#"builtin.function public extern("C") @args(%c: i1, %a: u32, %b: u64) -> (u32, u64) {
            cf.cond_br %c ^join(%a, %b : u32, u64), ^join(%a, %b : u32, u64) : (i1);
        ^join(%x: u32, %y: u64): builtin.ret %x, %y : (u32, u64);
        };"#
        };
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("redundant.hir"),
            source,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let join = function.borrow().body().body().back().as_pointer().unwrap();
        let retained = if mixed {
            Some(join.borrow().arguments()[1])
        } else {
            None
        };
        let region = function.borrow().body().as_region_ref();
        let mut rewriter: RewriterImpl = RewriterImpl::new(context);
        let _ =
            Region::simplify_all(&[region], &mut rewriter, RegionSimplificationLevel::Aggressive);
        if let Some(retained) = retained {
            assert_eq!(join.borrow().arguments().len(), 1);
            assert_eq!(join.borrow().arguments()[0], retained);
            let branch = entry.borrow().terminator().unwrap();
            let branch = branch.borrow();
            for i in 0..2 {
                let operands = branch.successor(i).arguments;
                assert_eq!(operands.len(), 1);
                assert_eq!(
                    operands[0].borrow().as_value_ref(),
                    entry.borrow().arguments().as_value_range().get(3 + i).unwrap()
                );
            }
        } else {
            assert!(join.borrow().arguments().is_empty());
        }
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }

    #[test]
    fn canonical_entry_normal_preserves_initial_interface() -> Result<(), Report> {
        preserve_entry(RegionSimplificationLevel::Normal, false)
    }

    #[test]
    fn canonical_entry_aggressive_preserves_initial_interface() -> Result<(), Report> {
        preserve_entry(RegionSimplificationLevel::Aggressive, false)
    }

    #[test]
    fn canonical_entry_invariant_i1_normal_preserves_initial_interface() -> Result<(), Report> {
        preserve_entry(RegionSimplificationLevel::Normal, true)
    }

    #[test]
    fn canonical_entry_invariant_i1_aggressive_preserves_initial_interface() -> Result<(), Report> {
        preserve_entry(RegionSimplificationLevel::Aggressive, true)
    }

    #[test]
    fn canonical_entry_aggressive_region_preserves_initial_interface() -> Result<(), Report> {
        preserve_entry_with_config(RegionSimplificationLevel::Aggressive, false, false)
    }

    fn preserve_entry(level: RegionSimplificationLevel, invariant: bool) -> Result<(), Report> {
        preserve_entry_with_config(level, invariant, true)
    }

    fn preserve_entry_with_config(
        level: RegionSimplificationLevel,
        invariant: bool,
        patterns: bool,
    ) -> Result<(), Report> {
        use alloc::{boxed::Box, vec::Vec};

        use midenc_hir::{
            pass::{Nesting, PassManager},
            patterns::GreedyRewriteConfig,
        };
        let context = Rc::new(Context::default());
        let source = r#"builtin.function public extern("C") @cycle(%a: u32, %b: u64) -> (u32, u64) {
            %one = arith.constant 1 : u32;
            %wide = arith.constant 7 : u64;
            %next_a = arith.add %a, %one <{ overflow = #builtin.overflow<unchecked> }>;
            %next_b = arith.add %b, %wide <{ overflow = #builtin.overflow<unchecked> }>;
            %limit = arith.constant 4 : u32;
            %done = arith.eq %next_a, %limit;
            cf.cond_br %done ^exit, ^again : (i1);
        ^again: cf.br ^placeholder(%next_a, %next_b : u32, u64);
        ^exit: builtin.ret %next_a, %next_b : (u32, u64);
        ^placeholder(%y: u32, %z: u64): builtin.ret %y, %z : (u32, u64);
        };"#;
        let source = if invariant {
            source
                .replace("@cycle(%a: u32, %b: u64)", "@cycle(%c: i1, %a: u32, %b: u64)")
                .replace("cf.cond_br %done", "cf.cond_br %c")
                .replace(
                    "^placeholder(%next_a, %next_b : u32, u64)",
                    "^placeholder(%c, %next_a, %next_b : i1, u32, u64)",
                )
                .replace("^placeholder(%y: u32, %z: u64)", "^placeholder(%x: i1, %y: u32, %z: u64)")
        } else {
            source.into()
        };
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("entry.hir"),
            &source,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let args = entry.borrow().arguments().to_vec();
        let initial = args
            .iter()
            .map(|arg| {
                let arg = arg.borrow();
                (arg.ty().clone(), arg.span())
            })
            .collect::<Vec<_>>();
        let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
        let mut branch = placeholder
            .borrow()
            .get_single_predecessor()
            .unwrap()
            .borrow()
            .terminator()
            .unwrap();
        let expected_backedge = branch
            .borrow()
            .successor(0)
            .arguments
            .iter()
            .map(|operand| operand.borrow().as_value_ref())
            .collect::<Vec<_>>();
        let evolving_start = usize::from(invariant);
        assert_eq!(expected_backedge.len(), evolving_start + 2);
        if invariant {
            assert_eq!(expected_backedge[0], args[0].borrow().as_value_ref());
        }
        for value in &expected_backedge[evolving_start..] {
            assert!(
                value
                    .borrow()
                    .get_defining_op()
                    .unwrap()
                    .borrow()
                    .is::<midenc_dialect_arith::Add>()
            );
        }
        branch.borrow_mut().successor_mut(0).set(entry);
        placeholder.borrow_mut().erase();
        function.as_operation_ref().borrow().recursively_verify()?;
        let mut config = GreedyRewriteConfig::default();
        config.with_region_simplification_level(level);
        let mut passes = PassManager::on::<Function>(context.clone(), Nesting::Implicit);
        if patterns {
            passes.add_pass(Box::new(crate::Canonicalizer::new(config, true)));
            passes.run(function.as_operation_ref())?;
        } else {
            let region = function.borrow().body().as_region_ref();
            let mut rewriter: RewriterImpl = RewriterImpl::new(context.clone());
            let _ = Region::simplify_all(&[region], &mut rewriter, level);
        }
        assert_eq!(
            function.borrow().body().entry_block_ref(),
            Some(entry),
            "{}",
            function.as_operation_ref().borrow()
        );
        assert_eq!(entry.borrow().arguments(), args.as_slice());
        assert_eq!(
            entry
                .borrow()
                .arguments()
                .iter()
                .map(|arg| {
                    let arg = arg.borrow();
                    (arg.ty().clone(), arg.span())
                })
                .collect::<Vec<_>>(),
            initial
        );
        // Inspect the final incoming edges, including branches in any inserted split blocks.
        // Types alone cannot distinguish evolving results from the initial entry arguments.
        let mut backedges = 0;
        for block in function.borrow().body().body().iter() {
            let Some(op) = block.terminator() else {
                continue;
            };
            let op = op.borrow();
            for index in 0..op.num_successors() {
                let target = op.successor(index);
                if target.successor() == entry {
                    backedges += 1;
                    assert_eq!(
                        target
                            .arguments
                            .iter()
                            .map(|operand| operand.borrow().as_value_ref())
                            .collect::<Vec<_>>(),
                        expected_backedge
                    );
                }
            }
        }
        assert!(
            backedges > 0,
            "the evolving backedge must remain at the canonicalization boundary"
        );
        function.as_operation_ref().borrow().recursively_verify()?;
        let mut passes = PassManager::on::<Function>(context, Nesting::Implicit);
        passes.add_pass(Box::new(midenc_dialect_scf::transforms::LiftControlFlowToSCF));
        passes.run(function.as_operation_ref())?;
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }

    #[test]
    fn canonical_entry_aggressive_visits_nested_regions() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let module = parse::parse::<midenc_hir::dialects::builtin::Module>(
            ParserConfig::new(context.clone()),
            Uri::new("nested.hir"),
            r#"builtin.module public @outer {
            builtin.function public extern("C") @nested(%c: i1, %x: u32, %y: u32) -> u32 {
                cf.cond_br %c ^join(%x : u32), ^join(%x : u32) : (i1);
            ^join(%ret: u32): builtin.ret %ret : (u32);
            };
        };"#,
        )?;
        let function = module.borrow().functions().next().unwrap();
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let args = entry.borrow().arguments().to_vec();
        let join = function.borrow().body().body().back().as_pointer().unwrap();
        assert_eq!(join.borrow().arguments().len(), 1);
        let region = module.borrow().body().as_region_ref();
        let mut rewriter: RewriterImpl = RewriterImpl::new(context);
        let _ =
            Region::simplify_all(&[region], &mut rewriter, RegionSimplificationLevel::Aggressive);
        assert_eq!(entry.borrow().arguments(), args.as_slice());
        assert!(join.borrow().arguments().is_empty());
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }
}
