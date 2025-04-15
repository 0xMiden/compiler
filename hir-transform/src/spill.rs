use alloc::{collections::VecDeque, rc::Rc};

use midenc_hir::{
    adt::{SmallDenseMap, SmallSet},
    cfg::Graph,
    dominance::{DomTreeNode, DominanceFrontier, DominanceInfo},
    pass::{AnalysisManager, PostPassStatus},
    traits::SingleRegion,
    BlockRef, Builder, Context, FxHashMap, OpBuilder, OpOperand, Operation, OperationRef,
    ProgramPoint, Region, RegionBranchOpInterface, RegionBranchPoint, RegionRef, Report, Rewriter,
    SmallVec, SourceSpan, Spanned, StorableEntity, Usable, ValueRange, ValueRef,
};
use midenc_hir_analysis::analyses::{
    spills::{Placement, Predecessor},
    SpillAnalysis,
};

/// This interface is used in conjunction with [transform_spills] so that the transform can be used
/// with any dialect, and more importantly, avoids forming a dependency on our own dialects for the
/// subset of operations we need to emit/rewrite.
pub trait TransformSpillsInterface {
    /// Create an unconditional branch to `destination`
    fn create_unconditional_branch(
        &self,
        builder: &mut OpBuilder,
        destination: BlockRef,
        arguments: &[ValueRef],
        span: SourceSpan,
    ) -> Result<(), Report>;

