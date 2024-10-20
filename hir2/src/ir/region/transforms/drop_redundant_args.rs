use smallvec::SmallVec;

use super::RegionTransformFailed;
use crate::{
    traits::BranchOpInterface, BlockArgumentRef, BlockRef, Region, RegionRef, Rewriter,
    SuccessorOperands, Usable,
};

impl Region {
    /// This optimization drops redundant argument to blocks. I.e., if a given argument to a block
    /// receives the same value from each of the block predecessors, we can remove the argument from
    /// the block and use directly the original value.
    ///
    /// ## Example
    ///
    /// A simple example:
    ///
    /// ```hir,ignore
    /// %cond = llvm.call @rand() : () -> i1
    /// %val0 = llvm.mlir.constant(1 : i64) : i64
    /// %val1 = llvm.mlir.constant(2 : i64) : i64
    /// %val2 = llvm.mlir.constant(3 : i64) : i64
    /// llvm.cond_br %cond, ^bb1(%val0 : i64, %val1 : i64), ^bb2(%val0 : i64, %val2
    /// : i64)
    ///
    /// ^bb1(%arg0 : i64, %arg1 : i64):
    ///    llvm.call @foo(%arg0, %arg1)
    /// ```
    ///
    /// That can be rewritten as:
    ///
    /// ```hir,ignore
    /// %cond = llvm.call @rand() : () -> i1
    /// %val0 = llvm.mlir.constant(1 : i64) : i64
    /// %val1 = llvm.mlir.constant(2 : i64) : i64
    /// %val2 = llvm.mlir.constant(3 : i64) : i64
    /// llvm.cond_br %cond, ^bb1(%val1 : i64), ^bb2(%val2 : i64)
    ///
    /// ^bb1(%arg0 : i64):
    ///    llvm.call @foo(%val0, %arg0)
    /// ```
    pub(in crate::ir::region) fn drop_redundant_arguments(
        regions: &[RegionRef],
        rewriter: &mut dyn Rewriter,
    ) -> Result<(), RegionTransformFailed> {
        let mut worklist = SmallVec::<[RegionRef; 1]>::from_iter(regions.iter().cloned());

        let mut any_changed = false;
        while let Some(region) = worklist.pop() {
            // Add any nested regions to the worklist
            let region = region.borrow();
            let mut blocks = region.body().front();
            while let Some(block) = blocks.as_pointer() {
                blocks.move_next();

                any_changed |=
                    Self::drop_redundant_block_arguments(block.clone(), rewriter).is_ok();

                for op in block.borrow().body() {
                    let mut regions = op.regions().front();
                    while let Some(region) = regions.as_pointer() {
                        worklist.push(region);
                        regions.move_next();
                    }
                }
            }
        }

        if any_changed {
            Ok(())
        } else {
            Err(RegionTransformFailed)
        }
    }

    /// If a block's argument is always the same across different invocations, then
    /// drop the argument and use the value directly inside the block
    fn drop_redundant_block_arguments(
        mut block: BlockRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<(), RegionTransformFailed> {
        let mut args_to_erase = SmallVec::<[usize; 4]>::default();

        // Go through the arguments of the block.
        let mut block_mut = block.borrow_mut();
        let block_args =
            SmallVec::<[BlockArgumentRef; 2]>::from_iter(block_mut.arguments().iter().cloned());
        for (arg_index, block_arg) in block_args.into_iter().enumerate() {
            let mut same_arg = true;
            let mut common_value = None;

            // Go through the block predecessor and flag if they pass to the block different values
            // for the same argument.
            for pred in block_mut.predecessors() {
                let pred_op = pred.owner.borrow();
                if let Some(branch_op) = pred_op.as_trait::<dyn BranchOpInterface>() {
                    let succ_index = pred.index as usize;
                    let succ_operands = branch_op.get_successor_operands(succ_index);
                    let branch_operands = succ_operands.forwarded();
                    let arg = branch_operands[arg_index].borrow().as_value_ref();
                    if common_value.is_none() {
                        common_value = Some(arg);
                        continue;
                    }
                    if common_value.as_ref().is_some_and(|cv| cv != &arg) {
                        same_arg = false;
                        break;
                    }
                } else {
                    same_arg = false;
                    break;
                }
            }

            // If they are passing the same value, drop the argument.
            if let Some(common_value) = common_value {
                if same_arg {
                    args_to_erase.push(arg_index);

                    // Remove the argument from the block.
                    rewriter.replace_all_uses_of_value_with(block_arg, common_value);
                }
            }
        }

        // Remove the arguments.
        for arg_index in args_to_erase.iter().copied() {
            block_mut.erase_argument(arg_index);

            // Remove the argument from the branch ops.
            let mut preds = block_mut.uses_mut().front_mut();
            while let Some(mut pred) = preds.as_pointer() {
                preds.move_next();

                let mut pred = pred.borrow_mut();
                let mut pred_op = pred.owner.borrow_mut();
                if let Some(branch_op) = pred_op.as_trait_mut::<dyn BranchOpInterface>() {
                    let succ_index = pred.index as usize;
                    let mut succ_operands = branch_op.get_successor_operands_mut(succ_index);
                    succ_operands.forwarded_mut().erase(arg_index);
                }
            }
        }

        if !args_to_erase.is_empty() {
            Ok(())
        } else {
            Err(RegionTransformFailed)
        }
    }
}
