use midenc_hir::{
    Block, CallOpInterface, CallableOpInterface, Operation, ProgramPoint, RegionBranchOpInterface,
    RegionRef, Report, Spanned,
};

use super::*;
use crate::{
    AnalysisStateGuardMut, BuildableAnalysisState, CallControlFlowAction, DataFlowSolver,
    analyses::dce::{CfgEdge, Executable, PredecessorState},
};

/// The base trait for all dense forward data-flow analyses.
///
/// This type of dense data-flow analysis attaches a lattice to program points and implements a
/// transfer function from the lattice before each operation to the lattice after, thus propagating
/// forwards. The lattice contains information about the state of the program at that program point.
///
/// Visiting a program point will invoke the transfer function of the operation preceding the
/// program point iterator. Visiting a program point at the beginning of a block will visit the
/// block itself.
#[allow(unused_variables)]
pub trait DenseForwardDataFlowAnalysis: 'static {
    type Lattice: BuildableAnalysisState + DenseLattice;

    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    /// Propagate the dense lattice before the execution of an operation to the lattice after its
    /// execution.
    fn visit_operation(
        &self,
        op: &Operation,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report>;

    /// Set the dense lattice at control flow entry point and propagate an update if it changed.
    ///
    /// The lattice here may be anchored to one of the following points:
    ///
    /// 1. `ProgramPoint::at_start_of(block)` for the block being entered
    /// 2. `ProgramPoint::before(op)` for the first op in a block being entered
    /// 3. `ProgramPoint::after(call)` for propagating lattice state from the predecessor of a
    ///    call to a callable op (i.e. from return sites to after the call returns).
    ///
    /// In the case of 2 specifically, we distinguish the anchors "start of block" and "before op
    /// at start of block", however in general these effectively refer to the same program point.
    /// It is up to the implementation to decide how they wish to handle this case, but it is safe
    /// to simply propagate the state from 1 to 2.
    fn set_to_entry_state(
        &self,
        lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    );

    /// Propagate the dense lattice forward along the control flow edge represented by `from` and
    /// `to`, which is known to be the result of intra-region control flow, i.e. via
    /// [BranchOpInterface]. This is invoked when visiting blocks, rather than the terminators of
    /// those blocks. The block being visited when this function is called is `to`.
    ///
    /// The default implementation just invokes `join` on the states, meaning that operations
    /// implementing [BranchOpInterface] don't have any effect on the lattice that isn't already
    /// expressed by the interface itself.
    ///
    /// The lattices are as follows:
    ///
    /// * `before` is the lattice at the end of `from`
    /// * `after` is the lattice at the beginning of `to`
    ///
    /// By default, the `after` state is joined with the `before` state. Implementations can
    /// override this in certain cases. Specifically, if the edge itself should be taken into
    /// account in some way, such as if there are subtleties in the transfer function due to edge
    /// weights or other control flow considerations. For example, one might wish to take into
    /// account the fact that an edge enters or exits a loop.
    fn visit_branch_control_flow_transfer(
        &self,
        from: BlockRef,
        to: &Block,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        after.join(before.lattice());
    }

    /// Propagate the dense lattice forward along the control flow edge from `region_from` to
    /// `region_to`, which must be regions of the `branch` operation, or `None`, which corresponds
    /// to the branch op itself (i.e. control flow originating at, or targeting, the `branch` op).
    ///
    /// The default implementation just invokes `join` on the states, meaning that operations
    /// implementing [RegionBranchOpInterface] don't have any effect on the lattice that isn't
    /// already expressed by the interface itself.
    ///
    /// The lattices are as follows:
    ///
    /// * `before`:
    ///   - If `region_from` is set, this is the lattice at the end of the block that exits the
    ///     region. Note that for multi-exit regions, the lattices are equal at the end of all
    ///     exiting blocks, but they are associated with different program points.
    ///   - Otherwise, this is the lattice before the parent op
    /// * `after`:
    ///   - If `region-to` is set, this is the lattice at the beginning of the entry block of that
    ///     region.
    ///   - Otherwise, this is the lattice after the parent op
    ///
    /// Implementations can implement additional custom behavior by handling specific cases manually.
    /// For example, if the `branch` op may affect the lattice before entering any region, the impl
    /// can handle `region_from.is_none()`. Similarly, if the `branch` op may affect the lattice
    /// after all terminated, the implementation can handle `region_to.is_none()`. Additional
    /// refinements are possible for specific pairs of regions.
    fn visit_region_branch_control_flow_transfer(
        &self,
        branch: &dyn RegionBranchOpInterface,
        region_from: Option<RegionRef>,
        region_to: Option<RegionRef>,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        after.join(before.lattice());
    }

    /// Propagate the dense lattice forward along the call control flow edge, which can be either
    /// entering or exiting the callee.
    ///
    /// The default implementation for enter and exit callee actions invokes `join` on the states,
    /// meaning that operations implementing [CallOpInterface] don't have any effect on the lattice
    /// that isn't already expressed by the interface itself. The default handling for the external
    /// callee action additionally sets the `after` lattice to the entry state.
    ///
    /// Two types of forward-propagation are possible here:
    ///
    /// * `CallControlFlowAction::Enter` indicates:
    ///   - `before` is the state before the call operation
    ///   - `after` is the state at the beginning of the callee entry block
    /// * `CallControlFlowAction::Exit` indicates:
    ///   - `before` is the state at the end of a callee exit block
    ///   - `after` is the state after the call operation
    ///
    /// Implementations can implement additional custom behavior by handling specific cases manually.
    /// For example, if `call` may affect the lattice prior to entering the callee, the impl can
    /// handle `CallControlFlowAction::Enter`. Similarly, if `call` may affect the lattice post-
    /// exiting the callee, the impl can handle `CallControlFlowAction::Exit`.
    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        after.join(before.lattice());
        // Note that `set_to_entry_state` may be a "partial fixpoint" for some
        // lattices, e.g., lattices that are lists of maps of other lattices will
        // only set fixpoint for "known" lattices.
        if matches!(action, CallControlFlowAction::External) {
            self.set_to_entry_state(after, solver);
        }
    }
}