    /// Create a spill for `value`, returning the spill instruction
    fn create_spill(
        &self,
        builder: &mut OpBuilder,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<OperationRef, Report>;

    /// Create a reload of `value`, returning the reload instruction
    fn create_reload(
        &self,
        builder: &mut OpBuilder,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<OperationRef, Report>;

    /// Convert `spill`, a [SpillLike] operation, into a primitive memory store of the spilled
    /// value.
    fn convert_spill_to_store(
        &mut self,
        rewriter: &mut dyn Rewriter,
        spill: OperationRef,
    ) -> Result<(), Report>;

    /// Convert `reload`, a [ReloadLike] operation, into a primitive memory load of the spilled
    /// value.
    fn convert_reload_to_load(
        &mut self,
        rewriter: &mut dyn Rewriter,
        reload: OperationRef,
    ) -> Result<(), Report>;
}

/// An operation trait for operations that implement spill-like behavior for purposes of the
/// spills transformation/rewrite.
///
/// A spill-like operation is expected to take a single value, and store it somewhere in memory
/// temporarily, such that the live range of the original value is terminated by the spill. Spilled
/// values may then be reloaded, starting a new live range, using the corresponding [ReloadLike] op.
pub trait SpillLike {
    /// Returns the operand corresponding to the spilled value
    fn spilled(&self) -> OpOperand;
    /// Returns a reference to the spilled value
    fn spilled_value(&self) -> ValueRef {
        self.spilled().borrow().as_value_ref()
    }
}

/// An operation trait for operations that implement reload-like behavior for purposes of the
/// spills transformation/rewrite.
///
/// A reload-like operation is expected to take a single value, for which a dominating [SpillLike]
/// op exists, and produce a new, unique SSA value corresponding to the reloaded spill value. The
/// spills transformation will handle rewriting any uses of the [SpillLike] and [ReloadLike] ops
/// such that they are not present after the transformation, in conjunction with an implementation
/// of the [TransformSpillsInterface].
pub trait ReloadLike {
    /// Returns the operand corresponding to the spilled value
    fn spilled(&self) -> OpOperand;
    /// Returns a reference to the spilled value
    fn spilled_value(&self) -> ValueRef {
        self.spilled().borrow().as_value_ref()
    }
    /// Returns the value representing this unique reload of the spilled value
    ///
    /// Generally, this always corresponds to this op's result
    fn reloaded(&self) -> ValueRef;
}

/// This transformation rewrites `op` by applying the results of the provided [SpillAnalysis],
/// using the provided implementation of the [TransformSpillsInterface].
///
/// In effect, it performs the following steps:
///
/// * Splits all control flow edges that need to carry spills/reloads
/// * Inserts all spills and reloads at their computed locations
/// * Rewrites `op` such that all uses of a spilled value dominated by a reload, are rewritten to
///   use that reload, or in the case of crossing a dominance frontier, a materialized block
///   argument/phi representing the closest definition of that value from each predecessor.
/// * Rewrites all spill and reload instructions to their primitive memory store/load ops
pub fn transform_spills(
    op: OperationRef,
    analysis: &mut SpillAnalysis,
    interface: &mut dyn TransformSpillsInterface,
    analysis_manager: AnalysisManager,
) -> Result<PostPassStatus, Report> {
    assert!(
        op.borrow().implements::<dyn SingleRegion>(),
        "the spills transformation is not supported when the root op is multi-region"
    );

    let mut builder = OpBuilder::new(op.borrow().context_rc());

    log::debug!(target: "insert-spills", "analysis determined that some spills were required");
    log::debug!(target: "insert-spills", "    edges to split = {}", analysis.splits().len());
    log::debug!(target: "insert-spills", "    values spilled = {}", analysis.spills().len());
    log::debug!(target: "insert-spills", "    reloads issued = {}", analysis.reloads().len());

    // Split all edges along which spills/reloads are required
    for split_info in analysis.splits_mut() {
        log::trace!(target: "insert-spills", "splitting control flow edge {} -> {}", match split_info.predecessor {
            Predecessor::Parent => ProgramPoint::before(split_info.predecessor.operation(split_info.point)),
            Predecessor::Block { op, .. } | Predecessor::Region(op) => ProgramPoint::at_end_of(op.parent().unwrap()),
        }, split_info.point);

        let predecessor_block = split_info
            .predecessor
            .block()
            .unwrap_or_else(|| todo!("implement support for splits following a region branch op"));
        let predecessor_region = predecessor_block.parent().unwrap();

        // Create the split and switch the insertion point to the end of it
        let split = builder.create_block(predecessor_region, Some(predecessor_block), &[]);
        log::trace!(target: "insert-spills", "created {split} to hold contents of split edge");

        // Record the block we created for this split
        split_info.split = Some(split);

        // Rewrite the terminator in the predecessor so that it transfers control to the
        // original successor via `split`, moving any block arguments into the unconditional
        // branch that terminates `split`.
        match split_info.predecessor {
            Predecessor::Block { mut op, index } => {
                log::trace!(target: "insert-spills", "redirecting {predecessor_block} to {split}");
                let mut op = op.borrow_mut();
                let mut succ = op.successor_mut(index as usize);
                let prev_dest = succ.dest.parent().unwrap();
                succ.dest.borrow_mut().set(split);
                log::trace!(target: "insert-spills", "creating edge from {split} to {prev_dest}");
                let arguments = succ
                    .arguments
                    .take()
                    .into_iter()
                    .map(|mut operand| {
                        let mut operand = operand.borrow_mut();
                        let value = operand.as_value_ref();
                        // It is our responsibility to unlink the operands we removed from `succ`
                        operand.unlink();
                        value
                    })
                    .collect::<SmallVec<[_; 4]>>();
                match split_info.point {
                    ProgramPoint::Block { block, .. } => {
                        assert_eq!(
                            prev_dest, block,
                            "unexpected mismatch between predecessor target and successor block"
                        );
                        interface.create_unconditional_branch(
                            &mut builder,
                            block,
                            &arguments,
                            op.span(),
                        )?;
                    }
                    point => panic!(
                        "unexpected program point for split: unstructured control flow requires a \
                         block entry, got {point}"
                    ),
                }
            }
            Predecessor::Region(predecessor) => {
                log::trace!(target: "insert-spills", "splitting region control flow edge to {} from {predecessor}", split_info.point);
                todo!()
            }
            Predecessor::Parent => unimplemented!(
                "support for splits on exit from region branch ops is not yet implemented"
            ),
        }
    }

    // Insert all spills
    for spill in analysis.spills.iter_mut() {
        let ip = match spill.place {
            Placement::Split(split) => {
                let split_block = analysis.splits[split.as_usize()]
                    .split
                    .expect("expected split to have been materialized");
                let terminator = split_block.borrow().terminator().unwrap();
                ProgramPoint::before(terminator)
            }
            Placement::At(ip) => ip,
        };
        log::trace!(target: "insert-spills", "inserting spill of {} at {ip}", spill.value);
        builder.set_insertion_point(ip);
        let inst = interface.create_spill(&mut builder, spill.value, spill.span)?;
        spill.inst = Some(inst);
    }

    // Insert all reloads
    for reload in analysis.reloads.iter_mut() {
        let ip = match reload.place {
            Placement::Split(split) => {
                let split_block = analysis.splits[split.as_usize()]
                    .split
                    .expect("expected split to have been materialized");
                let terminator = split_block.borrow().terminator().unwrap();
                ProgramPoint::before(terminator)
            }
            Placement::At(ip) => ip,
        };
        log::trace!(target: "insert-spills", "inserting reload of {} at {ip}", reload.value);
        builder.set_insertion_point(ip);
        let inst = interface.create_reload(&mut builder, reload.value, reload.span)?;
        reload.inst = Some(inst);
    }

    log::trace!(target: "insert-spills", "all spills and reloads inserted successfully");

    let dominfo = analysis_manager.get_analysis::<DominanceInfo>()?;

    let region = op.borrow().regions().front().as_pointer().unwrap();
    if region.borrow().has_one_block() {
        rewrite_single_block_spills(op, region, analysis, interface, analysis_manager)?;
    } else {
        rewrite_cfg_spills(
            builder.context_rc(),
            region,
            analysis,
            interface,
            &dominfo,
            analysis_manager,
        )?;
    }

    Ok(PostPassStatus::IRChanged)
}

fn rewrite_single_block_spills(
    op: OperationRef,
    region: RegionRef,
    analysis: &mut SpillAnalysis,
    interface: &mut dyn TransformSpillsInterface,
    _analysis_manager: AnalysisManager,
) -> Result<(), Report> {
    // In a flattened CFG with only structured control flow, no dominance tree is required.
    //
    // Instead, similar to a regular CFG, we walk the region graph in post-order, doing the
    // following:
    //
    // 1. If we encounter a use of a spilled value, we add it to a use list
    // 2. If we encounter a reloaded spill, we rewrite any uses found so far to use the reloaded
    //    value
    // 3. If we encounter a spill, then we clear the set of uses of that spill found so far and
    //    continue
    // 4. If we reach the top of a region's entry block, and the region has no predecessors other
    //    than the containing operation, then we do nothing but continue the traversal.
    // 5. If we reach the top of a region's entry block, and the region has multiple predecessors,
    //    then for each spilled value for which we have found at least one use, we must insert a
    //    new region argument representing the spilled value, and rewrite all uses to use that
    //    argument instead. For any dominating predecessors, the original spilled value is passed
    //    as the value of the new argument.

    struct Node {
        block: BlockRef,
        cursor: Option<OperationRef>,
        is_first_visit: bool,
    }
    impl Node {
        pub fn new(block: BlockRef) -> Self {
            Self {
                block,
                cursor: block.borrow().body().back().as_pointer(),
                is_first_visit: true,
            }
        }

        pub fn current(&self) -> Option<OperationRef> {
            self.cursor
        }

        pub fn move_next(&mut self) -> Option<OperationRef> {
            let next = self.cursor.take()?;
            self.cursor = next.prev();
            Some(next)
        }
    }

    let mut block_states =
        FxHashMap::<BlockRef, SmallDenseMap<ValueRef, SmallSet<OpOperand, 4>, 4>>::default();
    let entry_block = region.borrow().entry_block_ref().unwrap();
    let mut block_q = VecDeque::from([Node::new(entry_block)]);

    while let Some(mut node) = block_q.pop_back() {
        let Some(operation) = node.current() else {
            // We've reached the top of the block, remove any uses of the block arguments, if they
            // were spilled, as they represent the original definitions of those values.
            let block = node.block.borrow();
            let used = block_states.entry(node.block).or_default();
            for arg in ValueRange::<2>::from(block.arguments()) {
                if analysis.is_spilled(&arg) {
                    used.remove(&arg);
                }
            }
            continue;
        };

        let op = operation.borrow();
        if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
            // Before we process this op, we need to visit all if it's regions first, as rewriting
            // those regions might introduce new region arguments that we must rewrite here. So,
            // if this is our first visit to this op, we recursively visit its regions in postorder
            // first, and then mark the op has visited. The next time we visit this op, we will
            // skip this part, and proceed to handling uses/defs of spilled values at the op entry/
            // exit.
            if node.is_first_visit {
                node.is_first_visit = false;
                block_q.push_back(node);
                for region in Region::postorder_region_graph_for(branch).into_iter().rev() {
                    let region = region.borrow();
                    assert!(
                        region.has_one_block(),
                        "multi-block regions are not currently supported"
                    );
                    let entry = region.entry();
                    block_q.push_back(Node::new(entry.as_block_ref()));
                }
                continue;
            } else {
                // Process any uses in the entry regions of this op before proceeding
                for region in branch.get_successor_regions(RegionBranchPoint::Parent) {
                    let Some(region) = region.into_successor() else {
                        continue;
                    };

                    let region_entry = region.borrow().entry_block_ref().unwrap();
                    if let Some(uses) = block_states.remove(&region_entry) {
                        let parent_uses = block_states.entry(node.block).or_default();
                        for (spilled, users) in uses {
                            // TODO(pauls): If `users` is non-empty, and `region` has multiple
                            // predecessors, then we need to introduce a new region argument to
                            // represent the definition of each spilled value from those
                            // predecessors, and then rewrite the uses to use the new argument.
                            let parent_users = parent_uses.entry(spilled).or_default();
                            let merged = users.into_union(parent_users);
                            *parent_users = merged;
                        }
                    }
                }
            }
        }

        let used = block_states.entry(node.block).or_default();

        let reload_like = op.as_trait::<dyn ReloadLike>();
        let is_reload_like = reload_like.is_some();
        if let Some(reload_like) = reload_like {
            // We've found a reload of a spilled value, rewrite all uses of the spilled value
            // found so far to use the reload instead.
            let spilled = reload_like.spilled_value();
            let reloaded = reload_like.reloaded();

            if let Some(to_rewrite) = used.remove(&spilled) {
                debug_assert!(!to_rewrite.is_empty(), "expected empty use sets to be removed");

                for mut user in to_rewrite {
                    user.borrow_mut().set(reloaded);
                }
            } else {
                // This reload is unused, so remove it entirely, and move to the next op
                node.move_next();
                continue;
            }
        }

        // Advance the cursor in this block
        node.move_next();

        // Remove any use tracking for spilled values defined by this op
        for result in ValueRange::<2>::from(op.results().all()) {
            if analysis.is_spilled(&result) {
                used.remove(&result);
                continue;
            }
        }

        // Record any uses of spilled values by this op, so long as the op is not reload-like
        if !is_reload_like {
            for operand in op.operands().iter().copied() {
                let value = operand.borrow().as_value_ref();
                if analysis.is_spilled(&value) {
                    used.entry(value).or_default().insert(operand);
                }
            }
        }
    }

    rewrite_spill_pseudo_instructions(op.borrow().context_rc(), analysis, interface, None)
}

fn rewrite_cfg_spills(
    context: Rc<Context>,
    region: RegionRef,
    analysis: &mut SpillAnalysis,
    interface: &mut dyn TransformSpillsInterface,
    dominfo: &DominanceInfo,
    _analysis_manager: AnalysisManager,
) -> Result<(), Report> {
    // At this point, we've potentially emitted spills/reloads, but these are not yet being
    // used to split the live ranges of the SSA values to which they apply. Our job now, is
    // to walk the CFG bottom-up, finding uses of values for which we have issued reloads,
    // and then looking for the dominating definition (either original, or reload) that controls
    // that use, rewriting the use with the SSA value corresponding to the reloaded value.
    //
    // This has the effect of "reconstructing" the SSA form - although in our case it is more
    // precise to say that we are fixing up the original program to reflect the live-range
    // splits that we have computed (and inserted pseudo-instructions for). In the original
    // paper, they actually had multiple definitions of reloaded SSA values, which is why
    // this phase is referred to as "reconstructing", as it is intended to recover the SSA
    // property that was lost once multiple definitions are introduced.
    //
    //   * For each original definition of a spilled value `v`, get the new definitions of `v`
    //     (reloads) and the uses of `v`.
    //   * For each use of `v`, walk the dominance tree upwards until a definition of `v` is
    //     found that is responsible for that use. If an iterated dominance frontier is passed,
    //     a block argument is inserted such that appropriate definitions from each predecessor
    //     are wired up to that block argument, which is then the definition of `v` responsible
    //     for subsequent uses. The predecessor instructions which branch to it are new uses
    //     which we visit in the same manner as described above. After visiting all uses, any
    //     definitions (reloads) which are dead will have no uses of the reloaded value, and can
    //     thus be eliminated.

    // We consume the spill analysis in this pass, as it will no longer be valid after this
    let domtree = dominfo.dominance(region);
    let domf = DominanceFrontier::new(&domtree);

    // Make sure that any block in the iterated dominance frontier of a spilled value, has
    // a new phi (block argument) inserted, if one is not already present. These must be in
    // the CFG before we search for dominating definitions.
    let inserted_phis = insert_required_phis(&context, analysis, &domf);

    // Traverse the CFG bottom-up, doing the following along the way:
    //
    // 0. Merge the "used" sets of each successor of the current block (see remaining steps for
    //    how the "used" set is computed for a block). NOTE: We elaborate in step 4 on how to
    //    handle computing the "used" set for a successor, from the "used" set at the start of
    //    the successor block.
    // 1. If we encounter a use of a spilled value, record the location of that use in the set
    // of uses we're seeking a dominating definition for, i.e. the "used" set
    // 2. If we reach a definition for a value with uses in the "used" set:
    //   * If the definition is the original definition of the value, no action is needed, so we
    //     remove all uses of that value from the "used" set.
    //   * If the definition is a reload, rewrite all of the uses in the "used" set to use the
    //     reload instead, removing them from the "used" set. Mark the reload used.
    // 3. When we reach the start of the block, the state of the "used" set is associated with
    //    the current block. This will be used as the starting state of the "used" set in each
    //    predecessor of the block
    // 4. When computing the "used" set in the predecessor (i.e. step 0), we also check whether
    //    a given successor is in the iterated dominance frontier for any values in the "used"
    //    set of that successor. If so, we need to insert a block parameter for each such value,
    //    rewrite all uses of that value to use the new block parameter, and add the "used"
    //    value as an additional argument to that successor. The resulting "used" set will thus
    //    retain a single entry for each of the values for which uses were rewritten
    //    (corresponding to the block arguments for the successor), but all of the uses
    //    dominated by the introduced block parameter are no longer in the set, as their
    //    dominating definition has been found. Any values in the "used" set for which the
    //    successor is not in the iterated dominance frontier for that value, are retained in
    //    the "used" set without any changes.
    let mut used_sets =
        SmallDenseMap::<BlockRef, SmallDenseMap<ValueRef, SmallSet<OpOperand, 8>, 8>, 8>::default();
    let mut block_q = VecDeque::from(domtree.postorder());
    while let Some(node) = block_q.pop_front() {
        let Some(block_ref) = node.block() else {
            continue;
        };

        // Compute the initial "used" set for this block
        let mut used = SmallDenseMap::<ValueRef, SmallSet<OpOperand, 8>, 8>::default();
        for succ in Rc::<DomTreeNode>::children(node) {
            let Some(succ_block) = succ.block() else {
                continue;
            };

            if let Some(usages) = used_sets.get_mut(&succ_block) {
                // Union the used set from this successor with the others
                for (value, users) in usages.iter() {
                    used.entry(*value).or_default().extend(users.iter().copied());
                }
            }
        }

        // Traverse the block bottom-up, recording uses of spilled values while looking for
        // definitions
        let block = block_ref.borrow();
        for op in block.body().iter().rev() {
            find_inst_uses(&op, &mut used, analysis);
        }

        // At the top of the block, if any of the block parameters are in the "used" set, remove
        // those uses, as the block parameters are the original definitions for those values, and
        // thus no rewrite is needed.
        for arg in ValueRange::<2>::from(block.arguments()) {
            used.remove(&arg);
        }

        rewrite_inserted_phi_uses(&inserted_phis, block_ref, &mut used);

        // What remains are the unsatisfied uses of spilled values for this block and its
        // successors
        used_sets.insert(block_ref, used);
    }

    rewrite_spill_pseudo_instructions(context, analysis, interface, Some(dominfo))
}

/// Insert additional phi nodes as follows:
///
/// 1. For each spilled value V
/// 2. Obtain the set of blocks, R, containing a reload of V
/// 3. For each block B in the iterated dominance frontier of R, insert a phi in B for V
/// 4. For every predecessor of B, append a new block argument to B, passing V initially
/// 5. Traverse the CFG bottom-up, finding uses of V, until we reach an inserted phi, a reload, or
///    the original definition. Rewrite all found uses of V up to that point, to use this
///    definition.
fn insert_required_phis(
    context: &Context,
    analysis: &SpillAnalysis,
    domf: &DominanceFrontier,
) -> SmallDenseMap<BlockRef, SmallDenseMap<ValueRef, ValueRef, 8>, 8> {
    use midenc_hir::adt::smallmap::Entry;

    let mut required_phis = SmallDenseMap::<ValueRef, SmallSet<BlockRef, 2>, 4>::default();
    for reload in analysis.reloads() {
        let block = reload.inst.unwrap().parent().unwrap();
        let r = required_phis.entry(reload.value).or_default();
        r.insert(block);
    }

    let mut inserted_phis =
        SmallDenseMap::<BlockRef, SmallDenseMap<ValueRef, ValueRef, 8>, 8>::default();
    for (value, domf_r) in required_phis {
        // Compute the iterated dominance frontier, DF+(R)
        let idf_r = domf.iterate_all(domf_r);
        // Add phi to each B in DF+(R)
        let (ty, span) = {
            let value = value.borrow();
            (value.ty().clone(), value.span())
        };
        for mut b in idf_r {
            // Allocate new block parameter/phi, if one is not already present
            let phis = inserted_phis.entry(b).or_default();
            if let Entry::Vacant(entry) = phis.entry(value) {
                let phi = context.append_block_argument(b, ty.clone(), span);
                entry.insert(phi);

                // Append `value` as new argument to every predecessor to satisfy new parameter
                let block = b.borrow_mut();
                let mut next_use = block.uses().front().as_pointer();
                while let Some(pred) = next_use.take() {
                    next_use = pred.next();

                    let (mut predecessor, successor_index) = {
                        let pred = pred.borrow();
                        (pred.owner, pred.index as usize)
                    };
                    let operand = context.make_operand(value, predecessor, 0);
                    predecessor.borrow_mut().successor_mut(successor_index).arguments.push(operand);
                }
            }
        }
    }

    inserted_phis
}

fn find_inst_uses(
    op: &Operation,
    used: &mut SmallDenseMap<ValueRef, SmallSet<OpOperand, 8>, 8>,
    analysis: &SpillAnalysis,
) {
    let reload_like = op.as_trait::<dyn ReloadLike>();
    let is_reload = reload_like.is_some();
    if let Some(reload_like) = reload_like {
        // We have found a new definition for a spilled value, we must rewrite all uses of the
        // spilled value found so far, with the reloaded value.
        let spilled = reload_like.spilled_value();
        let reloaded = reload_like.reloaded();

        if let Some(to_rewrite) = used.remove(&spilled) {
            debug_assert!(!to_rewrite.is_empty(), "expected empty use sets to be removed");

            for mut user in to_rewrite {
                user.borrow_mut().set(reloaded);
            }
        } else {
            // This reload is unused, so remove it entirely, and move to the next op
            return;
        }
    }

    for result in ValueRange::<2>::from(op.results().all()) {
        if analysis.is_spilled(&result) {
            // This op is the original definition for a spilled value, so remove any
            // uses of it we've accumulated so far, as they do not need to be rewritten
            used.remove(&result);
        }
    }

    // Record any uses of spilled values in the argument list for `op`, but ignore reload-likes
    if !is_reload {
        for operand in op.operands().iter().copied() {
            let value = operand.borrow().as_value_ref();
            if analysis.is_spilled(&value) {
                used.entry(value).or_default().insert(operand);
            }
        }
    }
}

fn rewrite_inserted_phi_uses(
    inserted_phis: &SmallDenseMap<BlockRef, SmallDenseMap<ValueRef, ValueRef, 8>, 8>,
    block_ref: BlockRef,
    used: &mut SmallDenseMap<ValueRef, SmallSet<OpOperand, 8>, 8>,
) {
    // If we have inserted any phis in this block, rewrite uses of the spilled values they
    // represent.
    if let Some(phis) = inserted_phis.get(&block_ref) {
        for (spilled, phi) in phis.iter() {
            if let Some(to_rewrite) = used.remove(spilled) {
                debug_assert!(!to_rewrite.is_empty(), "expected empty use sets to be removed");

                for mut user in to_rewrite {
                    user.borrow_mut().set(*phi);
                }
            } else {
                // TODO(pauls): This phi is unused, we should be able to remove it
                log::warn!(target: "insert-spills", "unused phi {phi} encountered during rewrite phase");
                continue;
            }
        }
    }
}

/// For each spilled value, allocate a procedure local, rewrite the spill instruction as a
/// `local.store`, unless the spill is dead, in which case we remove the spill entirely.
///
/// Dead spills can occur because the spills analysis must conservatively place them to
/// ensure that all paths to a block where a value has been spilled along at least one
/// of those paths, gets spilled on all of them, by inserting extra spills along those
/// edges where a spill hasn't occurred yet.
///
/// However, this produces dead spills on some paths through the function, which are not
/// needed once rewrites have been performed. So we eliminate dead spills by identifying
/// those spills which do not dominate any reloads - if a store to a spill slot can never
/// be read, then the store can be elided.
fn rewrite_spill_pseudo_instructions(
    context: Rc<Context>,
    analysis: &mut SpillAnalysis,
    interface: &mut dyn TransformSpillsInterface,
    dominfo: Option<&DominanceInfo>,
) -> Result<(), Report> {
    use midenc_hir::{
        dominance::Dominates,
        patterns::{NoopRewriterListener, RewriterImpl},
    };

    let mut builder = RewriterImpl::<NoopRewriterListener>::new(context);
    for spill in analysis.spills() {
        let operation = spill.inst.expect("expected spill to have been materialized");
        let op = operation.borrow();
        let spill_like = op
            .as_trait::<dyn SpillLike>()
            .expect("expected materialized spill operation to implement SpillLike");
        // The current SSA value representing the original spilled value at this point in the
        // program
        let stored = spill_like.spilled_value();
        // This spill is used if it properly dominates any reload of `stored` (and all uses should
        // be reloads)
        //
        // If we have no dominance info, the spill is presumed used
        let is_used = dominfo.is_none_or(|dominfo| {
            stored.borrow().uses().iter().any(|user| {
                let used_by = user.owner.borrow();
                debug_assert!(
                    user.owner == operation || used_by.implements::<dyn ReloadLike>(),
                    "unexpected non-reload use of spilled value"
                );
                op.dominates(&used_by, dominfo)
            })
        });
        drop(op);

        if is_used {
            builder.set_insertion_point_after(operation);
            interface.convert_spill_to_store(&mut builder, operation)?;
        } else {
            builder.erase_op(operation);
        }
    }

    // Rewrite all used reload instructions as `local.load` instructions from the corresponding
    // procedure local
    for reload in analysis.reloads() {
        let operation = reload.inst.expect("expected reload to have been materialized");
        let op = operation.borrow();
        let reload_like = op
            .as_trait::<dyn ReloadLike>()
            .expect("expected materialized reload op to implement ReloadLike");
        let is_used = reload_like.reloaded().borrow().is_used();
        drop(op);

        // Avoid emitting loads for unused reloads
        if is_used {
            builder.set_insertion_point_after(operation);
            interface.convert_reload_to_load(&mut builder, operation)?;
        } else {
            builder.erase_op(operation);
        }
    }

    Ok(())
}
