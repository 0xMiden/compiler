mod backward;
mod forward;
mod lattice;

pub use self::{
    backward::{set_all_to_exit_states, SparseBackwardDataFlowAnalysis},
    forward::{set_all_to_entry_states, SparseForwardDataFlowAnalysis},
    lattice::{Lattice, LatticeValue, SparseLattice},
};
use super::{
    AnalysisStrategy, Backward, DataFlowAnalysis, DataFlowSolver, Forward, ProgramPoint, Sparse,
};
use crate::{Operation, Report};

pub struct SparseDataFlowAnalysis<A, D> {
    analysis: A,
    _direction: core::marker::PhantomData<D>,
}

impl<A: SparseForwardDataFlowAnalysis> AnalysisStrategy<A> for SparseDataFlowAnalysis<A, Forward> {
    type Direction = Forward;
    type Kind = Sparse;

    fn build(analysis: A, _solver: &mut DataFlowSolver) -> Self {
        Self {
            analysis,
            _direction: core::marker::PhantomData,
        }
    }
}

impl<A: SparseBackwardDataFlowAnalysis> AnalysisStrategy<A>
    for SparseDataFlowAnalysis<A, Backward>
{
    type Direction = Backward;
    type Kind = Sparse;

    fn build(analysis: A, _solver: &mut DataFlowSolver) -> Self {
        Self {
            analysis,
            _direction: core::marker::PhantomData,
        }
    }
}

impl<A: SparseForwardDataFlowAnalysis> DataFlowAnalysis for SparseDataFlowAnalysis<A, Forward> {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    /// Initialize the analysis by visiting every owner of an SSA value: all operations and blocks.
    fn initialize(&self, top: &Operation, solver: &mut DataFlowSolver) -> Result<(), Report> {
        // Mark the entry block arguments as having reached their pessimistic fixpoints.
        for region in top.regions() {
            if region.is_empty() {
                continue;
            }

            for argument in region.entry().arguments() {
                let argument = argument.borrow().as_value_ref();
                let mut lattice = solver.get_or_create_mut::<_, _>(argument);
                <A as SparseForwardDataFlowAnalysis>::set_to_entry_state(
                    &self.analysis,
                    &mut lattice,
                );
            }
        }

        forward::initialize_recursively(&self.analysis, top, solver)
    }

    /// Visit a program point.
    ///
    /// If this is at beginning of block and all control-flow predecessors or callsites are known,
    /// then the arguments lattices are propagated from them. If this is after call operation or an
    /// operation with region control-flow, then its result lattices are set accordingly. Otherwise,
    /// the operation transfer function is invoked.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report> {
        if !point.is_at_block_start() {
            return forward::visit_operation(
                &self.analysis,
                &point.prev_operation().unwrap().borrow(),
                solver,
            );
        }

        forward::visit_block(&self.analysis, &point.block().unwrap().borrow(), solver);

        Ok(())
    }
}

impl<A: SparseBackwardDataFlowAnalysis> DataFlowAnalysis for SparseDataFlowAnalysis<A, Backward> {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    /// Initialize the analysis by visiting the operation and everything nested under it.
    fn initialize(&self, top: &Operation, solver: &mut DataFlowSolver) -> Result<(), Report> {
        backward::initialize_recursively(&self.analysis, top, solver)
    }

    /// Visit a program point.
    ///
    /// If it is after call operation or an operation with block or region control-flow, then
    /// operand lattices are set accordingly. Otherwise, invokes the operation transfer function.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report> {
        // For backward dataflow, we don't have to do any work for the blocks themselves. CFG edges
        // between blocks are processed by the BranchOp logic in `visit_operation`, and entry blocks
        // for functions are tied to the CallOp arguments by `visit_operation`.
        if point.is_at_block_start() {
            Ok(())
        } else {
            backward::visit_operation(
                &self.analysis,
                &point.prev_operation().unwrap().borrow(),
                solver,
            )
        }
    }
}
