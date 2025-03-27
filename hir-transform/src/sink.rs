use alloc::vec::Vec;

use midenc_hir::{
    adt::SmallDenseMap,
    dominance::DominanceInfo,
    matchers::{self, Matcher},
    pass::{Pass, PassExecutionState},
    traits::{ConstantLike, Terminator},
    Backward, Builder, EntityMut, Forward, FxHashSet, OpBuilder, Operation, OperationName,
    OperationRef, ProgramPoint, RawWalk, Region, RegionBranchOpInterface,
    RegionBranchTerminatorOpInterface, RegionRef, Report, SmallVec, Usable, ValueRef,
};

/// This transformation sinks operations as close as possible to their uses, one of two ways:
///
/// 1. If there exists only a single use of the operation, move it before it's use so that it is
///    in an ideal position for code generation.
///
/// 2. If there exist multiple uses, materialize a duplicate operation for all but one of the uses,
///    placing them before the use. The last use will receive the original operation.
///
/// To make this rewrite even more useful, we take care to place the operation at a position before
/// the using op, such that when generating code, the operation value will be placed on the stack
/// at the appropriate place relative to the other operands of the using op. This makes the operand
/// stack scheduling optimizer's job easier.
///
/// The purpose of this rewrite is to improve the quality of generated code by reducing the live
/// ranges of values that are trivial to materialize on-demand.
///
/// # Restrictions
///
/// This transform will not sink operations under the following conditions:
///
/// * The operation has side effects
/// * The operation is a block terminator
/// * The operation has regions
///
/// # Implementation
///
/// Given a list of regions, perform control flow sinking on them. For each region, control-flow
/// sinking moves operations that dominate the region but whose only users are in the region into
/// the regions so that they aren't executed on paths where their results are not needed.
///
/// TODO: For the moment, this is a *simple* control-flow sink, i.e., no duplicating of ops. It
/// should be made to accept a cost model to determine whether duplicating a particular op is
/// profitable.
///
/// Example:
///
/// ```mlir
/// %0 = arith.addi %arg0, %arg1
/// scf.if %cond {
///   scf.yield %0
/// } else {
///   scf.yield %arg2
/// }
/// ```
///
/// After control-flow sink:
///
/// ```mlir
/// scf.if %cond {
///   %0 = arith.addi %arg0, %arg1
///   scf.yield %0
/// } else {
///   scf.yield %arg2
/// }
/// ```
///
/// If using the `control_flow_sink` function, callers can supply a callback
/// `should_move_into_region` that determines whether the given operation that only has users in the
/// given operation should be moved into that region. If this returns true, `move_into_region` is
/// called on the same operation and region.
///
/// `move_into_region` must move the operation into the region such that dominance of the operation
/// is preserved; for example, by moving the operation to the start of the entry block. This ensures
/// the preservation of SSA dominance of the operation's results.
pub struct ControlFlowSink;

impl Pass for ControlFlowSink {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "control-flow-sink"
    }

    fn argument(&self) -> &'static str {
        "control-flow-sink"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let op = op.into_entity_ref();
        log::debug!(target: "control-flow-sink", "sinking operations in {op}");

        let operation = op.as_operation_ref();
        drop(op);

        let dominfo = state.analysis_manager().get_analysis::<DominanceInfo>()?;

        operation.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
            let regions_to_sink = {
                let op = op.borrow();
                let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() else {
                    return;
                };
                let mut regions = SmallVec::<[_; 4]>::default();
                // Get the regions are that known to be executed at most once.
                get_singly_executed_regions_to_sink(branch, &mut regions);
                regions
            };

            // Sink side-effect free operations.
            control_flow_sink(
                &regions_to_sink,
                &dominfo,
                |op: &Operation, _region: &Region| op.is_memory_effect_free(),
                |mut op: OperationRef, region: RegionRef| {
                    // Move the operation to the beginning of the region's entry block.
                    // This guarantees the preservation of SSA dominance of all of the
                    // operation's uses are in the region.
                    let entry_block = region.borrow().entry_block_ref().unwrap();
                    op.borrow_mut().move_to(ProgramPoint::at_start_of(entry_block));
                },
            );
        });

        Ok(())
    }
}

