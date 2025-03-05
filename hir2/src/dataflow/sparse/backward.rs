use bitvec::bitvec;
use smallvec::SmallVec;

use super::{SparseDataFlowAnalysis, SparseLattice};
use crate::{
    dataflow::{
        analyses::dce::{Executable, PredecessorState},
        AnalysisStateGuard, AnalysisStateGuardMut, Backward, BuildableAnalysisState,
        DataFlowSolver, ProgramPoint,
    },
    traits::{BranchOpInterface, ReturnLike},
    AttributeValue, CallOpInterface, CallableOpInterface, EntityWithId, OpOperandImpl,
    OpOperandRange, OpResultRange, Operation, OperationRef, RegionBranchOpInterface,
    RegionBranchTerminatorOpInterface, RegionSuccessorIter, Report, StorableEntity,
    SuccessorOperands, ValueRef,
};

/// A sparse (backward) data-flow analysis for propagating SSA value lattices backwards across the
/// IR by implementing transfer functions for operations.
///
/// Visiting a program point in sparse backward data-flow analysis will invoke the transfer function
/// of the operation preceding the program point. Visiting a program point at the begining of block
/// will visit the block itself.
#[allow(unused_variables)]
pub trait SparseBackwardDataFlowAnalysis: 'static {
    type Lattice: BuildableAnalysisState + SparseLattice;

    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    /// The operation transfer function.
    ///
    /// Given the result lattices, this function is expected to set the operand lattices.
    fn visit_operation(
        &self,
        op: &Operation,
        operands: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        results: &[AnalysisStateGuard<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report>;

    /// The transfer function for calls to external functions.
    ///
    /// This function is expected to set lattice values of the call operands. By default, this calls
    /// [visit_call_operand] for all operands.
    fn visit_external_call(
        &self,
        call: &dyn CallOpInterface,
        arguments: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        results: &[AnalysisStateGuard<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) {
        for operand in call.arguments() {
            let operand = operand.borrow();
            self.visit_call_operand(&operand, solver);
        }
    }

    /// Visit operands on branch instructions that are not forwarded.
    fn visit_branch_operand(&self, operand: &OpOperandImpl, solver: &mut DataFlowSolver);

    /// Visit operands on call instructions that are not forwarded.
    fn visit_call_operand(&self, operand: &OpOperandImpl, solver: &mut DataFlowSolver);

    /// Set the given lattice element(s) at control flow exit point(s).
    fn set_to_exit_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>);
}

pub fn set_all_to_exit_states<A>(
    analysis: &A,
    lattices: &mut [AnalysisStateGuardMut<'_, <A as SparseBackwardDataFlowAnalysis>::Lattice>],
) where
    A: SparseBackwardDataFlowAnalysis,
{
    for lattice in lattices {
        analysis.set_to_exit_state(lattice);
    }
}

/// Recursively initialize the analysis on nested operations and blocks.
pub(super) fn initialize_recursively<A>(
    analysis: &SparseDataFlowAnalysis<A, Backward>,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: SparseBackwardDataFlowAnalysis,
{
    log::trace!("initializing op recursively");
    visit_operation(analysis, op, solver)?;

    for region in op.regions() {
        for block in region.body() {
            log::trace!("initializing analysis for block {}", block.id());
            {
                let state =
                    solver.get_or_create::<Executable, _>(ProgramPoint::at_start_of(&*block));
                AnalysisStateGuard::subscribe(&state, analysis);
            }

            // Initialize ops in reverse order, so we can do as much initial propagation as possible
            // without having to go through the solver queue.
            let mut ops = block.body().back();
            log::trace!("initializing ops of {} bottom-up", block.id());
            while let Some(op) = ops.as_pointer() {
                ops.move_prev();

                let op = op.borrow();
                initialize_recursively(analysis, &op, solver)?;
            }
            log::trace!("all ops of {} have been initialized", block.id());
        }
    }

    Ok(())
}

/// Visit an operation. If this is a call operation or an operation with
/// region control-flow, then its operand lattices are set accordingly.
/// Otherwise, the operation transfer function is invoked.
pub(super) fn visit_operation<A>(
    analysis: &SparseDataFlowAnalysis<A, Backward>,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: SparseBackwardDataFlowAnalysis,
{
    // If we're in a dead block, bail out.
    let in_dead_block = op.parent().is_some_and(|block| {
        !solver
            .get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block))
            .is_live()
    });
    if in_dead_block {
        log::trace!("skipping analysis for op in dead/non-executable block: {op}");
        return Ok(());
    }

    let current_point = ProgramPoint::after(op);
    let mut operands = get_lattice_elements_mut::<A>(op.operands().all(), solver);
    let results = get_lattice_elements_for::<A>(current_point, op.results().all(), solver);

    // Block arguments of region branch operations flow back into the operands of the parent op
    if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
        log::trace!("op implements RegionBranchOpInterface - handling as special case");
        visit_region_successors(analysis, branch, solver);
        return Ok(());
    }

    // Block arguments of successor blocks flow back into our operands.
    if let Some(branch) = op.as_trait::<dyn BranchOpInterface>() {
        log::trace!("op implements BranchOpInterface - handling as special case");

        // We remember all operands not forwarded to any block in a bitvector.
        // We can't just cut out a range here, since the non-forwarded ops might be non-contiguous
        // (if there's more than one successor).
        let mut unaccounted = bitvec![1; op.num_operands()];

        for successor_index in 0..branch.num_successors() {
            let successor_operands = branch.get_successor_operands(successor_index);
            let forwarded = successor_operands.forwarded();
            if !forwarded.is_empty() {
                let num_produced = successor_operands.num_produced();
                for (operand_index, operand) in forwarded.iter().enumerate() {
                    unaccounted.set(operand.index(), false);
                    if let Some(block_arg) =
                        branch.get_successor_block_argument(operand_index + num_produced)
                    {
                        let mut operand_lattice =
                            get_lattice_element_mut::<A>(operand.borrow().as_value_ref(), solver);
                        let result_lattice = get_lattice_element_for::<A>(
                            current_point,
                            block_arg.borrow().as_value_ref(),
                            solver,
                        );
                        operand_lattice.meet(result_lattice.lattice());
                    }
                }
            }
        }

        // Operands not forwarded to successor blocks are typically parameters of the branch
        // operation itself (for example the boolean for if/else).
        for index in unaccounted.iter_ones() {
            let operand = op.operands().all()[index].borrow();
            analysis.visit_branch_operand(&operand, solver);
        }

        return Ok(());
    }

    // For function calls, connect the arguments of the entry blocks to the operands of the call op
    // that are forwarded to these arguments.
    if let Some(call) = op.as_trait::<dyn CallOpInterface>() {
        log::trace!("op implements CallOpInterface - handling as special case");

        // TODO: resolve_in_symbol_table
        if let Some(callable_symbol) = call.resolve() {
            let callable_symbol = callable_symbol.borrow();
            log::trace!("resolved callee as {}", callable_symbol.name());
            let callable_op = callable_symbol.as_symbol_operation();
            if let Some(callable) = callable_op.as_trait::<dyn CallableOpInterface>() {
                log::trace!("{} implements CallableOpInterface", callable_symbol.name());
                // Not all operands of a call op forward to arguments. Such operands are stored in
                // `unaccounted`.
                let mut unaccounted = bitvec![1; op.num_operands()];

                // If the call invokes an external function (or a function treated as external due to
                // config), defer to the corresponding extension hook. By default, it just does
                // `visit_call_operand` for all operands.
                let arg_operands = call.arguments();
                let region = callable.get_callable_region();
                if region.as_ref().is_none_or(|region| {
                    region.borrow().is_empty() || !solver.config().is_interprocedural()
                }) {
                    log::trace!("{} is an external callee", callable_symbol.name());
                    analysis.visit_external_call(call, &mut operands, &results, solver);
                    return Ok(());
                }

                // Otherwise, propagate information from the entry point of the function back to
                // operands whenever possible.
                log::trace!("propagating value lattices from callee entry to call operands");
                let region = region.unwrap();
                let region = region.borrow();
                let block = region.entry();
                for (block_arg, arg_operand) in block.arguments().iter().zip(arg_operands.iter()) {
                    let mut arg_lattice =
                        get_lattice_element_mut::<A>(arg_operand.borrow().as_value_ref(), solver);
                    let result_lattice = get_lattice_element_for::<A>(
                        current_point,
                        block_arg.borrow().as_value_ref(),
                        solver,
                    );
                    arg_lattice.meet(result_lattice.lattice());
                    unaccounted.set(arg_operand.borrow().index as usize, false);
                }

                // Handle the operands of the call op that aren't forwarded to any arguments.
                for index in unaccounted.iter_ones() {
                    let operand = op.operands().all()[index].borrow();
                    analysis.visit_call_operand(&operand, solver);
                }

                return Ok(());
            }
        }
    }

    // When the region of an op implementing `RegionBranchOpInterface` has a terminator implementing
    // `RegionBranchTerminatorOpInterface` or a return-like terminator, the region's successors'
    // arguments flow back into the "successor operands" of this terminator.
    //
    // A successor operand with respect to an op implementing `RegionBranchOpInterface` is an
    // operand that is forwarded to a region successor's input. There are two types of successor
    // operands: the operands of this op itself and the operands of the terminators of the regions
    // of this op.
    if let Some(terminator) = op.as_trait::<dyn RegionBranchTerminatorOpInterface>() {
        log::trace!("op implements RegionBranchTerminatorOpInterface");
        let parent_op = op.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        if let Some(branch) = parent_op.as_trait::<dyn RegionBranchOpInterface>() {
            log::trace!(
                "op's parent implements RegionBranchOpInterface - handling as special case"
            );
            visit_region_successors_from_terminator(analysis, terminator, branch, solver);
            return Ok(());
        }
    }

    if op.implements::<dyn ReturnLike>() {
        log::trace!("op implements ReturnLike");
        // Going backwards, the operands of the return are derived from the results of all CallOps
        // calling this CallableOp.
        let parent_op = op.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        if let Some(callable) = parent_op.as_trait::<dyn CallableOpInterface>() {
            log::trace!("op's parent implements CallableOpInterface - visiting call sites");
            let callsites = solver.require::<PredecessorState, _>(
                ProgramPoint::after(callable.as_operation()),
                current_point,
            );
            if callsites.all_predecessors_known() {
                log::trace!(
                    "found all {} call sites of the current callable op",
                    callsites.known_predecessors().len()
                );
                log::trace!("meeting lattices of return values and call site results");
                for call in callsites.known_predecessors() {
                    let call = call.borrow();
                    let call_result_lattices =
                        get_lattice_elements_for::<A>(current_point, call.results().all(), solver);
                    for (op, result) in operands.iter_mut().zip(call_result_lattices.into_iter()) {
                        op.meet(result.lattice());
                    }
                }
            } else {
                // If we don't know all the callers, we can't know where the returned values go.
                // Note that, in particular, this will trigger for the return ops of any public
                // functions.
                log::trace!(
                    "not all call sites are known - setting return value lattices to exit state"
                );
                set_all_to_exit_states(analysis, &mut operands);
            }
            return Ok(());
        }
    }

    log::trace!("invoking {}::visit_operation", core::any::type_name::<A>());
    analysis.visit_operation(op, &mut operands, &results, solver)
}

/// Visit an op with regions (like e.g. `scf.while`)
fn visit_region_successors<A>(
    analysis: &SparseDataFlowAnalysis<A, Backward>,
    branch: &dyn RegionBranchOpInterface,
    solver: &mut DataFlowSolver,
) where
    A: SparseBackwardDataFlowAnalysis,
{
    let op = branch.as_operation();

    let mut const_operands =
        SmallVec::<[Option<Box<dyn AttributeValue>>; 2]>::with_capacity(op.num_operands());
    const_operands.resize_with(op.num_operands(), || None);
    let successors = branch.get_entry_successor_regions(&const_operands);

    // All operands not forwarded to any successor. This set can be non-contiguous in the presence
    // of multiple successors.
    let mut unaccounted = bitvec![1; op.num_operands()];

    for successor in successors {
        let operands = branch.get_entry_successor_operands(*successor.branch_point());
        let inputs = successor.successor_inputs();
        for (operand, input) in operands.forwarded().iter().zip(inputs.iter()) {
            let operand = operand.borrow();
            let operand_index = operand.index as usize;
            let mut operand_lattice = get_lattice_element_mut::<A>(operand.as_value_ref(), solver);
            let point = ProgramPoint::after(op);
            let input_lattice = get_lattice_element_for::<A>(point, input, solver);
            operand_lattice.meet(input_lattice.lattice());
            unaccounted.set(operand_index, false);
        }
    }

    // All operands not forwarded to regions are typically parameters of the branch operation itself
    // (for example the boolean for if/else).
    for index in unaccounted.iter_ones() {
        analysis.visit_branch_operand(&op.operands().all()[index].borrow(), solver);
    }
}

/// Visit a `RegionBranchTerminatorOpInterface` to compute the lattice values
/// of its operands, given its parent op `branch`. The lattice value of an
/// operand is determined based on the corresponding arguments in
/// `terminator`'s region successor(s).
fn visit_region_successors_from_terminator<A>(
    analysis: &SparseDataFlowAnalysis<A, Backward>,
    terminator: &dyn RegionBranchTerminatorOpInterface,
    branch: &dyn RegionBranchOpInterface,
    solver: &mut DataFlowSolver,
) where
    A: SparseBackwardDataFlowAnalysis,
{
    assert!(
        OperationRef::ptr_eq(
            &terminator.as_operation().parent_op().unwrap(),
            &branch.as_operation().as_operation_ref()
        ),
        "expected `branch` to be the parent op of `terminator`"
    );

    let num_operands = terminator.num_operands();
    let mut const_operands =
        SmallVec::<[Option<Box<dyn AttributeValue>>; 2]>::with_capacity(num_operands);
    const_operands.resize_with(num_operands, || None);

    let terminator_op = terminator.as_operation();
    let successors = terminator.get_successor_regions(&const_operands);
    let successors = RegionSuccessorIter::new(terminator_op, successors);

    // All operands not forwarded to any successor. This set can be non-contiguous in the presence
    // of multiple successors.
    let mut unaccounted = bitvec![1; num_operands];

    for successor in successors {
        let inputs = successor.successor_inputs();
        let operands = terminator.get_successor_operands(*successor.branch_point());
        for (operand, input) in operands.forwarded().iter().zip(inputs.iter()) {
            let operand = operand.borrow();
            let mut operand_lattice = get_lattice_element_mut::<A>(operand.as_value_ref(), solver);
            let point = ProgramPoint::after(terminator_op);
            let input_lattice = get_lattice_element_for::<A>(point, input, solver);
            operand_lattice.meet(input_lattice.lattice());
            unaccounted.set(operand.index(), false);
        }
    }

    // Visit operands of the branch op not forwarded to the next region. (Like e.g. the boolean of
    // `scf.conditional`)
    for index in unaccounted.iter_ones() {
        analysis.visit_branch_operand(&terminator_op.operands()[index].borrow(), solver);
    }
}

#[inline]
fn get_lattice_element_mut<'guard, A>(
    value: ValueRef,
    solver: &mut DataFlowSolver,
) -> AnalysisStateGuardMut<'guard, <A as SparseBackwardDataFlowAnalysis>::Lattice>
where
    A: SparseBackwardDataFlowAnalysis,
{
    log::trace!("getting lattice for {value}");
    solver.get_or_create_mut::<_, _>(value)
}