/// Visit an operation. If this is a call operation or region control-flow
/// operation, then the state after the execution of the operation is set by
/// control-flow or the callgraph. Otherwise, this function invokes the
/// operation transfer function.
pub fn process_operation<A>(
    analysis: &DenseDataFlowAnalysis<A, Forward>,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: DenseForwardDataFlowAnalysis,
{
    let point: ProgramPoint = ProgramPoint::after(op);

    // If the containing block is not executable, bail out.
    let not_executable = op.parent().is_some_and(|block| {
        let block_start = ProgramPoint::at_start_of(block);
        !solver.require::<Executable, _>(block_start, point).is_live()
    });
    if not_executable {
        return Ok(());
    }

    // Get the dense lattice to update.
    let mut after = solver.get_or_create_mut(point);

    // If this op implements region control-flow, then control-flow dictates its transfer
    // function.
    if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
        visit_region_branch_operation(analysis, point, branch, &mut after, solver);
        return Ok(());
    }

    // Get the dense state before the execution of the op.
    let before = solver.require(ProgramPoint::before(op), point);

    // If this is a call operation, then join its lattices across known return sites.
    if let Some(call) = op.as_trait::<dyn CallOpInterface>() {
        visit_call_operation(analysis, call, &before, &mut after, solver);
        return Ok(());
    }

    // Invoke the operation transfer function.
    analysis.visit_operation(op, &before, &mut after, solver)
}

