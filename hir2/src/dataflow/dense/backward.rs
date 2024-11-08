use super::*;
use crate::{
    cfg::Graph,
    dataflow::{
        analyses::dce::{CfgEdge, Executable, PredecessorState},
        AnalysisStateGuard, BuildableAnalysisState, CallControlFlowAction, DataFlowSolver,
        ProgramPoint,
    },
    Block, CallOpInterface, CallableOpInterface, Operation, RegionBranchOpInterface,
    RegionBranchPoint, RegionBranchTerminatorOpInterface, RegionRef, Report, Spanned, SymbolTable,
};

/// The base trait for all dense backward data-flow analyses.
///
/// This type of dense data-flow analysis attaches a lattice to program points and implements a
/// transfer function from the lattice after each operation to the lattice before it, thus
/// propagating backwards.
///
/// Visiting a program point will invoke the transfer function of the operation following the
/// program point iterator. Visiting a program point at the end of a block will visit the block
/// itself.
///
/// Implementations of this analysis are expected to be constructed with a symbol table collection
/// that is used to cache symbol resolution during interprocedural analysis. This table can be
/// empty.
#[allow(unused_variables)]
pub trait DenseBackwardDataFlowAnalysis: 'static {
    type Lattice: BuildableAnalysisState + DenseLattice;

    /// Get the symbol table to use for caching symbol resolution during interprocedural analysis.
    ///
    /// If `None`, no caching is performed, and the symbol table is dynamically looked up from
    /// the current operation being analyzed.
    fn symbol_table(&self) -> Option<&dyn SymbolTable>;

    /// The transfer function.
    ///
    /// Visits an operation with the dense lattice state as computed after it. Implementations are
    /// expected to compute/update the state of the lattice before the op.
    fn visit_operation(
        &self,
        op: &Operation,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report>;

    /// Set the dense lattice before the control flow exit point and propagate an update if it
    /// changed.
    fn set_to_exit_state(
        &self,
        lattice: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    );

    /// Propagate the dense lattice backward along the control flow edge represented by `from` and
    /// `to`, which is known to be the result of intra-region control flow, i.e. via
    /// [BranchOpInterface]. This is invoked when visiting blocks, rather than the terminators of
    /// those blocks.
    ///
    /// The default implementation just invokes `meet` on the states, meaning that operations
    /// implementing [BranchOpInterface] don't have any effect on the lattice that isn't already
    /// expressed by the interface itself.
    ///
    /// The lattices are as follows:
    ///
    /// * `after` is the lattice at the beginning of `to`
    /// * `before` is the lattice at the end of `from`
    ///
    /// By default, the `before` state is met with the `after` state. Implementations can override
    /// this in certain cases. Specifically, if the edge itself should be taken into account in
    /// some way, such as if there are subtleties in the transfer function due to edge weights or
    /// other control flow considerations. For example, one might wish to take into account the
    /// fact that an edge enters or exits a loop.
    fn visit_branch_control_flow_transfer(
        &self,
        from: &Block,
        to: BlockRef,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        before.meet(after.lattice());
    }

    /// Propagate the dense lattice backwards along the control flow edge from `region_from` to
    /// `region_to` regions of the `branch` operation. If set to `None`, this corresponds to control
    /// flow branches originating at, or targeting, the `branch` operation itself. The default
    /// implementation just invokes `meet` on the states, meaning that operations implementing
    /// [RegionBranchOpInterface] don't have any effect on the lattice that isn't already expressed
    /// by the interface itself.
    ///
    /// The lattices are as follows:
    ///
    /// * `after`:
    ///   - If `region_to` is set, this is the lattice at the beginning of the entry block of that
    ///     region.
    ///   - Otherwise, this is the lattice after the parent op.
    /// * `before`:
    ///   - If `region_from` is set, this is the lattice at the end of the block that exits the
    ///     region. Note that for multi-exit regions, the lattices are equal at the end of all
    ///     exiting blocks, but they are associated with different program points.
    ///   - Otherwise, this is the lattice before the parent op.
    ///
    /// By default, the `before` state is met with the `after` state. Implementations can override
    /// this in certain cases. Specifically, if the `branch` op may affect the lattice before
    /// entering any region, the implementation can handle `region_from.is_none()`. If the `branch`
    /// op may affect the lattice after all terminated, the implementation can handle
    /// `region_to.is_none()`. Additional refinements are possible based on specific pairs of
    /// `region_from` and `region_to`.
    fn visit_region_branch_control_flow_transfer(
        &self,
        branch: &dyn RegionBranchOpInterface,
        region_from: Option<RegionRef>,
        region_to: Option<RegionRef>,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        before.meet(after.lattice());
    }

    /// Propagate the dense lattice backwards along the call control flow edge, which can be either
    /// entering or exiting the callee.
    ///
    /// The default implementation for enter and exit callee action just invokes `meet` on the
    /// states, meaning that operations implementing [CallOpInterface] don't have any effect on the
    /// lattice that isn't already expressed by the interface itself. The default implementation for
    /// external callee action additionally sets the result to the exit (fixpoint) state.
    ///
    /// Three types of back-propagation are possible here:
    ///
    /// * `CallControlFlowAction::External` indicates that:
    ///   - `after` is the state following the call operation
    ///   - `before` is the state before the call operation
    /// * `CallControlFlowAction::Enter` indicates that:
    ///   - `after` is the state at the top of the callee entry block
    ///   - `before` is the state before the call operation
    /// * `CallControlFlowAction::Exit` indicates that:
    ///   - `after` is the state after the call operation
    ///   - `before` is the state of exit blocks of the callee
    ///
    /// By default, the `before` state is simply met with the `after` state. Implementations may
    /// be interested in overriding this in some circumstances. Specifically, if the `call` op
    /// may affect the lattice prior to entering the callee, a custom implementation can be added
    /// for `CallControlFlowAction::Enter`. If the `call` op may affect the lattice post-exiting
    /// the callee, the implementation can handle `CallControlFlowAction::Exit`
    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        before.meet(after.lattice());
        // Note that `set_to_exit_state` may be a "partial fixpoint" for some
        // lattices, e.g., lattices that are lists of maps of other lattices will
        // only set fixpoint for "known" lattices.
        if matches!(action, CallControlFlowAction::External) {
            self.set_to_exit_state(before, solver);
        }
    }
}