/// This transformation sinks constants as close as possible to their uses, one of two ways:
///
/// 1. If there exists only a single use of the constant, move it before it's use so that it is
///    in an ideal position for code generation.
///
/// 2. If there exist multiple uses, materialize a duplicate constant for all but one of the uses,
///    placing them before the use. The last use will receive the original constant.
///
/// To make this rewrite even more useful, we take care to place the constant at a position before
/// the using op, such that when generating code, the constant value will be placed on the stack
/// at the appropriate place relative to the other operands of the using op. This makes the operand
/// stack scheduling optimizer's job easier.
///
/// The purpose of this rewrite is to improve the quality of generated code by reducing the live
/// ranges of values that are trivial to materialize on-demand.
pub struct SinkOperandDefs;

impl Pass for SinkOperandDefs {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "sink-operand-defs"
    }

    fn argument(&self) -> &'static str {
        "sink-operand-defs"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        _state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let operation = op.as_operation_ref();
        drop(op);

        log::debug!(target: "sink-operand-defs", "sinking operand defs for regions of {}", operation.borrow());

        // For each operation, we enqueue it in this worklist, we then recurse on each of it's
        // dependency operations until all dependencies have been visited. We move up blocks from
        // the bottom, and skip any operations we've already visited. Once the queue is built, we
        // then process the worklist, moving everything into position.
        let mut worklist = alloc::collections::VecDeque::default();

        // Visit ops in "true" post-order (i.e. block bodies are visited bottom-up).
        operation.raw_postwalk_all::<Backward, _>(|operation: OperationRef| {
            // Determine if any of this operation's operands represent one of the following:
            //
            // 1. A constant value
            // 2. The sole use of the defining op's single result, and that op has no side-effects
            //
            // If 1, then we either materialize a fresh copy of the constant, or move the original
            // if there are no more uses.
            //
            // In both cases, to the extent possible, we order operand dependencies such that the
            // values will be on the Miden operand stack in the correct order. This means that we
            // visit operands in reverse order, and move defining ops directly before `op` when
            // possible. Some values may be block arguments, or refer to op's we're unable to move,
            // and thus those values be out of position on the operand stack, but the overall
            // result will reduce the amount of unnecessary stack movement.
            let op = operation.borrow();

            log::trace!(target: "sink-operand-defs", "visiting {op}");

            for operand in op.operands().iter().rev() {
                let value = operand.borrow();
                let value = value.value();
                let is_sole_user = value.iter_uses().all(|user| user.owner == operation);

                let Some(defining_op) = value.get_defining_op() else {
                    // Skip block arguments, nothing to move in that situation
                    //
                    // NOTE: In theory, we could move effect-free operations _up_ the block to place
                    // them closer to the block arguments they use, but that's unlikely to be all
                    // that profitable of a rewrite in practice.
                    log::trace!(target: "sink-operand-defs", "  ignoring block argument operand '{value}'");
                    continue;
                };

                log::trace!(target: "sink-operand-defs", "  evaluating operand '{value}'");

                let def = defining_op.borrow();
                if def.implements::<dyn ConstantLike>() {
                    log::trace!(target: "sink-operand-defs", "    defining '{}' is constant-like", def.name());
                    worklist.push_back(OpOperandSink::new(operation));
                    break;
                }

                let incorrect_result_count = def.num_results() != 1;
                let has_effects = !def.is_memory_effect_free();
                if !is_sole_user || incorrect_result_count || has_effects {
                    // Skip this operand if the defining op cannot be safely moved
                    //
                    // NOTE: For now we do not move ops that produce more than a single result, but
                    // if the other results are unused, or the users would still be dominated by
                    // the new location, then we could still move those ops.
                    log::trace!(target: "sink-operand-defs", "    defining '{}' cannot be moved:", def.name());
                    log::trace!(target: "sink-operand-defs", "      * op has multiple uses");
                    if incorrect_result_count {
                        log::trace!(target: "sink-operand-defs", "      * op has incorrect number of results ({})", def.num_results());
                    }
                    if has_effects {
                        log::trace!(target: "sink-operand-defs", "      * op has memory effects");
                    }
                } else {
                    log::trace!(target: "sink-operand-defs", "    defining '{}' is moveable, but is non-constant", def.name());
                    worklist.push_back(OpOperandSink::new(operation));
                    break;
                }
            }
        });

        for sinker in worklist.iter() {
            log::debug!(target: "sink-operand-defs", "sink scheduled for {}", sinker.operation.borrow());
        }

        let mut visited = FxHashSet::default();
        let mut erased = FxHashSet::default();
        'next_operation: while let Some(mut sink_state) = worklist.pop_front() {
            let mut operation = sink_state.operation;
            let op = operation.borrow();

            // If this operation is unused, remove it now if it has no side effects
            let is_memory_effect_free =
                op.is_memory_effect_free() || op.implements::<dyn ConstantLike>();
            if !op.is_used()
                && is_memory_effect_free
                && !op.implements::<dyn Terminator>()
                && !op.implements::<dyn RegionBranchTerminatorOpInterface>()
                && erased.insert(operation)
            {
                log::debug!(target: "sink-operand-defs", "erasing unused, effect-free, non-terminator op {op}");
                drop(op);
                operation.borrow_mut().erase();
                continue;
            }

            // If we've already worked this operation, skip it
            if !visited.insert(operation) && sink_state.next_operand_index == op.num_operands() {
                log::trace!(target: "sink-operand-defs", "already visited {}", operation.borrow());
                continue;
            } else {
                log::trace!(target: "sink-operand-defs", "visiting {}", operation.borrow());
            }

            let mut builder = OpBuilder::new(op.context_rc());
            builder.set_insertion_point(sink_state.ip);
            'next_operand: loop {
                // The next operand index starts at `op.num_operands()` when first initialized, so
                // we subtract 1 immediately to get the actual index of the current operand
                let Some(next_operand_index) = sink_state.next_operand_index.checked_sub(1) else {
                    // We're done processing this operation's operands
                    break;
                };

                log::debug!(target: "sink-operand-defs", "  sinking next operand def for {op} at index {next_operand_index}");

                let mut operand = op.operands()[next_operand_index];
                sink_state.next_operand_index = next_operand_index;
                let operand_value = operand.borrow().as_value_ref();
                log::trace!(target: "sink-operand-defs", "  visiting operand {operand_value}");

                // Reuse moved/materialized replacements when the same operand is used multiple times
                if let Some(replacement) = sink_state.replacements.get(&operand_value).copied() {
                    if replacement != operand_value {
                        log::trace!(target: "sink-operand-defs", "    rewriting operand {operand_value} as {replacement}");
                        operand.borrow_mut().set(replacement);

                        // If no other uses of this value remain, then remove the original
                        // operation, as it is now dead.
                        if !operand_value.borrow().is_used() {
                            log::trace!(target: "sink-operand-defs", "    {operand_value} is no longer used, erasing definition");
                            // Replacements are only ever for op results
                            let mut defining_op = operand_value.borrow().get_defining_op().unwrap();
                            defining_op.borrow_mut().erase();
                        }
                    }
                    continue 'next_operand;
                }

                let value = operand_value.borrow();
                let is_sole_user = value.iter_uses().all(|user| user.owner == operation);

                let Some(mut defining_op) = value.get_defining_op() else {
                    // Skip block arguments, nothing to move in that situation
                    //
                    // NOTE: In theory, we could move effect-free operations _up_ the block to place
                    // them closer to the block arguments they use, but that's unlikely to be all
                    // that profitable of a rewrite in practice.
                    log::trace!(target: "sink-operand-defs", "    {value} is a block argument, ignoring..");
                    continue 'next_operand;
                };

                log::trace!(target: "sink-operand-defs", "    is sole user of {value}? {is_sole_user}");

                let def = defining_op.borrow();
                if let Some(attr) = matchers::constant().matches(&*def) {
                    if !is_sole_user {
                        log::trace!(target: "sink-operand-defs", "    defining op is a constant with multiple uses, materializing fresh copy");
                        // Materialize a fresh copy of the original constant
                        let span = value.span();
                        let ty = value.ty();
                        let Some(new_def) =
                            def.dialect().materialize_constant(&mut builder, attr, ty, span)
                        else {
                            log::trace!(target: "sink-operand-defs", "    unable to materialize copy, skipping rewrite of this operand");
                            continue 'next_operand;
                        };
                        drop(def);
                        drop(value);
                        let replacement = new_def.borrow().results()[0] as ValueRef;
                        log::trace!(target: "sink-operand-defs", "    rewriting operand {operand_value} as {replacement}");
                        sink_state.replacements.insert(operand_value, replacement);
                        operand.borrow_mut().set(replacement);
                    } else {
                        log::trace!(target: "sink-operand-defs", "    defining op is a constant with no other uses, moving into place");
                        // The original op can be moved
                        drop(def);
                        drop(value);
                        defining_op.borrow_mut().move_to(*builder.insertion_point());
                        sink_state.replacements.insert(operand_value, operand_value);
                    }
                } else if !is_sole_user || def.num_results() != 1 || !def.is_memory_effect_free() {
                    // Skip this operand if the defining op cannot be safely moved
                    //
                    // NOTE: For now we do not move ops that produce more than a single result, but
                    // if the other results are unused, or the users would still be dominated by
                    // the new location, then we could still move those ops.
                    log::trace!(target: "sink-operand-defs", "    defining op is unsuitable for sinking, ignoring this operand");
                } else {
                    // The original op can be moved
                    //
                    // Determine if we _should_ move it:
                    //
                    // 1. If the use is inside a loop, and the def is outside a loop, do not
                    //    move the defining op into the loop unless it is profitable to do so,
                    //    i.e. a cost model indicates it is more efficient than the equivalent
                    //    operand stack movement instructions
                    //
                    // 2.
                    drop(def);
                    drop(value);
                    log::trace!(target: "sink-operand-defs", "    defining op can be moved and has no other uses, moving into place");
                    defining_op.borrow_mut().move_to(*builder.insertion_point());
                    sink_state.replacements.insert(operand_value, operand_value);

                    // Enqueue the defining op to be visited before continuing with this op's operands
                    log::trace!(target: "sink-operand-defs", "    enqueing defining op for immediate processing");
                    //sink_state.ip = *builder.insertion_point();
                    sink_state.ip = ProgramPoint::before(operation);
                    worklist.push_front(sink_state);
                    worklist.push_front(OpOperandSink::new(defining_op));
                    continue 'next_operation;
                }
            }
        }

        Ok(())
    }
}

