use smallvec::SmallVec;

use super::SparseLattice;
use crate::{
    dataflow::{
        analyses::dce::{CfgEdge, Executable, PredecessorState},
        AnalysisStateGuard, BuildableAnalysisState, DataFlowSolver, ProgramPoint,
    },
    traits::BranchOpInterface,
    Block, BlockArgument, BlockArgumentRange, CallOpInterface, CallableOpInterface, EntityRef,
    OpOperandRange, OpResult, OpResultRange, Operation, OperationRef, RegionBranchOpInterface,
    RegionBranchPoint, RegionBranchTerminatorOpInterface, RegionSuccessor, Report, Spanned,
    StorableEntity, SuccessorOperands, ValueRef,
};

/// The base trait for sparse forward data-flow analyses.
///
/// A sparse analysis implements a transfer function on operations from the lattices of the operands
/// to the lattices of the results. This analysis will propagate lattices across control-flow edges
/// and the callgraph using liveness information.
///
/// Visiting a program point in sparse forward data-flow analysis will invoke the transfer function
/// of the operation preceding the program point. Visiting a program point at the begining of block
/// will visit the block itself.
#[allow(unused_variables)]
pub trait SparseForwardDataFlowAnalysis: 'static {
    type Lattice: BuildableAnalysisState + SparseLattice;

    /// The operation transfer function.
    ///
    /// Given the operand lattices, this function is expected to set the result lattices.
    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[EntityRef<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuard<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report>;

    /// The transfer function for calls to external functions.
    fn visit_external_call(
        &self,
        call: &dyn CallOpInterface,
        arguments: &[EntityRef<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuard<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) {
        set_all_to_entry_states(self, results);
    }

    /// Given an operation with region control-flow, the lattices of the operands, and a region
    /// successor, compute the lattice values for block arguments that are not accounted for by the
    /// branching control flow (ex. the bounds of loops).
    ///
    /// By default, this method marks all such lattice elements as having reached a pessimistic
    /// fixpoint.
    ///
    /// `first_index` is the index of the first element of `arguments` that is set by control-flow.
    fn visit_non_control_flow_arguments(
        &self,
        op: &Operation,
        successor: &RegionSuccessor<'_>,
        arguments: &mut [AnalysisStateGuard<'_, Self::Lattice>],
        first_index: usize,
        solver: &mut DataFlowSolver,
    ) {
        let (leading, rest) = arguments.split_at_mut(first_index);
        let (_, trailing) = rest.split_at_mut(successor.successor_inputs().len());
        set_all_to_entry_states(self, leading);
        set_all_to_entry_states(self, trailing);
    }

    /// Set the given lattice element(s) at control flow entry point(s).
    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuard<'_, Self::Lattice>);
}

pub fn set_all_to_entry_states<A>(
    analysis: &A,
    lattices: &mut [AnalysisStateGuard<'_, <A as SparseForwardDataFlowAnalysis>::Lattice>],
) where
    A: ?Sized + SparseForwardDataFlowAnalysis,
{
    for lattice in lattices {
        analysis.set_to_entry_state(lattice);
    }
}

/// Recursively initialize the analysis on nested operations and blocks.
pub(super) fn initialize_recursively<A>(
    analysis: &A,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: SparseForwardDataFlowAnalysis,
{
    // Initialize the analysis by visiting every owner of an SSA value (all operations and blocks).
    visit_operation(analysis, op, solver)?;

    let current_analysis = solver.current_analysis().unwrap();
    for region in op.regions() {
        for block in region.body() {
            {
                let mut exec = solver.get_or_create_mut::<Executable, _>(block.as_block_ref());
                AnalysisStateGuard::subscribe_nonnull(&mut exec, current_analysis);
            }

            visit_block(analysis, &block, solver);

            for op in block.body() {
                initialize_recursively(analysis, &op, solver)?;
            }
        }
    }

    Ok(())
}

/// Visit an operation. If this is a call operation or an operation with
/// region control-flow, then its result lattices are set accordingly.
/// Otherwise, the operation transfer function is invoked.
pub(super) fn visit_operation<A>(
    analysis: &A,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: SparseForwardDataFlowAnalysis,
{
    // Exit early on operations with no results.
    if !op.has_results() {
        return Ok(());
    }

    // If the containing block is not executable, bail out.
    if op.parent().is_some_and(|block| {
        !solver
            .get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block))
            .is_live()
    }) {
        return Ok(());
    }

    // Get the result lattices.
    let mut result_lattices = get_lattice_elements::<A>(op.results().all(), solver);

    // The results of a region branch operation are determined by control-flow.
    if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
        let point = ProgramPoint::after(op);
        visit_region_successors(
            analysis,
            point,
            branch,
            RegionBranchPoint::Parent,
            &mut result_lattices,
            solver,
        );
        return Ok(());
    }

    // Grab the lattice elements of the operands.
    let current_analysis = solver.current_analysis().unwrap();
    let mut operand_lattices = SmallVec::<[_; 4]>::with_capacity(op.num_operands());
    for operand in op.operands().iter() {
        let mut operand_lattice = get_lattice_element::<A>(operand.borrow().as_value_ref(), solver);
        AnalysisStateGuard::subscribe_nonnull(&mut operand_lattice, current_analysis);
        operand_lattices.push(AnalysisStateGuard::into_entity_ref(operand_lattice));
    }

    if let Some(call) = op.as_trait::<dyn CallOpInterface>() {
        // If the call operation is to an external function, attempt to infer the results from the
        // call arguments.
        //
        // TODO: resolve_in_symbol_table
        let callable = call.resolve();
        let callable = callable.as_ref().map(|c| c.borrow());
        let callable = callable
            .as_ref()
            .and_then(|c| c.as_symbol_operation().as_trait::<dyn CallableOpInterface>());
        if !solver.config().is_interprocedural()
            || callable.is_some_and(|c| c.get_callable_region().is_none())
        {
            analysis.visit_external_call(call, &operand_lattices, &mut result_lattices, solver);
            return Ok(());
        }

        // Otherwise, the results of a call operation are determined by the callgraph.
        let predecessors = solver.require::<PredecessorState, _>(
            ProgramPoint::after(call.as_operation()),
            ProgramPoint::after(op),
        );

        // If not all return sites are known, then conservatively assume we can't reason about the
        //data-flow.
        if !predecessors.all_predecessors_known() {
            set_all_to_entry_states(analysis, &mut result_lattices);
            return Ok(());
        }

        let current_point = ProgramPoint::after(op);
        for predecessor in predecessors.known_predecessors() {
            for (operand, result_lattice) in
                predecessor.borrow().operands().all().iter().zip(result_lattices.iter_mut())
            {
                let operand_lattice = get_lattice_element_for::<A>(
                    current_point,
                    operand.borrow().as_value_ref(),
                    solver,
                );
                result_lattice.join(operand_lattice.lattice());
            }
        }

        return Ok(());
    }

    // Invoke the operation transfer function.
    analysis.visit_operation(op, &operand_lattices, &mut result_lattices, solver)
}

/// Visit a block to compute the lattice values of its arguments. If this is
/// an entry block, then the argument values are determined from the block's
/// "predecessors" as set by `PredecessorState`. The predecessors can be
/// region terminators or callable callsites. Otherwise, the values are
/// determined from block predecessors.
pub(super) fn visit_block<A>(analysis: &A, block: &Block, solver: &mut DataFlowSolver)
where
    A: SparseForwardDataFlowAnalysis,
{
    // Exit early on blocks with no arguments.
    if !block.has_arguments() {
        return;
    }

    // If the block is not executable, bail out.
    if !solver
        .get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block))
        .is_live()
    {
        return;
    }

    // Get the argument lattices.
    let mut arg_lattices = SmallVec::<[_; 4]>::with_capacity(block.num_arguments());
    for argument in block.arguments() {
        let lattice = get_lattice_element::<A>(argument.borrow().as_value_ref(), solver);
        arg_lattices.push(lattice);
    }

    // The argument lattices of entry blocks are set by region control-flow or the callgraph.
    let current_point = ProgramPoint::at_start_of(block);
    if block.is_entry_block() {
        // Check if this block is the entry block of a callable region.
        let parent_op = block.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        let callable = parent_op.as_trait::<dyn CallableOpInterface>();
        if callable.is_some_and(|c| c.get_callable_region() == block.parent()) {
            let callable = callable.unwrap();
            let callsites = solver.require::<PredecessorState, _>(
                ProgramPoint::after(callable.as_operation()),
                current_point,
            );

            // If not all callsites are known, conservatively mark all lattices as having reached
            // their pessimistic fixpoints.
            if !callsites.all_predecessors_known() || !solver.config().is_interprocedural() {
                return set_all_to_entry_states(analysis, &mut arg_lattices);
            }

            for callsite in callsites.known_predecessors() {
                let callsite = callsite.borrow();
                let call = callsite.as_trait::<dyn CallOpInterface>().unwrap();
                for (arg, arg_lattice) in call.arguments().iter().zip(arg_lattices.iter_mut()) {
                    let input = get_lattice_element_for::<A>(
                        current_point,
                        arg.borrow().as_value_ref(),
                        solver,
                    );
                    arg_lattice.join(input.lattice());
                }
            }

            return;
        }

        // Check if the lattices can be determined from region control flow.
        if let Some(branch) = parent_op.as_trait::<dyn RegionBranchOpInterface>() {
            return visit_region_successors(
                analysis,
                current_point,
                branch,
                RegionBranchPoint::Child(block.parent().unwrap()),
                &mut arg_lattices,
                solver,
            );
        }

        // Otherwise, we can't reason about the data-flow.
        let successor = RegionSuccessor::new(
            RegionBranchPoint::Child(block.parent().unwrap()),
            OpOperandRange::empty(),
        );
        return analysis.visit_non_control_flow_arguments(
            &parent_op,
            &successor,
            &mut arg_lattices,
            0,
            solver,
        );
    }

    // Iterate over the predecessors of the non-entry block.
    let current_analysis = solver.current_analysis().unwrap();
    for pred in block.predecessors() {
        let predecessor = pred.block.borrow();

        // If the edge from the predecessor block to the current block is not live, bail out.
        let mut edge_executable = {
            let anchor = solver.create_lattice_anchor(CfgEdge::new(
                predecessor.as_block_ref(),
                block.as_block_ref(),
                predecessor.span(),
            ));
            solver.get_or_create_mut::<Executable, _>(anchor)
        };
        AnalysisStateGuard::subscribe_nonnull(&mut edge_executable, current_analysis);
        if !edge_executable.is_live() {
            continue;
        }

        // Check if we can reason about the data-flow from the predecessor.
        let terminator = predecessor.terminator();
        let terminator = terminator.as_ref().map(|t| t.borrow());
        if let Some(branch) =
            terminator.as_ref().and_then(|t| t.as_trait::<dyn BranchOpInterface>())
        {
            let operands = branch.get_successor_operands(pred.index());
            for (idx, lattice) in arg_lattices.iter_mut().enumerate() {
                if let Some(operand) =
                    operands.get(idx).and_then(|operand| operand.into_value_ref())
                {
                    let operand_lattice =
                        get_lattice_element_for::<A>(current_point, operand, solver);
                    lattice.join(operand_lattice.lattice());
                } else {
                    // Conservatively consider internally produced arguments as entry points.
                    analysis.set_to_entry_state(lattice);
                }
            }
        } else {
            return set_all_to_entry_states(analysis, &mut arg_lattices);
        }
    }
}

