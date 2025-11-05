use alloc::collections::VecDeque;

use smallvec::SmallVec;

use super::RegionTransformFailed;
use crate::{
    adt::SmallSet,
    traits::{BranchOpInterface, Terminator},
    OpOperandImpl, OpResult, Operation, OperationRef, PostOrderBlockIter, Region, RegionRef,
    Rewriter, SuccessorOperands, ValueRef,
};

/// Data structure used to track which values have already been proved live.
///
/// Because operations can have multiple results, this data structure tracks liveness for both
/// values and operations to avoid having to look through all results when analyzing a use.
///
/// This data structure essentially tracks the dataflow lattice. The set of values/ops proved live
/// increases monotonically to a fixed-point.
#[derive(Default)]
struct LiveMap {
    values: SmallSet<ValueRef, 16>,
    ops: SmallSet<OperationRef, 16>,
    changed: bool,
}
impl LiveMap {
    pub fn was_proven_live(&self, value: &ValueRef) -> bool {
        // TODO(pauls): For results that are removable, e.g. for region based control flow,
        // we could allow for these values to be tracked independently.
        let val = value.borrow();
        if let Some(result) = val.downcast_ref::<OpResult>() {
            self.ops.contains(&result.owner())
        } else {
            self.values.contains(value)
        }
    }

    #[inline]
    pub fn was_op_proven_live(&self, op: &OperationRef) -> bool {
        self.ops.contains(op)
    }

    pub fn set_proved_live(&mut self, value: ValueRef) {
        // TODO(pauls): For results that are removable, e.g. for region based control flow,
        // we could allow for these values to be tracked independently.
        let val = value.borrow();
        if let Some(result) = val.downcast_ref::<OpResult>() {
            self.changed |= self.ops.insert(result.owner());
        } else {
            self.changed |= self.values.insert(value);
        }
    }

    pub fn set_op_proved_live(&mut self, op: OperationRef) {
        self.changed |= self.ops.insert(op);
    }

    #[inline(always)]
    pub fn mark_unchanged(&mut self) {
        self.changed = false;
    }

    #[inline(always)]
    pub const fn has_changed(&self) -> bool {
        self.changed
    }

    pub fn is_use_specially_known_dead(&self, user: &OpOperandImpl) -> bool {
        // DCE generally treats all uses of an op as live if the op itself is considered live.
        // However, for successor operands to terminators we need a finer-grained notion where we
        // deduce liveness for operands individually. The reason for this is easiest to think about
        // in terms of a classical phi node based SSA IR, where each successor operand is really an
        // operand to a _separate_ phi node, rather than all operands to the branch itself as with
        // the block argument representation that we use.
        //
        // And similarly, because each successor operand is really an operand to a phi node, rather
        // than to the terminator op itself, a terminator op can't e.g. "print" the value of a
        // successor operand.
        let owner = &user.owner;
        if owner.borrow().implements::<dyn Terminator>() {
            if let Some(branch_interface) = owner.borrow().as_trait::<dyn BranchOpInterface>() {
                if let Some(arg) =
                    branch_interface.get_successor_block_argument(user.index as usize)
                {
                    return !self.was_proven_live(&arg.upcast());
                }
            }
        }

        false
    }

    pub fn propagate_region_liveness(&mut self, region: &Region) {
        if region.body().is_empty() {
            return;
        }

        for block in PostOrderBlockIter::new(region.body().front().as_pointer().unwrap()) {
            // We process block arguments after the ops in the block, to promote faster convergence
            // to a fixed point (we try to visit uses before defs).
            let block = block.borrow();
            for op in block.body().iter().rev() {
                self.propagate_liveness(&op);
            }

            // We currently do not remove entry block arguments, so there is no need to track their
            // liveness.
            //
            // TODO(pauls): We could track these and enable removing dead operands/arguments from
            // region control flow operations in the future.
            if block.is_entry_block() {
                continue;
            }

            for arg in block.arguments().iter().copied() {
                let arg = arg as ValueRef;
                if !self.was_proven_live(&arg) {
                    self.process_value(arg);
                }
            }
        }
    }

    pub fn propagate_liveness(&mut self, op: &Operation) {
        // Recurse on any regions the op has
        for region in op.regions() {
            self.propagate_region_liveness(&region);
        }

        // We process terminator operations separately
        if op.implements::<dyn Terminator>() {
            return self.propagate_terminator_liveness(op);
        }

        // Don't reprocess live operations.
        if self.was_op_proven_live(&op.as_operation_ref()) {
            return;
        }

        // Process this op
        if !op.would_be_trivially_dead() {
            self.set_op_proved_live(op.as_operation_ref());
        }

        // If the op isn't intrinsically alive, check it's results
        for result in op.results().iter().copied() {
            self.process_value(result as ValueRef);
        }
    }

