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
use crate::{
    cfg::Graph,
    dataflow::analyses::{dce::CfgEdge, LoopAction, LoopState},
    dominance::DominanceInfo,
    loops::LoopForest,
    pass::AnalysisManager,
    BlockRef, Operation, RegionKindInterface, Report, Spanned,
};

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
    fn initialize(
        &self,
        top: &Operation,
        solver: &mut DataFlowSolver,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        forward::process_operation(&self.analysis, top, solver)?;

        // If the op has SSACFG regions, use the dominator tree analysis, if available, to visit the
        // CFG top-down. Otherwise, fall back to a naive iteration over the contents of each region.
        //
        // If we have a domtree, we don't bother visiting unreachable blocks (i.e. blocks that
        // are not in the tree because they are unreachable via the CFG). If we don't have a domtree,
        // then all blocks are visited, regardless of reachability.
        if !top.has_regions() {
            return Ok(());
        }

        let is_ssa_cfg = top
            .as_trait::<dyn RegionKindInterface>()
            .is_none_or(|rki| rki.has_ssa_dominance());
        if is_ssa_cfg {
            let dominfo = analysis_manager.get_analysis::<DominanceInfo>();
            for region in top.regions() {
                // Single-block regions do not require a dominance tree (and do not have one)
                if region.has_one_block() {
                    let block = region.entry();
                    forward::visit_block(&self.analysis, &block, solver);
                    for op in block.body() {
                        let child_analysis_manager = analysis_manager.nest(&op.as_operation_ref());
                        self.initialize(&op, solver, child_analysis_manager)?;
                    }
                } else {
                    let domtree = dominfo.info().dominance(&region.as_region_ref());
                    let loop_forest = LoopForest::new(&domtree);

                    // Visit blocks in CFG preorder
                    for node in domtree.preorder() {
                        let Some(block) = node.block().cloned() else {
                            continue;
                        };

                        // Anchor the fact that a loop is being exited to the CfgEdge of the exit,
                        // if applicable for this block
                        if let Some(loop_info) = loop_forest.loop_for(&block) {
                            // This block can exit a loop
                            if loop_info.is_loop_exiting(&block) {
                                for succ in BlockRef::children(block.clone()) {
                                    if !loop_info.contains_block(&succ) {
                                        let mut guard = solver.get_or_create_mut::<LoopState, _>(
                                            CfgEdge::new(block.clone(), succ, block.span()),
                                        );
                                        guard.join(LoopAction::Exit);
                                    }
                                }
                            }
                        }

                        let block = block.borrow();
                        forward::visit_block(&self.analysis, &block, solver);
                        for op in block.body() {
                            let child_analysis_manager =
                                analysis_manager.nest(&op.as_operation_ref());
                            self.initialize(&op, solver, child_analysis_manager)?;
                        }
                    }
                }
            }
        } else {
            for region in top.regions() {
                for block in region.body() {
                    forward::visit_block(&self.analysis, &block, solver);
                    for op in block.body() {
                        let child_analysis_manager = analysis_manager.nest(&op.as_operation_ref());
                        self.initialize(&op, solver, child_analysis_manager)?;
                    }
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
    fn initialize(
        &self,
        top: &Operation,
        solver: &mut DataFlowSolver,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        backward::process_operation(&self.analysis, top, solver)?;

        // If the op has SSACFG regions, use the dominator tree analysis, if available, to visit the
        // CFG in post-order. Otherwise, fall back to a naive iteration over the contents of each region.
        //
        // If we have a domtree, we don't bother visiting unreachable blocks (i.e. blocks that
        // are not in the tree because they are unreachable via the CFG). If we don't have a domtree,
        // then all blocks are visited, regardless of reachability.
        if !top.has_regions() {
            return Ok(());
        }

        let is_ssa_cfg = top
            .as_trait::<dyn RegionKindInterface>()
            .is_none_or(|rki| rki.has_ssa_dominance());
        if is_ssa_cfg {
            let dominfo = analysis_manager.get_analysis::<DominanceInfo>();
            for region in top.regions() {
                // Single-block regions do not require a dominance tree (and do not have one)
                if region.has_one_block() {
                    let block = region.entry();
                    backward::visit_block(&self.analysis, &block, solver);
                    for op in block.body().iter().rev() {
                        let child_analysis_manager = analysis_manager.nest(&op.as_operation_ref());
                        self.initialize(&op, solver, child_analysis_manager)?;
                    }
                } else {
                    let domtree = dominfo.info().dominance(&region.as_region_ref());
                    let loop_forest = LoopForest::new(&domtree);

                    // Visit blocks in CFG postorder
                    for node in domtree.postorder() {
                        let Some(block) = node.block().cloned() else {
                            continue;
                        };

                        // Anchor the fact that a loop is being exited to the CfgEdge of the exit,
                        // if applicable for this block
                        if let Some(loop_info) = loop_forest.loop_for(&block) {
                            // This block can exit a loop
                            if loop_info.is_loop_exiting(&block) {
                                for succ in BlockRef::children(block.clone()) {
                                    if !loop_info.contains_block(&succ) {
                                        let mut guard = solver.get_or_create_mut::<LoopState, _>(
                                            CfgEdge::new(block.clone(), succ, block.span()),
                                        );
                                        guard.join(LoopAction::Exit);
                                    }
                                }
                            }
                        }

                        let block = block.borrow();
                        backward::visit_block(&self.analysis, &block, solver);
                        for op in block.body().iter().rev() {
                            let child_analysis_manager =
                                analysis_manager.nest(&op.as_operation_ref());
                            self.initialize(&op, solver, child_analysis_manager)?;
                        }
                    }
                }
            }
        } else {
            for region in top.regions() {
                for block in region.body().iter().rev() {
                    backward::visit_block(&self.analysis, &block, solver);
                    for op in block.body().iter().rev() {
                        let child_analysis_manager = analysis_manager.nest(&op.as_operation_ref());
                        self.initialize(&op, solver, child_analysis_manager)?;
                    }
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