/// Visit a program point `point` with predecessors within a region branch
/// operation `branch`, which can either be the entry block of one of the
/// regions or the parent operation itself, and set either the argument or
/// parent result lattices.
fn visit_region_successors<A>(
    analysis: &A,
    point: ProgramPoint,
    branch: &dyn RegionBranchOpInterface,
    successor: RegionBranchPoint,
    lattices: &mut [AnalysisStateGuard<'_, <A as SparseForwardDataFlowAnalysis>::Lattice>],
    solver: &mut DataFlowSolver,
) where
    A: SparseForwardDataFlowAnalysis,
{
    let predecessors = solver.require::<PredecessorState, _>(point, point);
    assert!(predecessors.all_predecessors_known(), "unexpected unresolved region successors");

    for op in predecessors.known_predecessors() {
        let operation = op.borrow();

        // Get the incoming successor operands.
        let mut operands = None;

        // Check if the predecessor is the parent op.
        if core::ptr::addr_eq(OperationRef::as_ptr(op), branch.as_operation()) {
            operands = Some(branch.get_entry_successor_operands(successor.clone()));
        } else if let Some(region_terminator) =
            operation.as_trait::<dyn RegionBranchTerminatorOpInterface>()
        {
            // Otherwise, try to deduce the operands from a region return-like op.
            operands = Some(region_terminator.get_successor_operands(successor.clone()));
        }

        let Some(operands) = operands else {
            // We can't reason about the data-flow
            return set_all_to_entry_states(analysis, lattices);
        };

        let inputs = predecessors.successor_inputs(op);
        assert_eq!(
            inputs.len(),
            operands.len(),
            "expected the same number of successor inputs as operands"
        );

        let mut first_index = 0;
        if inputs.len() != lattices.len() {
            if !point.is_at_block_start() {
                if !inputs.is_empty() {
                    let input = inputs[0].borrow();
                    first_index = input.downcast_ref::<OpResult>().unwrap().index();
                }
                let results = branch.results().all();
                let results = OpResultRange::new(
                    first_index..(first_index + inputs.len()),
                    results.as_slice(),
                );
                let successor = RegionSuccessor::new(RegionBranchPoint::Parent, results);
                analysis.visit_non_control_flow_arguments(
                    branch.as_operation(),
                    &successor,
                    lattices,
                    first_index,
                    solver,
                );
            } else {
                if !inputs.is_empty() {
                    let input = inputs[0].borrow();
                    first_index = input.downcast_ref::<BlockArgument>().unwrap().index();
                }
                let region = point.block().unwrap().borrow().parent().unwrap();
                let region_borrowed = region.borrow();
                let entry = region_borrowed.entry();
                let successor_arg_range = BlockArgumentRange::new(
                    first_index..(first_index + inputs.len()),
                    entry.arguments(),
                );
                let successor =
                    RegionSuccessor::new(RegionBranchPoint::Child(region), successor_arg_range);
                analysis.visit_non_control_flow_arguments(
                    branch.as_operation(),
                    &successor,
                    lattices,
                    first_index,
                    solver,
                );
            }
        }

        for (operand, lattice) in
            operands.forwarded().iter().zip(lattices.iter_mut().skip(first_index))
        {
            let operand_lattice =
                get_lattice_element_for::<A>(point, operand.borrow().as_value_ref(), solver);
            lattice.join(operand_lattice.lattice());
        }
    }
}

#[inline]
fn get_lattice_element<'guard, A>(
    value: ValueRef,
    solver: &mut DataFlowSolver,
) -> AnalysisStateGuard<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice>
where
    A: SparseForwardDataFlowAnalysis,
{
    solver.get_or_create_mut::<_, _>(value)
}

#[inline]
fn get_lattice_element_for<'guard, A>(
    point: ProgramPoint,
    value: ValueRef,
    solver: &mut DataFlowSolver,
) -> EntityRef<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice>
where
    A: SparseForwardDataFlowAnalysis,
{
    solver.require::<_, _>(value, point)
}

fn get_lattice_elements<'guard, A>(
    values: OpResultRange<'_>,
    solver: &mut DataFlowSolver,
) -> SmallVec<[AnalysisStateGuard<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice>; 2]>
where
    A: SparseForwardDataFlowAnalysis,
{
    let mut results = SmallVec::with_capacity(values.len());
    for value in values.iter() {
        let lattice = solver.get_or_create_mut::<_, _>(value.borrow().as_value_ref());
        results.push(lattice);
    }
    results
}