/// Visit a block. The state at the start of the block is propagated from
/// control-flow predecessors or callsites.
pub fn visit_block<A>(
    analysis: &DenseDataFlowAnalysis<A, Forward>,
    block: &Block,
    solver: &mut DataFlowSolver,
) where
    A: DenseForwardDataFlowAnalysis,
{
    // If the block is not executable, bail out.
    let point = ProgramPoint::at_start_of(block);
    if !solver.require::<Executable, _>(point, point).is_live() {
        return;
    }

    // Get the dense lattice to update.
    let mut after = solver.get_or_create_mut(point);

    // The dense lattices of entry blocks are set by region control-flow or the callgraph.
    if block.is_entry_block() {
        // Check if this block is the entry block of a callable region.
        let op = block.parent_op().expect("orphaned block");
        let operation = op.borrow();
        if let Some(callable) = operation.as_trait::<dyn CallableOpInterface>() {
            let region = block.parent().unwrap();
            let callable_region = callable.get_callable_region();
            if callable_region.is_some_and(|r| r == region) {
                let callsites = solver.require::<PredecessorState, _>(
                    ProgramPoint::after(callable.as_operation()),
                    point,
                );
                // If not all callsites are known, conservatively mark all lattices as having
                // reached their pessimistic fixpoints. Do the same if interprocedural analysis
                // is not enabled.
                if !callsites.all_predecessors_known() || !solver.config().is_interprocedural() {
                    return analysis.set_to_entry_state(&mut after, solver);
                }

                for callsite in callsites.known_predecessors() {
                    // Get the dense lattice before the callsite.
                    let before = solver.require(ProgramPoint::before(*callsite), point);
                    let call = callsite.borrow();
                    let call = call.as_trait::<dyn CallOpInterface>().unwrap();
                    analysis.visit_call_control_flow_transfer(
                        call,
                        CallControlFlowAction::Enter,
                        &before,
                        &mut after,
                        solver,
                    );
                }
                return;
            }
        }

        // Check if we can reason about the control-flow.
        if let Some(branch) = operation.as_trait::<dyn RegionBranchOpInterface>() {
            return visit_region_branch_operation(analysis, point, branch, &mut after, solver);
        }

        // Otherwise, we can't reason about the data-flow.
        return analysis.set_to_entry_state(&mut after, solver);
    }

    // Join the state with the state after the block's predecessors.
    for pred in block.predecessors() {
        // Skip control edges that aren't executable.
        let predecessor = pred.predecessor();
        let anchor = CfgEdge::new(predecessor, pred.successor(), block.span());
        if !solver.require::<Executable, _>(anchor, point).is_live() {
            continue;
        }

        // Merge in the state from the predecessor's terminator.
        let before = solver.require::<<A as DenseForwardDataFlowAnalysis>::Lattice, _>(
            ProgramPoint::after(pred.owner),
            point,
        );
        analysis.visit_branch_control_flow_transfer(
            predecessor,
            block,
            &before,
            &mut after,
            solver,
        );
    }
}