struct OpOperandSink {
    operation: OperationRef,
    ip: ProgramPoint,
    replacements: SmallDenseMap<ValueRef, ValueRef, 4>,
    next_operand_index: usize,
}

impl OpOperandSink {
    pub fn new(operation: OperationRef) -> Self {
        Self {
            operation,
            ip: ProgramPoint::before(operation),
            replacements: SmallDenseMap::new(),
            next_operand_index: operation.borrow().num_operands(),
        }
    }
}

/// A helper struct for control-flow sinking.
struct Sinker<'a, P, F> {
    /// Dominance info to determine op user dominance with respect to regions.
    dominfo: &'a DominanceInfo,
    /// The callback to determine whether an op should be moved in to a region.
    should_move_into_region: P,
    /// The calback to move an operation into the region.
    move_into_region: F,
    /// The number of operations sunk
    num_sunk: usize,
}
impl<'a, P, F> Sinker<'a, P, F>
where
    P: Fn(&Operation, &Region) -> bool,
    F: Fn(OperationRef, RegionRef),
{
    /// Create an operation sinker with given dominance info.
    pub fn new(
        dominfo: &'a DominanceInfo,
        should_move_into_region: P,
        move_into_region: F,
    ) -> Self {
        Self {
            dominfo,
            should_move_into_region,
            move_into_region,
            num_sunk: 0,
        }
    }

    /// Given a list of regions, find operations to sink and sink them.
    ///
    /// Returns the number of operations sunk.
    pub fn sink_regions(mut self, regions: &[RegionRef]) -> usize {
        for region in regions.iter().copied() {
            if !region.borrow().is_empty() {
                self.sink_region(region);
            }
        }

        self.num_sunk
    }

    /// Given a region and an op which dominates the region, returns true if all
    /// users of the given op are dominated by the entry block of the region, and
    /// thus the operation can be sunk into the region.
    fn all_users_dominated_by(&self, op: &Operation, region: &Region) -> bool {
        assert!(
            region.find_ancestor_op(op.as_operation_ref()).is_none(),
            "expected op to be defined outside the region"
        );
        let region_entry = region.entry_block_ref().unwrap();
        op.results().iter().all(|result| {
            let result = result.borrow();
            result.iter_uses().all(|user| {
                // The user is dominated by the region if its containing block is dominated
                // by the region's entry block.
                self.dominfo.dominates(&region_entry, &user.owner.parent().unwrap())
            })
        })
    }

    /// Given a region and a top-level op (an op whose parent region is the given
    /// region), determine whether the defining ops of the op's operands can be
    /// sunk into the region.
    ///
    /// Add moved ops to the work queue.
    fn try_to_sink_predecessors(
        &mut self,
        user: OperationRef,
        region: RegionRef,
        stack: &mut Vec<OperationRef>,
    ) {
        log::trace!(target: "control-flow-sink", "contained op: {}", user.borrow());
        let user = user.borrow();
        for operand in user.operands().iter() {
            let op = operand.borrow().value().get_defining_op();
            // Ignore block arguments and ops that are already inside the region.
            if op.is_none_or(|op| op.grandparent().is_some_and(|r| r == region)) {
                continue;
            }

            let op = unsafe { op.unwrap_unchecked() };

            log::trace!(target: "control-flow-sink", "try to sink op: {}", op.borrow());

            // If the op's users are all in the region and it can be moved, then do so.
            let (all_users_dominated_by, should_move_into_region) = {
                let op = op.borrow();
                let region = region.borrow();
                let all_users_dominated_by = self.all_users_dominated_by(&op, &region);
                let should_move_into_region = (self.should_move_into_region)(&op, &region);
                (all_users_dominated_by, should_move_into_region)
            };
            if all_users_dominated_by && should_move_into_region {
                (self.move_into_region)(op, region);

                self.num_sunk += 1;

                // Add the op to the work queue
                stack.push(op);
            }
        }
    }

    /// Iterate over all the ops in a region and try to sink their predecessors.
    /// Recurse on subgraphs using a work queue.
    fn sink_region(&mut self, region: RegionRef) {
        // Initialize the work queue with all the ops in the region.
        let mut stack = Vec::new();
        for block in region.borrow().body() {
            for op in block.body() {
                stack.push(op.as_operation_ref());
            }
        }

        // Process all the ops depth-first. This ensures that nodes of subgraphs are sunk in the
        // correct order.
        while let Some(op) = stack.pop() {
            self.try_to_sink_predecessors(op, region, &mut stack);
        }
    }
}

