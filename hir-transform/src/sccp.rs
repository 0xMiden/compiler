use midenc_hir::{
    pass::{Pass, PassExecutionState},
    patterns::NoopRewriterListener,
    BlockRef, Builder, EntityMut, OpBuilder, Operation, OperationFolder, OperationName, RegionList,
    Report, SmallVec, ValueRef,
};
use midenc_hir_analysis::{
    analyses::{constant_propagation::ConstantValue, DeadCodeAnalysis, SparseConstantPropagation},
    DataFlowSolver, Lattice,
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
        mut op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        // Run sparse constant propagation + dead code analysis
        let mut solver = DataFlowSolver::default();
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        solver.initialize_and_run(&op, state.analysis_manager().clone())?;

        // Rewrite based on results of analysis
        self.rewrite(&mut op, state, &solver)
    }
}

impl SparseConditionalConstantPropagation {
    /// Rewrite the given regions using the computing analysis. This replaces the uses of all values
    /// that have been computed to be constant, and erases as many newly dead operations.
    fn rewrite(
        &mut self,
        op: &mut Operation,
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
        let context = op.context_rc();
        let mut folder = OperationFolder::new(context.clone(), None::<NoopRewriterListener>);
        let mut builder = OpBuilder::new(context.clone());

        add_to_worklist(op.regions(), &mut worklist);

        let mut replaced_any = false;
        while let Some(mut block) = worklist.pop() {
            let mut block = block.borrow_mut();
            let body = block.body_mut();
            let mut ops = body.front();

            while let Some(mut op) = ops.as_pointer() {
                ops.move_next();

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
                let mut op = op.borrow_mut();
                if replaced_all && op.would_be_trivially_dead() {
                    assert!(!op.is_used(), "expected all uses to be replaced");
                    op.erase();
                    continue;
                }

                // Add any of the regions of this operation to the worklist
                add_to_worklist(op.regions(), &mut worklist);
            }

            // Replace any block arguments with constants
            builder.set_insertion_point_to_start(block.as_block_ref());

            for arg in block.arguments() {
                replaced_any |= replace_with_constant(
                    solver,
                    &mut builder,
                    &mut folder,
                    arg.borrow().as_value_ref(),
                );
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