/// Visit an operation.
///
/// Dispatches to specialized methods for call or region control-flow operations. Otherwise, this
/// function invokes the operation transfer function.
pub fn process_operation<A>(
    analysis: &A,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: DenseBackwardDataFlowAnalysis,
{
    let point = ProgramPoint::before(op);
    // If the containing block is not executable, bail out.
    if op.parent().is_some_and(|block| {
        !solver
            .require::<Executable, _>(ProgramPoint::at_start_of(block), point)
            .is_live()
    }) {
        return Ok(());
    }

    // Get the dense lattice to update.
    let mut before = solver.get_or_create_mut(point);

    // Get the dense state after execution of this op.
    let after = solver.require(ProgramPoint::after(op), point);

    // Special cases where control flow may dictate data flow.
    if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
        visit_region_branch_operation(
            analysis,
            point,
            branch,
            RegionBranchPoint::Parent,
            &mut before,
            solver,
        );
        return Ok(());
    }

    if let Some(call) = op.as_trait::<dyn CallOpInterface>() {
        visit_call_operation(analysis, call, &after, &mut before, solver);
        return Ok(());
    }

    // Invoke the operation transfer function.
    analysis.visit_operation(op, &after, &mut before, solver)
}

/// Visit a block. The state at the end of the block is propagated from control-flow successors of
/// the block or callsites.
pub fn visit_block<A>(analysis: &A, block: &Block, solver: &mut DataFlowSolver)
where
    A: DenseBackwardDataFlowAnalysis,
{
    let point = ProgramPoint::at_end_of(block);
    // If the block is not executable, bail out.
    if !solver
        .require::<Executable, _>(ProgramPoint::at_start_of(block), point)
        .is_live()
    {
        return;
    }

    let mut before = solver.get_or_create_mut(point);

    // We need "exit" blocks, i.e. the blocks that may return control to the parent operation.
    let is_region_exit_block = |block: &Block| {
        match block.terminator() {
            // Treat empty and terminator-less blocks as exit blocks.
            None => true,
            // There may be a weird case where a terminator may be transferring control either to
            // the parent or to another block, so exit blocks and successors are not mutually
            // exclusive.
            Some(op) => op.borrow().implements::<dyn RegionBranchTerminatorOpInterface>(),
        }
    };

    if is_region_exit_block(block) {
        // If this block is exiting from a callable, the successors of exiting from a callable are
        // the successors of all call sites. And the call sites themselves are predecessors of the
        // callable.
        let parent_op = block.parent_op().expect("orphaned block");
        let region = block.parent().unwrap();
        if let Some(callable) = parent_op.borrow().as_trait::<dyn CallableOpInterface>() {
            let callable_region = callable.get_callable_region();
            if callable_region.is_some_and(|r| r == region) {
                let callsites = solver.require::<PredecessorState, _>(
                    ProgramPoint::after(callable.as_operation()),
                    point,
                );
                // If not all call sites are known, conservative mark all lattices as
                // having reached their pessimistic fix points.
                if !callsites.all_predecessors_known() || !solver.config().is_interprocedural() {
                    return analysis.set_to_exit_state(&mut before, solver);
                }

                for callsite in callsites.known_predecessors() {
                    let call = callsite.borrow();
                    let call = call.as_trait::<dyn CallOpInterface>().expect("invalid callsite");
                    let after = solver.require(ProgramPoint::after(*callsite), point);
                    analysis.visit_call_control_flow_transfer(
                        call,
                        CallControlFlowAction::Exit,
                        &after,
                        &mut before,
                        solver,
                    );
                }

                return;
            }
        }

        // If this block is exiting from an operation with region-based control flow, propagate the
        // lattice back along the control flow edge.
        if let Some(branch) = parent_op.borrow().as_trait::<dyn RegionBranchOpInterface>() {
            return visit_region_branch_operation(
                analysis,
                point,
                branch,
                RegionBranchPoint::Child(region),
                &mut before,
                solver,
            );
        }

        // Cannot reason about successors of an exit block, set the pessimistic fixpoint.
        return analysis.set_to_exit_state(&mut before, solver);
    }

    // Meet the state with the state before block's successors.
    for successor in Block::children(block.as_block_ref()) {
        if !solver
            .require::<Executable, _>(
                CfgEdge::new(block.as_block_ref(), successor, block.span()),
                point,
            )
            .is_live()
        {
            continue;
        }

        // Merge in the state from the successor: either the first operation, or the block itself
        // when empty.
        let after = solver.require(ProgramPoint::before(successor), point);
        analysis.visit_branch_control_flow_transfer(block, successor, &after, &mut before, solver);
    }
}

