mod backward;
#[cfg(test)]
mod examples;
mod forward;
mod lattice;

pub use self::{
    backward::DenseBackwardDataFlowAnalysis, forward::DenseForwardDataFlowAnalysis,
    lattice::DenseLattice,
};
use super::{
    AnalysisStrategy, Backward, DataFlowAnalysis, DataFlowSolver, Dense, Forward, ProgramPoint,
};
use crate::{Operation, Report};

pub struct DenseDataFlowAnalysis<T, D> {
    analysis: T,
    _direction: core::marker::PhantomData<D>,
}

impl<A: DenseForwardDataFlowAnalysis> AnalysisStrategy<A> for DenseDataFlowAnalysis<A, Forward> {
    type Direction = Forward;
    type Kind = Dense;

    fn build(analysis: A, _solver: &mut DataFlowSolver) -> Self {
        Self {
            analysis,
            _direction: core::marker::PhantomData,
        }
    }
}

impl<A: DenseBackwardDataFlowAnalysis> AnalysisStrategy<A> for DenseDataFlowAnalysis<A, Backward> {
    type Direction = Backward;
    type Kind = Dense;

    fn build(analysis: A, _solver: &mut DataFlowSolver) -> Self {
        Self {
            analysis,
            _direction: core::marker::PhantomData,
        }
    }
}

impl<A: DenseForwardDataFlowAnalysis> DataFlowAnalysis for DenseDataFlowAnalysis<A, Forward> {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    /// Initialize the analysis by visiting every program point whose execution may modify the
    /// program state; that is, every operation and block.
    fn initialize(&self, top: &Operation, solver: &mut DataFlowSolver) -> Result<(), Report> {
        // Visit every operation and block
        forward::process_operation(&self.analysis, top, solver)?;

        for region in top.regions() {
            for block in region.body() {
                forward::visit_block(&self.analysis, &block, solver);
                for op in block.body() {
                    self.initialize(&op, solver)?;
                }
            }
        }
        Ok(())
    }

    /// Visit a program point that modifies the state of the program.
    ///
    /// If the program point is at the beginning of a block, then the state is propagated from
    /// control-flow predecessors or callsites. If the operation before the program point is a call
    /// operation or region control-flow operation, then the state after the execution of the
    /// operation is set by control-flow or the callgraph. Otherwise, this function invokes the
    /// operation transfer function before the program point iterator.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report> {
        if point.is_at_block_start() {
            let block = point.block().expect("expected block");
            forward::visit_block(&self.analysis, &block.borrow(), solver);
        } else {
            let op = point.operation().expect("expected operation");
            forward::process_operation(&self.analysis, &op.borrow(), solver)?;
        }

        Ok(())
    }
}

impl<A: DenseBackwardDataFlowAnalysis> DataFlowAnalysis for DenseDataFlowAnalysis<A, Backward> {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    /// Initialize the analysis by visiting every program point whose execution may modify the
    /// program state; that is, every operation and block.
    fn initialize(&self, top: &Operation, solver: &mut DataFlowSolver) -> Result<(), Report> {
        // Visit every operation and block
        backward::process_operation(&self.analysis, top, solver)?;

        for region in top.regions() {
            for block in region.body() {
                backward::visit_block(&self.analysis, &block, solver);
                let mut ops = block.body().back();
                while let Some(op) = ops.as_pointer() {
                    ops.move_prev();
                    self.initialize(&op.borrow(), solver)?;
                }
            }
        }
        Ok(())
    }

    /// Visit a program point that modifies the state of the program. If the program point is at the
    /// beginning of a block, then the state is propagated from control-flow predecessors or
    /// callsites.  If the operation before the program point is a call operation or region
    /// control-flow operation, then the state after the execution of the operation is set by
    /// control-flow or the callgraph. Otherwise, this function invokes the operation transfer
    /// function before the program point.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report> {
        if point.is_at_block_end() {
            let block = point.block().expect("expected block");
            backward::visit_block(&self.analysis, &block.borrow(), solver);
        } else {
            let op = point.next_operation().expect("expected operation");
            backward::process_operation(&self.analysis, &op.borrow(), solver)?;
        }

        Ok(())
    }
}