    fn propagate_terminator_liveness(&mut self, op: &Operation) {
        // Terminators are always live
        self.set_op_proved_live(op.as_operation_ref());

        // Check to see if we can reason about the successor operands
        //
        // If we can't reason about the operand to a successor, conservatively mark it as live
        if let Some(branch_op) = op.as_trait::<dyn BranchOpInterface>() {
            let num_successors = branch_op.num_successors();
            for successor_idx in 0..num_successors {
                let operands = branch_op.get_successor_operands(successor_idx);
                let succ = op.successor(successor_idx).dest.borrow().successor();
                // Produced operands are always live if the terminator is live
                for arg in succ.borrow().arguments().iter().copied().take(operands.num_produced()) {
                    self.set_proved_live(arg as ValueRef);
                }
            }
        } else {
            for successor in op.successors().iter() {
                let successor = successor.block.borrow().successor();
                for arg in successor.borrow().arguments().iter().copied() {
                    self.set_proved_live(arg as ValueRef);
                }
            }
        }
    }

    fn process_value(&mut self, value: ValueRef) {
        let proved_live = value.borrow().iter_uses().any(|user| {
            if self.is_use_specially_known_dead(&user) {
                return false;
            }
            self.was_op_proven_live(&user.owner)
        });
        if proved_live {
            self.set_proved_live(value);
        }
    }
}

impl Region {
    pub fn dead_code_elimination(
        regions: &[RegionRef],
        rewriter: &mut dyn Rewriter,
    ) -> Result<(), RegionTransformFailed> {
        log::debug!(target: "region-simplify", "starting region dead code elimination");
        let mut live_map = LiveMap::default();
        loop {
            live_map.mark_unchanged();

            log::trace!(target: "region-simplify", "propagating region liveness");

            for region in regions {
                live_map.propagate_region_liveness(&region.borrow());
            }

            if !live_map.has_changed() {
                log::trace!(target: "region-simplify", "liveness propagation has reached fixpoint");
                break;
            }
        }

        Self::cleanup_dead_code(regions, rewriter, &live_map)
    }

    /// Erase the unreachable blocks within the regions in `regions`.
    ///
    /// Returns `Ok` if any blocks were erased, `Err` otherwise.
    pub fn erase_unreachable_blocks(
        regions: &[RegionRef],
        rewriter: &mut dyn crate::Rewriter,
    ) -> Result<(), RegionTransformFailed> {
        let mut erased_dead_blocks = false;
        let mut reachable = SmallSet::<_, 8>::default();
        let mut worklist = VecDeque::from_iter(regions.iter().cloned());
        while let Some(mut region) = worklist.pop_front() {
            log::debug!(target: "region-simplify", "erasing unreachable blocks in region");
            let mut current_region = region.borrow_mut();
            let blocks = current_region.body_mut();
            if blocks.is_empty() {
                log::debug!(target: "region-simplify", "skipping empty region");
                continue;
            }

            // If this is a single block region, just collect nested regions.
            let entry = blocks.front().as_pointer().unwrap();
            if entry.next().is_none() {
                log::trace!(target: "region-simplify", "region is a single-block ({entry}) region: adding nested regions to worklist");
                for op in blocks.front().get().unwrap().body() {
                    worklist.extend(op.regions().iter().map(|r| r.as_region_ref()));
                }
                continue;
            }

            // Mark all reachable blocks.
            log::trace!(target: "region-simplify", "locating reachable blocks from {entry}");
            reachable.clear();
            let iter = PostOrderBlockIter::new(entry);
            reachable.extend(iter);

            // Collect all of the dead blocks and push the live regions on the worklist
            let mut cursor = entry.next();
            drop(current_region);
            while let Some(mut block) = cursor.take() {
                cursor = block.next();

                if reachable.contains(&block) {
                    log::trace!(target: "region-simplify", "{block} is reachable - adding nested regions to worklist");
                    // Walk any regions within this block
                    for op in block.borrow().body() {
                        worklist.extend(op.regions().iter().map(|r| r.as_region_ref()));
                    }
                    continue;
                }

                // The block is unreachable, erase it
                log::trace!(target: "region-simplify", "{block} is unreachable - erasing block");
                block.borrow_mut().drop_all_defined_value_uses();
                rewriter.erase_block(block);
                erased_dead_blocks = true;
            }
        }

        if erased_dead_blocks {
            Ok(())
        } else {
            Err(RegionTransformFailed)
        }
    }