/// Visit an operation for which the data flow is described by the `CallOpInterface`. Performs
/// inter-procedural data flow as follows:
///
/// * Find the callable (resolve via the symbol table)
/// * If the solver is not configured for inter-procedural analysis, or the callable op is just a
///   declaration, then invoke the `visit_call_control_flow_transfer` callback of the analysis, to
///   let it decide how to proceed. This can work just like `visit_operation` for some analyses.
/// * If the solver is configured for inter-procedural analysis, and the callable op is a definition
///   then `after` is set to the lattice state of the entry block of the callable region, and then
///   the `visit_call_control_flow_transfer` callback is invoked.
/// * Lastly, if the callable op is not resolvable:
///   * If configured for inter-procedural analysis, then `set_to_exit_state` is called on the
///     `before` lattice. This is because we expected to perform an analyis taking into account
///     the state of the callee, but it was not available, so we cannot assume anything.
///   * If _not_ configured for inter-procedural analysis, then `visit_call_control_flow_transfer`
///     is invoked, so that the analysis implementation can decide how to proceed.
pub fn visit_call_operation<A>(
    analysis: &A,
    call: &dyn CallOpInterface,
    after: &<A as DenseBackwardDataFlowAnalysis>::Lattice,
    before: &mut AnalysisStateGuard<'_, <A as DenseBackwardDataFlowAnalysis>::Lattice>,
    solver: &mut DataFlowSolver,
) where
    A: DenseBackwardDataFlowAnalysis,
{
    // Find the callee.
    let callee = match analysis.symbol_table() {
        None => call.resolve(),
        Some(cache) => call.resolve_in_symbol_table(cache),
    };

    let callee_ref = callee.as_ref().map(|callee| callee.borrow());
    let callable = match callee_ref.as_ref() {
        None => None,
        Some(callee) => callee.as_symbol_operation().as_trait::<dyn CallableOpInterface>(),
    };

    // No region means the callee is only declared in this module. If that is the case or if the
    // solver is not interprocedural, let the hook handle it.
    let is_declaration =
        callable.is_some_and(|c| c.get_callable_region().is_none_or(|cr| cr.borrow().is_empty()));
    let is_interprocedural = solver.config().is_interprocedural();
    if is_declaration || !is_interprocedural {
        return analysis.visit_call_control_flow_transfer(
            call,
            CallControlFlowAction::External,
            after,
            before,
            solver,
        );
    }

    if let Some(callable) = callable {
        // Call-level control flow specifies the data flow here.
        //
        //   func.func @callee() {
        //     ^calleeEntryBlock:
        //     // latticeAtCalleeEntry
        //     ...
        //   }
        //   func.func @caller() {
        //     ...
        //     // latticeBeforeCall
        //     call @callee
        //     ...
        //   }
        let region = callable.get_callable_region().unwrap().borrow();
        let callee_entry_block = region.entry();
        let callee_entry = ProgramPoint::at_start_of(&*callee_entry_block);
        let lattice_at_callee_entry =
            solver.require(callee_entry, ProgramPoint::before(call.as_operation()));
        let lattice_before_call = &mut *before;
        analysis.visit_call_control_flow_transfer(
            call,
            CallControlFlowAction::Enter,
            &lattice_at_callee_entry,
            lattice_before_call,
            solver,
        );
    } else {
        analysis.set_to_exit_state(before, solver);
    }
}