#[inline]
fn get_lattice_element_for<'guard, A>(
    point: ProgramPoint,
    value: ValueRef,
    solver: &mut DataFlowSolver,
) -> AnalysisStateGuard<'guard, <A as SparseBackwardDataFlowAnalysis>::Lattice>
where
    A: SparseBackwardDataFlowAnalysis,
{
    log::trace!("getting lattice for {value} at {point}");
    solver.require::<_, _>(value, point)
}

fn get_lattice_elements_mut<'guard, A>(
    values: OpOperandRange<'_>,
    solver: &mut DataFlowSolver,
) -> SmallVec<[AnalysisStateGuardMut<'guard, <A as SparseBackwardDataFlowAnalysis>::Lattice>; 2]>
where
    A: SparseBackwardDataFlowAnalysis,
{
    log::trace!("getting lattices for {:#?}", values.as_slice());
    let mut results = SmallVec::with_capacity(values.len());
    for value in values.iter() {
        let lattice = solver.get_or_create_mut::<_, _>(value.borrow().as_value_ref());
        results.push(lattice);
    }
    results
}

/// Get the lattice elements for a range of values, and also set up dependencies so that the
/// analysis on the given ProgramPoint is re-invoked if any of the values change.
fn get_lattice_elements_for<'guard, A>(
    point: ProgramPoint,
    values: OpResultRange<'_>,
    solver: &mut DataFlowSolver,
) -> SmallVec<[AnalysisStateGuard<'guard, <A as SparseBackwardDataFlowAnalysis>::Lattice>; 2]>
where
    A: SparseBackwardDataFlowAnalysis,
{
    log::trace!("getting lattices for {:#?}", values.as_slice());
    let mut results = SmallVec::with_capacity(values.len());
    for value in values.iter() {
        let lattice = solver.require(value.borrow().as_value_ref(), point);
        results.push(lattice);
    }
    results
}
