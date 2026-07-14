use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    Backward, Context, EntityMut, Operation, OperationName, OperationRef, RawWalk, Report,
    Rewriter, Value,
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterImpl, TracingRewriterListener},
};
/*
use midenc_hir_analysis::{
    analyses::{DeadCodeAnalysis, SparseConstantPropagation, liveness::Liveness},
};
 */

/// This transformation pass uses liveness analysis to remove any instructions which consist solely
/// of dead values, and which have no side effects (i.e. `MemoryEffect::Write` or
/// `MemoryEffect::Free`).
#[derive(Default)]
pub struct DeadCodeElimination;

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<DeadCodeElimination>(
    "dce",
    "dead code elimination"
));

impl Pass for DeadCodeElimination {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "dead-code-elimination"
    }

    fn argument(&self) -> &'static str {
        "dce"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        // Run sccp + dead code + liveness analysis so we can remove relevant dead ops
        let op = op.into_entity_ref();
        //let mut solver = DataFlowSolver::default();
        //solver.load::<DeadCodeAnalysis>();
        //solver.load::<SparseConstantPropagation>();
        //solver.load::<Liveness>();
        //solver.initialize_and_run(&op, state.analysis_manager().clone())?;

        // Rewrite based on results of analysis
        let context = op.context_rc();
        let op = {
            let op_ref = op.as_operation_ref();
            drop(op);
            op_ref
        };
        self.rewrite(op, context, state)
    }
}

impl DeadCodeElimination {
    fn rewrite(
        &mut self,
        op: OperationRef,
        context: Rc<Context>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);
        let mut changed = PostPassStatus::Unchanged;
        op.raw_postwalk_all::<Backward, _>(|op: OperationRef| {
            // Transparent uses are informational and must not keep runtime computations alive.
            // Preserve standalone transparent ops, but when a producer has no real users, replace
            // its debug-value users with kills before erasing it.
            let dead_results = {
                let op = op.borrow();
                if op.implements::<dyn midenc_hir::traits::Transparent>()
                    || !op.would_be_trivially_dead()
                    || op.results().iter().any(|result| result.borrow().has_real_uses())
                {
                    None
                } else {
                    Some(
                        op.results()
                            .iter()
                            .map(|result| result.borrow().as_value_ref())
                            .collect::<Vec<_>>(),
                    )
                }
            };
            if let Some(results) = dead_results {
                changed = PostPassStatus::Changed;
                for result in results {
                    midenc_hir::dialects::debuginfo::transform::erase_debug_info(&result);
                }
                rewriter.erase_op(op);
            }
        });

        state.set_post_pass_status(changed);

        if !changed.ir_changed() {
            state.preserved_analyses_mut().preserve_all();
        }

        Ok(())
    }
}