/// Visit a program point within a region branch operation with successors (from which the state is
/// propagated) in or after it.
pub fn visit_region_branch_operation<A>(
    analysis: &A,
    point: ProgramPoint,
    branch: &dyn RegionBranchOpInterface,
    branch_point: RegionBranchPoint,
    before: &mut AnalysisStateGuard<'_, <A as DenseBackwardDataFlowAnalysis>::Lattice>,
    solver: &mut DataFlowSolver,
) where
    A: DenseBackwardDataFlowAnalysis,
{
    // The successors of the operation may be either the first operation of the entry block of each
    // possible successor region, or the next operation when the branch is a successor of itself.
    for successor in branch.get_successor_regions(branch_point.clone()) {
        let successor_region = successor.successor();
        let after = match successor_region.as_ref() {
            None => {
                // The successor is `branch` itself
                solver.require(ProgramPoint::after(branch.as_operation()), point)
            }
            Some(region) => {
                // The successor is a region of `branch`
                let block =
                    region.borrow().entry_block_ref().expect("unexpected empty successor region");
                if !solver
                    .require::<Executable, _>(ProgramPoint::at_start_of(block), point)
                    .is_live()
                {
                    continue;
                }
                solver.require(ProgramPoint::at_start_of(block), point)
            }
        };

        let region_from = branch_point.region();
        analysis.visit_region_branch_control_flow_transfer(
            branch,
            region_from,
            successor_region,
            &after,
            before,
            solver,
        );
    }
}
