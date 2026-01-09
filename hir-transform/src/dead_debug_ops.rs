//! This pass removes debug operations (DbgValue) whose operands are no longer
//! live. This prevents issues during codegen where the operand stack state
//! becomes inconsistent due to debug ops referencing dropped values.

use alloc::vec::Vec;

use midenc_hir::{
    EntityMut, Operation, OperationName, OperationRef, Report,
    dialects::builtin,
    pass::{Pass, PassExecutionState, PostPassStatus},
};
use midenc_hir_analysis::analyses::LivenessAnalysis;

/// Removes debug operations whose operands are dead.
///
/// Debug operations like `DbgValue` reference SSA values to provide debug
/// information. However, these operations don't actually consume their operands;
/// they just observe them. This can cause issues during codegen when the
/// referenced value has been dropped from the operand stack.
///
/// This pass removes debug ops whose operands are not live after the debug op.
/// If a value is live after the debug op, it will still be available on the
/// operand stack during codegen and can be safely observed.
pub struct RemoveDeadDebugOps;

impl Pass for RemoveDeadDebugOps {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "remove-dead-debug-ops"
    }

    fn argument(&self) -> &'static str {
        "remove-dead-debug-ops"
    }

    fn description(&self) -> &'static str {
        "Removes debug operations whose operands are dead"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let op_ref = op.as_operation_ref();
        drop(op);

        // Collect all debug ops to potentially remove
        let mut debug_ops_to_check: Vec<OperationRef> = Vec::new();

        collect_debug_ops(&op_ref, &mut debug_ops_to_check);

        if debug_ops_to_check.is_empty() {
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        // Get liveness analysis
        let analysis_manager = state.analysis_manager();
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        let mut removed_any = false;

        // Check each debug op and remove if its operand will be dead by codegen time
        for mut debug_op in debug_ops_to_check {
            let should_remove = {
                let debug_op_borrowed = debug_op.borrow();

                // Get the operand (first operand for DbgValue)
                let operands = debug_op_borrowed.operands();
                if operands.is_empty() {
                    continue;
                }

                let operand = operands.iter().next().unwrap();
                let operand_value = operand.borrow().as_value_ref();

                // Only remove debug ops if their operand is not live after the debug op.
                // If the value is live after, it will still be on the operand stack
                // during codegen and can be safely observed by the debug op.
                //
                // Note: We previously also removed debug ops if the value had other uses,
                // but this was too aggressive - if the value is live after the debug op,
                // it doesn't matter how many uses it has; it's still available.
                !liveness.is_live_after(operand_value, &debug_op_borrowed)
            };

            if should_remove {
                debug_op.borrow_mut().erase();
                removed_any = true;
            }
        }

        state.set_post_pass_status(if removed_any {
            PostPassStatus::Changed
        } else {
            PostPassStatus::Unchanged
        });

        Ok(())
    }
}

/// Recursively collect all debug operations in the given operation
fn collect_debug_ops(op: &OperationRef, debug_ops: &mut Vec<OperationRef>) {
    let op = op.borrow();

    // Check if this is a debug op
    if op.is::<builtin::DbgValue>() {
        debug_ops.push(op.as_operation_ref());
    }

    // Recurse into regions
    for region in op.regions() {
        for block in region.body() {
            for inner_op in block.body() {
                collect_debug_ops(&inner_op.as_operation_ref(), debug_ops);
            }
        }
    }
}
