use alloc::rc::Rc;

use midenc_hir::{
    BlockRef, Builder, Context, EntityMut, OpBuilder, Operation, OperationFolder, OperationName,
    OperationRef, RegionList, Report, SmallVec, ValueRef,
    pass::{Pass, PassExecutionState},
    patterns::TracingRewriterListener,
};
use midenc_hir_analysis::{
    DataFlowSolver, Lattice,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation, constant_propagation::ConstantValue},
};

/// This pass implements a general algorithm for sparse conditional constant propagation.
///
/// This algorithm detects values that are known to be constant and optimistically propagates this
/// throughout the IR. Any values proven to be constant are replaced, and removed if possible.
///
/// This implementation is based on the algorithm described by Wegman and Zadeck in
/// [“Constant Propagation with Conditional Branches”](https://dl.acm.org/doi/10.1145/103135.103136)
/// (1991).
pub struct SparseConditionalConstantPropagation;

impl Pass for SparseConditionalConstantPropagation {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "sparse-conditional-constant-propagation"
    }

    fn argument(&self) -> &'static str {
        "sparse-conditional-constant-propagation"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        // Run sparse constant propagation + dead code analysis
        let op = op.into_entity_ref();
        let mut solver = DataFlowSolver::default();
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        solver.initialize_and_run(&op, state.analysis_manager().clone())?;

        // Rewrite based on results of analysis
        let context = op.context_rc();
        let op = {
            let op_ref = op.as_operation_ref();
            drop(op);
            op_ref
        };
        self.rewrite(op, context, state, &solver)
    }
}

impl SparseConditionalConstantPropagation {
    /// Rewrite the given regions using the computing analysis. This replaces the uses of all values
    /// that have been computed to be constant, and erases as many newly dead operations.
    fn rewrite(
        &mut self,
        op: OperationRef,
        context: Rc<Context>,
        state: &mut PassExecutionState,
        solver: &DataFlowSolver,
    ) -> Result<(), Report> {
        let mut worklist = SmallVec::<[BlockRef; 8]>::default();

        let add_to_worklist = |regions: &RegionList, worklist: &mut SmallVec<[BlockRef; 8]>| {
            for region in regions {
                for block in region.body().iter().rev() {
                    worklist.push(block.as_block_ref());
                }
            }
        };

        // An operation folder used to create and unique constants.
        let mut folder = OperationFolder::new(context.clone(), TracingRewriterListener);
        let mut builder = OpBuilder::new(context.clone());

        {
            let op = op.borrow();
            add_to_worklist(op.regions(), &mut worklist);
        }

        let mut replaced_any = false;
        while let Some(block) = worklist.pop() {
            let mut current_op = { block.borrow().body().front().as_pointer() };

            while let Some(op) = current_op.take() {
                current_op = op.next();

                builder.set_insertion_point_after(op);

                // Replace any result with constants.
                let num_results = op.borrow().num_results();
                let mut replaced_all = num_results != 0;
                for index in 0..num_results {
                    let result = { op.borrow().get_result(index).borrow().as_value_ref() };
                    let replaced = replace_with_constant(solver, &mut builder, &mut folder, result);

                    replaced_any |= replaced;
                    replaced_all &= replaced;
                }

                // If all of the results of the operation were replaced, try to erase the operation
                // completely.
                let op = op.borrow();
                if replaced_all && op.would_be_trivially_dead() {
                    assert!(!op.is_used(), "expected all uses to be replaced");
                    let mut op = op.into_entity_mut().unwrap();
                    op.erase();
                    continue;
                }

                // Add any of the regions of this operation to the worklist
                add_to_worklist(op.regions(), &mut worklist);
            }

            // Replace any block arguments with constants
            builder.set_insertion_point_to_start(block);

            let block_arguments = SmallVec::<[_; 4]>::from_iter(block.borrow().argument_values());
            for arg in block_arguments {
                replaced_any |= replace_with_constant(solver, &mut builder, &mut folder, arg);
            }
        }

        state.set_post_pass_status(replaced_any.into());

        Ok(())
    }
}

/// Replace the given value with a constant if the corresponding lattice represents a constant.
///
/// Returns success if the value was replaced, failure otherwise.
fn replace_with_constant(
    solver: &DataFlowSolver,
    builder: &mut OpBuilder,
    folder: &mut OperationFolder,
    mut value: ValueRef,
) -> bool {
    let Some(lattice) = solver.get::<Lattice<ConstantValue>, _>(&value) else {
        return false;
    };
    if lattice.value().is_uninitialized() {
        return false;
    }

    let Some(constant_value) = lattice.value().constant_value() else {
        return false;
    };

    // Attempt to materialize a constant for the given value.
    let dialect = lattice.value().constant_dialect().unwrap();
    let constant = folder.get_or_create_constant(
        builder.insertion_block().unwrap(),
        dialect,
        constant_value,
        value.borrow().ty().clone(),
    );
    if let Some(constant) = constant {
        value.borrow_mut().replace_all_uses_with(constant);
        true
    } else {
        false
    }
}