/// Visit an operation for which the data flow is described by the
/// `CallOpInterface`.
pub fn visit_call_operation<A>(
    analysis: &DenseDataFlowAnalysis<A, Forward>,
    call: &dyn CallOpInterface,
    before: &<A as DenseForwardDataFlowAnalysis>::Lattice,
    after: &mut AnalysisStateGuardMut<'_, <A as DenseForwardDataFlowAnalysis>::Lattice>,
    solver: &mut DataFlowSolver,
) where
    A: DenseForwardDataFlowAnalysis,
{
    // Allow for customizing the behavior of calls to external symbols, including when the
    // analysis is explicitly marked as non-interprocedural.
    let symbol = call.resolve();
    let symbol = symbol.as_ref().map(|s| s.borrow());
    let callable_op = symbol.as_ref().map(|s| s.as_symbol_operation());
    let callable = callable_op.and_then(|op| op.as_trait::<dyn CallableOpInterface>());
    if !solver.config().is_interprocedural()
        || callable.is_some_and(|callable| callable.get_callable_region().is_none())
    {
        return analysis.visit_call_control_flow_transfer(
            call,
            CallControlFlowAction::External,
            before,
            after,
            solver,
        );
    }

    // Otherwise, if not all return sites are known, then conservatively assume we
    // can't reason about the data-flow.
    let call_op = call.as_operation().as_operation_ref();
    let after_call = ProgramPoint::after(call_op);
    let predecessors = solver.require::<PredecessorState, _>(after_call, after_call);
    if !predecessors.all_predecessors_known() {
        return analysis.set_to_entry_state(after, solver);
    }

    for predecessor in predecessors.known_predecessors() {
        // Get the lattices at callee return:
        //
        //   func.func @callee() {
        //     ...
        //     return  // predecessor
        //     // latticeAtCalleeReturn
        //   }
        //   func.func @caller() {
        //     ...
        //     call @callee
        //     // latticeAfterCall
        //     ...
        //   }
        let lattice_after_call = &mut *after;
        let lattice_at_callee_return =
            solver.require(ProgramPoint::after(*predecessor), ProgramPoint::after(call_op));
        analysis.visit_call_control_flow_transfer(
            call,
            CallControlFlowAction::Exit,
            &lattice_at_callee_return,
            lattice_after_call,
            solver,
        );
    }
}

/// Visit a program point within a region branch operation with predecessors
/// in it. This can either be an entry block of one of the regions of the
/// parent operation itself.
pub fn visit_region_branch_operation<A>(
    analysis: &DenseDataFlowAnalysis<A, Forward>,
    point: ProgramPoint,
    branch: &dyn RegionBranchOpInterface,
    after: &mut AnalysisStateGuardMut<'_, <A as DenseForwardDataFlowAnalysis>::Lattice>,
    solver: &mut DataFlowSolver,
) where
    A: DenseForwardDataFlowAnalysis,
{
    // Get the terminator predecessors.
    let predecessors = solver.require::<PredecessorState, _>(point, point);
    assert!(predecessors.all_predecessors_known(), "unexpected unresolved region successors");

    let branch_op = branch.as_operation().as_operation_ref();
    for predecessor in predecessors.known_predecessors() {
        let before = if &branch_op == predecessor {
            // If the predecessor is the parent, get the state before the parent.
            solver.require(ProgramPoint::before(*predecessor), point)
        } else {
            // Otherwise, get the state after the terminator.
            solver.require(ProgramPoint::after(*predecessor), point)
        };

        // This function is called in two cases:
        //   1. when visiting the block (point = block start);
        //   2. when visiting the parent operation (point = iter after parent op).
        // In both cases, we are looking for predecessor operations of the point,
        //   1. predecessor may be the terminator of another block from another
        //   region (assuming that the block does belong to another region via an
        //   assertion) or the parent (when parent can transfer control to this
        //   region);
        //   2. predecessor may be the terminator of a block that exits the
        //   region (when region transfers control to the parent) or the operation
        //   before the parent.
        // In the latter case, just perform the join as it isn't the control flow
        // affected by the region.
        let region_from = if &branch_op == predecessor {
            None
        } else {
            predecessor.borrow().parent_region()
        };
        if point.is_at_block_start() {
            let region_to = point.block().unwrap().parent().unwrap();
            analysis.visit_region_branch_control_flow_transfer(
                branch,
                region_from,
                Some(region_to),
                &before,
                after,
                solver,
            );
        } else {
            assert_eq!(
                point.prev_operation().unwrap(),
                branch_op,
                "expected to be visiting the branch itself"
            );
            // Only need to call the arc transfer when the predecessor is the region or the op
            // itself, not the previous op.
            let parent_op = predecessor.borrow().parent_op().unwrap();
            if parent_op == branch_op || predecessor == &branch_op {
                analysis.visit_region_branch_control_flow_transfer(
                    branch,
                    region_from,
                    None,
                    &before,
                    after,
                    solver,
                );
            } else {
                after.join(before.lattice());
            }
        }
    }
}