pub fn control_flow_sink<P, F>(
    regions: &[RegionRef],
    dominfo: &DominanceInfo,
    should_move_into_region: P,
    move_into_region: F,
) where
    P: Fn(&Operation, &Region) -> bool,
    F: Fn(OperationRef, RegionRef),
{
    let sinker = Sinker::new(dominfo, should_move_into_region, move_into_region);
    sinker.sink_regions(regions);
}

/// Populates `regions` with regions of the provided region branch op that are executed at most once
/// at that are reachable given the current operands of the op. These regions can be passed to
/// `control_flow_sink` to perform sinking on the regions of the operation.
fn get_singly_executed_regions_to_sink(
    branch: &dyn RegionBranchOpInterface,
    regions: &mut SmallVec<[RegionRef; 4]>,
) {
    use midenc_hir::matchers::Matcher;

    // Collect constant operands.
    let mut operands = SmallVec::<[_; 4]>::with_capacity(branch.num_operands());

    for operand in branch.operands().iter() {
        let matcher = matchers::foldable_operand();
        operands.push(matcher.matches(operand));
    }

    // Get the invocation bounds.
    let bounds = branch.get_region_invocation_bounds(&operands);

    // For a simple control-flow sink, only consider regions that are executed at most once.
    for (region, bound) in branch.regions().iter().zip(bounds) {
        use core::range::Bound;
        match bound.max() {
            Bound::Unbounded => continue,
            Bound::Excluded(bound) if *bound > 2 => continue,
            Bound::Excluded(0) => continue,
            Bound::Included(bound) if *bound > 1 => continue,
            _ => {
                regions.push(region.as_region_ref());
            }
        }
    }
}