    fn cleanup_dead_code(
        regions: &[RegionRef],
        rewriter: &mut dyn Rewriter,
        live_map: &LiveMap,
    ) -> Result<(), RegionTransformFailed> {
        log::debug!(target: "region-simplify", "cleaning up dead code");

        let mut erased_anything = false;
        for region in regions {
            let current_region = region.borrow();
            if current_region.body().is_empty() {
                log::trace!(target: "region-simplify", "skipping empty region");
                continue;
            }

            let has_single_block = current_region.has_one_block();

            // Delete every operation that is not live. Graph regions may have cycles in the use-def
            // graph, so we must explicitly drop all uses from each operation as we erase it.
            // Visiting the operations in post-order guarantees that in SSA CFG regions, value uses
            // are removed before defs, which makes `drop_all_uses` a no-op.
            let region_entry = current_region.entry_block_ref().unwrap();
            log::debug!(target: "region-simplify", "visiting reachable blocks from {region_entry}");
            let iter = PostOrderBlockIter::new(region_entry);
            for block in iter {
                log::trace!(target: "region-simplify", "visiting block {block}");
                if !has_single_block {
                    Self::erase_terminator_successor_operands(
                        block.borrow().terminator().expect("expected block to have terminator"),
                        live_map,
                    );
                }
                log::trace!(target: "region-simplify", "visiting ops in {block} in post-order");
                let mut next_op = block.borrow().body().back().as_pointer();
                while let Some(mut child_op) = next_op.take() {
                    next_op = child_op.prev();
                    if !live_map.was_op_proven_live(&child_op) {
                        log::trace!(
                            target: "region-simplify", "found '{}' that was not proven live - erasing",
                            child_op.name()
                        );
                        erased_anything = true;
                        child_op.borrow_mut().drop_all_uses();
                        rewriter.erase_op(child_op);
                    } else {
                        let child_op = child_op.borrow();
                        if child_op.regions().is_empty() {
                            log::trace!(target: "region-simplify", "found '{}' that was proven live", child_op.name());
                            continue;
                        }
                        let child_regions = child_op
                            .regions()
                            .iter()
                            .map(|r| r.as_region_ref())
                            .collect::<SmallVec<[RegionRef; 2]>>();
                        log::trace!(
                            target: "region-simplify", "found '{}' that was proven live - cleaning up {} child regions",
                            child_op.name(),
                            child_regions.len()
                        );
                        erased_anything |=
                            Self::cleanup_dead_code(&child_regions, rewriter, live_map).is_ok();
                    }
                }
            }

            // Delete block arguments.
            //
            // The entry block has an unknown contract with their enclosing block, so leave it alone.
            drop(current_region);
            let mut current_block = region_entry.next();
            while let Some(mut block) = current_block.take() {
                log::debug!(target: "region-simplify", "deleting unused block arguments for {block}");
                current_block = block.next();
                block.borrow_mut().erase_arguments(|arg| {
                    let is_dead = !live_map.was_proven_live(&arg.as_value_ref());
                    if is_dead {
                        log::trace!(target: "region-simplify", "{arg} was not proven live - erasing");
                    }
                    is_dead
                });
            }
        }

        if erased_anything {
            Ok(())
        } else {
            Err(RegionTransformFailed)
        }
    }

    fn erase_terminator_successor_operands(mut terminator: OperationRef, live_map: &LiveMap) {
        let mut op = terminator.borrow_mut();
        if !op.implements::<dyn BranchOpInterface>() {
            return;
        }

        log::debug!(
            target: "region-simplify", "erasing branch successor operands for {op} ({} successors)",
            op.num_successors()
        );

        // Iterate successors in reverse to minimize the amount of operand shifting
        for succ_index in (0..op.num_successors()).rev() {
            let mut succ = op.successor_mut(succ_index);
            let block = succ.dest.borrow().successor();
            // Iterate arguments in reverse so that erasing an argument does not shift the others
            let num_arguments = succ.arguments.len();
            log::trace!(target: "region-simplify", "checking successor {block} for unused arguments");
            assert_eq!(num_arguments, block.borrow().num_arguments());
            for arg_index in (0..num_arguments).rev() {
                let arg = block.borrow().get_argument(arg_index) as ValueRef;
                let is_dead = !live_map.was_proven_live(&arg);
                if is_dead {
                    log::trace!(target: "region-simplify", "{arg} was not proven live - erasing");
                    succ.arguments.erase(arg_index);
                }
            }
        }
    }
}
