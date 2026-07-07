use midenc_hir::{
    Block, BlockArgument, BlockArgumentRange, CallOpInterface, CallableOpInterface, EntityWithId,
    Forward, OpOperandRange, OpResult, OpResultRange, Operation, ProgramPoint,
    RegionBranchOpInterface, RegionBranchPoint, RegionBranchTerminatorOpInterface, RegionSuccessor,
    Report, SmallVec, Spanned, StorableEntity, SuccessorOperands, ValueRef,
    formatter::DisplayValues, traits::BranchOpInterface,
};

use super::{SparseDataFlowAnalysis, SparseLattice};
use crate::{
    AnalysisState, AnalysisStateGuard, AnalysisStateGuardMut, BuildableAnalysisState,
    CallControlFlowAction, DataFlowSolver,
    analyses::dce::{CfgEdge, Executable, PredecessorState},
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

    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    /// Indicates that this analysis can continue using known predecessor facts when some
    /// predecessors are unknown.
    ///
    /// When unknown predecessors are present, the solver first seeds affected states with the
    /// analysis entry state. If this returns true, it then also applies transfers from every known
    /// predecessor. The default is false because most analyses cannot safely combine partial
    /// predecessor information with an unknown-predecessor fixpoint.
    #[inline]
    fn allow_unknown_predecessors(&self) -> bool {
        false
    }

    /// The operation transfer function.
    ///
    /// Given the operand lattices, this function is expected to set the result lattices.
    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report>;

    /// Propagate the operand lattices forward along a call control flow edge, which can be either
    /// entering or exiting the callee.
    ///
    /// The default implementation for enter and exit callee actions invokes `join` on the states.
    /// The handling for external callees defaults to setting the `after` states to the entry state.
    ///
    /// Two types of forward-propagation are possible here:
    ///
    /// * `CallControlFlowAction::Enter` indicates:
    ///   - `before` is the states of each argument passed to the callee
    ///   - `after` is the state of each callee parameter at the beginning of the callee entry block
    /// * `CallControlFlowAction::Exit` indicates:
    ///   - `before` is the state of each result being returned from the callee
    ///   - `after` is the state of each result after the call operation
    /// * `CallControlFlowAction::External` indicates:
    ///   - `before` is the state of each argument being passed to the callee
    ///   - `after` is the state of each result after the call operation
    ///
    /// Implementations can implement additional custom behavior by handling specific cases manually.
    /// For example, if `call` may affect the lattice prior to entering the callee, the impl can
    /// handle `CallControlFlowAction::Enter`. Similarly, if `call` may affect the lattice post-
    /// exiting the callee, the impl can handle `CallControlFlowAction::Exit`.
    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        before: &[AnalysisStateGuard<'_, Self::Lattice>],
        after: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) {
        // Note that `set_to_entry_state` may be a "partial fixpoint" for some
        // lattices, e.g., lattices that are lists of maps of other lattices will
        // only set fixpoint for "known" lattices.
        if matches!(action, CallControlFlowAction::External) {
            set_all_to_entry_states(self, after);
        } else {
            for (before, after) in before.iter().zip(after.iter_mut()) {
                after.join(before.lattice());
            }
        }
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
        arguments: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        first_index: usize,
        solver: &mut DataFlowSolver,
    ) {
        let (leading, rest) = arguments.split_at_mut(first_index);
        let (_, trailing) = rest.split_at_mut(successor.successor_inputs().len());
        set_all_to_entry_states(self, leading);
        set_all_to_entry_states(self, trailing);
    }

    /// Set the given lattice element(s) at control flow entry point(s).
    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>);
}

pub fn set_all_to_entry_states<A>(
    analysis: &A,
    lattices: &mut [AnalysisStateGuardMut<'_, <A as SparseForwardDataFlowAnalysis>::Lattice>],
) where
    A: ?Sized + SparseForwardDataFlowAnalysis,
{
    for lattice in lattices {
        analysis.set_to_entry_state(lattice);
    }
}

/// Recursively initialize the analysis on nested operations and blocks.
pub(super) fn initialize_recursively<A>(
    analysis: &SparseDataFlowAnalysis<A, Forward>,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: SparseForwardDataFlowAnalysis,
{
    // Initialize the analysis by visiting every owner of an SSA value (all operations and blocks).
    visit_operation(analysis, op, solver)?;

    if !op.regions().is_empty() {
        log::trace!(target: analysis.debug_name(), "visiting regions of '{}'", op.name());
        for region in op.regions() {
            if region.is_empty() {
                continue;
            }

            for block in region.body() {
                {
                    let point = ProgramPoint::at_start_of(block.as_block_ref());
                    let exec = solver.get_or_create::<Executable, _>(point);
                    log::trace!(
                        target: analysis.debug_name(), "subscribing to changes in liveness for {block} (current={exec})",
                    );
                    AnalysisStateGuard::subscribe(&exec, analysis);
                }

                visit_block(analysis, &block, solver);

                log::trace!(target: analysis.debug_name(), "visiting body of {} top-down", block.id());
                for op in block.body() {
                    initialize_recursively(analysis, &op, solver)?;
                }
            }
        }
    }

    Ok(())
}

/// Visit an operation. If this is a call operation or an operation with
/// region control-flow, then its result lattices are set accordingly.
/// Otherwise, the operation transfer function is invoked.
pub(super) fn visit_operation<A>(
    analysis: &SparseDataFlowAnalysis<A, Forward>,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Result<(), Report>
where
    A: SparseForwardDataFlowAnalysis,
{
    log::trace!(target: analysis.debug_name(), "visiting operation {op}");

    // Exit early on operations with no results.
    if !op.has_results() {
        log::debug!(target: analysis.debug_name(), "skipping analysis for {}: op has no results", op.name());
        return Ok(());
    }

    // If the containing block is not executable, bail out.
    if op.parent().is_some_and(|block| {
        !solver
            .get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block))
            .is_live()
    }) {
        log::trace!(target: analysis.debug_name(), "skipping analysis for op in dead/non-executable block: {}", ProgramPoint::before(op));
        return Ok(());
    }

    // Get the result lattices.
    log::trace!(
        target: analysis.debug_name(),
        "getting/initializing result lattices for {}",
        DisplayValues::new(op.results().all().into_iter())
    );
    let mut result_lattices = get_lattice_elements_mut::<A>(op.results().all(), solver);

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
    let mut operand_lattices = SmallVec::<[_; 4]>::with_capacity(op.num_operands());
    // TODO: Visit unique operands first to initialize analysis state and subscribe to changes
    for operand in op.operands().iter() {
        let operand = { operand.borrow().as_value_ref() };
        log::trace!(target: analysis.debug_name(), "getting/initializing operand lattice for {operand}");
        let operand_lattice = get_lattice_element::<A>(operand, solver);
        log::trace!(
            target: analysis.debug_name(), "subscribing to changes of operand {operand} (current={operand_lattice:#?})",
        );
        AnalysisStateGuard::subscribe(&operand_lattice, analysis);
        operand_lattices.push(operand_lattice);
    }

    if let Some(call) = op.as_trait::<dyn CallOpInterface>() {
        log::trace!(target: analysis.debug_name(), "{} is a call operation", op.name());
        // If the call operation is to an external function, attempt to infer the results from the
        // call arguments.
        //
        // TODO: resolve_in_symbol_table
        let callable = call.resolve();
        let callable = callable.as_ref().map(|c| c.borrow());
        let callable = callable
            .as_ref()
            .and_then(|c| c.as_symbol_operation().as_trait::<dyn CallableOpInterface>());
        let is_external_call = callable
            .as_ref()
            .is_none_or(|callable| callable.get_callable_region().is_none());
        if !solver.config().is_interprocedural() || is_external_call {
            log::trace!(target: analysis.debug_name(), "callee {} is external", call.callable_for_callee());
            analysis.visit_call_control_flow_transfer(
                call,
                CallControlFlowAction::External,
                &operand_lattices,
                &mut result_lattices,
                solver,
            );
            return Ok(());
        }

        // Otherwise, the results of a call operation are determined by the callgraph.
        log::trace!(target: analysis.debug_name(), "resolved callee as {}", call.callable_for_callee());
        let return_point = ProgramPoint::after(op);
        log::trace!(target: analysis.debug_name(), "getting/initializing predecessor state at {return_point}");
        let predecessors = solver
            .require::<PredecessorState, _>(ProgramPoint::after(call.as_operation()), return_point);
        log::trace!(target: analysis.debug_name(), "found {} known predecessors", predecessors.known_predecessors().len());

        // If not all return sites are known, then conservatively assume we can't reason about the
        // data-flow, unless the analysis specfically indicates that it can safely do so.
        if !predecessors.all_predecessors_known() {
            log::trace!(target: analysis.debug_name(), "not all predecessors are known - setting result lattices to entry state");
            set_all_to_entry_states(analysis, &mut result_lattices);
            if !analysis.allow_unknown_predecessors() {
                return Ok(());
            }
        }

        let current_point = ProgramPoint::after(op);
        log::trace!(target: analysis.debug_name(), "joining lattices from all call site predecessors at {current_point}");
        for predecessor in predecessors.known_predecessors() {
            let inputs = predecessors.successor_inputs(predecessor);
            let mut operand_lattices = SmallVec::<[_; 4]>::with_capacity(inputs.len());
            let mut required_inputs = SmallVec::<[ValueRef; 4]>::new();
            for operand in inputs.iter().copied() {
                let operand_lattice = if required_inputs.contains(&operand) {
                    get_lattice_element::<A>(operand, solver)
                } else {
                    required_inputs.push(operand);
                    get_lattice_element_for::<A>(current_point, operand, solver)
                };
                operand_lattices.push(operand_lattice);
            }
            analysis.visit_call_control_flow_transfer(
                call,
                CallControlFlowAction::Exit,
                &operand_lattices,
                &mut result_lattices,
                solver,
            );
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
pub(super) fn visit_block<A>(
    analysis: &SparseDataFlowAnalysis<A, Forward>,
    block: &Block,
    solver: &mut DataFlowSolver,
) where
    A: SparseForwardDataFlowAnalysis,
{
    // Exit early on blocks with no arguments.
    if !block.has_arguments() {
        log::debug!(target: analysis.debug_name(), "skipping {block}: no block arguments to process");
        return;
    }

    // If the block is not executable, bail out.
    if !solver
        .get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block))
        .is_live()
    {
        log::debug!(target: analysis.debug_name(), "skipping {block}: it is dead/non-executable");
        return;
    }

    // The argument lattices of entry blocks are set by region control-flow or the callgraph.
    let current_point = ProgramPoint::at_start_of(block);
    if block.is_entry_block() {
        log::trace!(target: analysis.debug_name(), "{block} is a region entry block");

        // Get the argument lattices.
        let mut arg_lattices = SmallVec::<[_; 4]>::with_capacity(block.num_arguments());
        for argument in block.arguments().iter().copied() {
            log::trace!(target: analysis.debug_name(), "getting/initializing lattice for {argument}");
            let lattice = get_lattice_element_mut::<A>(argument as ValueRef, solver);
            arg_lattices.push(lattice);
        }

        // Check if this block is the entry block of a callable region.
        let parent_op = block.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        let callable = parent_op.as_trait::<dyn CallableOpInterface>();
        if callable.is_some_and(|c| c.get_callable_region() == block.parent()) {
            let callable = callable.unwrap();
            log::trace!(
                target: analysis.debug_name(),
                "{block} is the entry of a callable region - analyzing call sites",
            );
            let callsites = solver.require::<PredecessorState, _>(
                ProgramPoint::after(callable.as_operation()),
                current_point,
            );
            log::trace!(target: analysis.debug_name(), "found {} call sites", callsites.known_predecessors().len());

            // If not all callsites are known, conservatively mark all lattices as having reached
            // their pessimistic fixpoints.
            if !solver.config().is_interprocedural() {
                log::trace!(
                    target: analysis.debug_name(),
                    "not all call sites are known - setting arguments to entry state"
                );
                return set_all_to_entry_states(analysis, &mut arg_lattices);
            }
            if !callsites.all_predecessors_known() {
                log::trace!(
                    target: analysis.debug_name(),
                    "not all call sites are known - setting arguments to entry state"
                );
                set_all_to_entry_states(analysis, &mut arg_lattices);
                if !analysis.allow_unknown_predecessors() {
                    return;
                }
            }

            log::trace!(target: analysis.debug_name(), "joining lattices from all call site predecessors at {current_point}");
            for callsite in callsites.known_predecessors() {
                let callsite = callsite.borrow();
                let call = callsite.as_trait::<dyn CallOpInterface>().unwrap();
                let arguments = call.arguments();
                let mut input_lattices = SmallVec::<[_; 4]>::with_capacity(arguments.len());
                for input in call.arguments().iter() {
                    let input_lattice = get_lattice_element_for::<A>(
                        current_point,
                        input.borrow().as_value_ref(),
                        solver,
                    );
                    input_lattices.push(input_lattice);
                }
                analysis.visit_call_control_flow_transfer(
                    call,
                    CallControlFlowAction::Enter,
                    &input_lattices,
                    &mut arg_lattices,
                    solver,
                );
            }

            return;
        }

        // Check if the lattices can be determined from region control flow.
        if let Some(branch) = parent_op.as_trait::<dyn RegionBranchOpInterface>() {
            log::trace!(
                target: analysis.debug_name(),
                "{block} is the entry of an region control flow op",
            );

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
        log::trace!(target: analysis.debug_name(), "unable to reason about control flow for {block}");
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
    log::trace!(target: analysis.debug_name(), "visiting predecessors of non-entry block {block}");
    for pred in block.predecessors() {
        let predecessor = pred.predecessor().borrow();
        log::trace!(target: analysis.debug_name(), "visiting control flow edge {predecessor} -> {block} (index {})", pred.index);

        // If the edge from the predecessor block to the current block is not live, bail out.
        let edge_executable = {
            let anchor = solver.create_lattice_anchor(CfgEdge::new(
                predecessor.as_block_ref(),
                block.as_block_ref(),
                predecessor.span(),
            ));
            let lattice = solver.get_or_create::<Executable, _>(anchor);
            log::trace!(
                target: analysis.debug_name(), "subscribing to changes of control flow edge {anchor} (current={lattice})",
            );
            lattice
        };
        AnalysisStateGuard::subscribe(&edge_executable, analysis);
        if !edge_executable.is_live() {
            log::trace!(target: analysis.debug_name(), "skipping {predecessor}: control flow edge is dead/non-executable");
            continue;
        }

        // Check if we can reason about the data-flow from the predecessor.
        let terminator = pred.owner;
        let terminator = terminator.borrow();
        if let Some(branch) = terminator.as_trait::<dyn BranchOpInterface>() {
            log::trace!(
                target: analysis.debug_name(),
                "joining operand lattices for successor {} of {predecessor}",
                pred.index
            );
            let operands = branch.get_successor_operands(pred.index());

            for (idx, argument) in block.arguments().iter().copied().enumerate() {
                let arg = argument as ValueRef;
                if let Some(operand) =
                    operands.get(idx).and_then(|operand| operand.into_value_ref())
                {
                    log::trace!(target: analysis.debug_name(), "joining lattice for {arg} with {operand}");
                    // If the block argument is being passed as a successor opernad in the same
                    // position, then we could end up attempting to obtain the same lattice twice,
                    // thus special handling is needed
                    let operand_lattice =
                        get_lattice_element_for::<A>(current_point, operand, solver);
                    let (change_result, arg_lattice) = if arg == operand {
                        let lattice_value = operand_lattice.lattice().clone();
                        drop(operand_lattice);
                        let mut arg_lattice = get_lattice_element_mut::<A>(arg, solver);
                        let change_result = arg_lattice.join(&lattice_value);
                        (change_result, arg_lattice)
                    } else {
                        let mut arg_lattice = get_lattice_element_mut::<A>(arg, solver);
                        let change_result = arg_lattice.join(operand_lattice.lattice());
                        (change_result, arg_lattice)
                    };
                    log::debug!(target: analysis.debug_name(), "updated lattice for {arg} to {:#?}: {change_result}", &arg_lattice);
                } else {
                    // Conservatively consider internally produced arguments as entry points.
                    log::trace!(target: analysis.debug_name(), "setting lattice for internally-produced argument {arg} to entry state");
                    let mut arg_lattice = get_lattice_element_mut::<A>(arg, solver);
                    analysis.set_to_entry_state(&mut arg_lattice);
                }
            }
        } else {
            log::trace!(
                target: analysis.debug_name(),
                "unable to reason about predecessor control flow - setting argument lattices to \
                 entry state"
            );
            // Get the argument lattices.
            let mut arg_lattices = SmallVec::<[_; 4]>::with_capacity(block.num_arguments());
            for argument in block.arguments().iter().copied() {
                log::trace!(target: analysis.debug_name(), "getting/initializing lattice for {argument}");
                let lattice = get_lattice_element_mut::<A>(argument as ValueRef, solver);
                arg_lattices.push(lattice);
            }
            return set_all_to_entry_states(analysis, &mut arg_lattices);
        }
    }
}

/// Visit a program point `point` with predecessors within a region branch
/// operation `branch`, which can either be the entry block of one of the
/// regions or the parent operation itself, and set either the argument or
/// parent result lattices.
fn visit_region_successors<A>(
    analysis: &SparseDataFlowAnalysis<A, Forward>,
    point: ProgramPoint,
    branch: &dyn RegionBranchOpInterface,
    successor: RegionBranchPoint,
    lattices: &mut [AnalysisStateGuardMut<'_, <A as SparseForwardDataFlowAnalysis>::Lattice>],
    solver: &mut DataFlowSolver,
) where
    A: SparseForwardDataFlowAnalysis,
{
    log::trace!(target: analysis.debug_name(), "getting/initializing predecessor state for {point}");
    let predecessors = solver.require::<PredecessorState, _>(point, point);
    assert!(predecessors.all_predecessors_known(), "unexpected unresolved region successors");

    log::debug!(target: analysis.debug_name(), "joining the lattices from {} known predecessors", predecessors.known_predecessors().len());
    for op in predecessors.known_predecessors().iter().copied() {
        let operation = op.borrow();

        // Get the incoming successor operands.
        let mut operands = None;

        // Check if the predecessor is the parent op.
        let predecessor_is_parent = op == branch.as_operation_ref();
        log::debug!(target: analysis.debug_name(), "analyzing predecessor {} (is parent = {predecessor_is_parent})", ProgramPoint::after(&*operation));
        if predecessor_is_parent {
            operands = Some(branch.get_entry_successor_operands(successor));
        } else if let Some(region_terminator) =
            operation.as_trait::<dyn RegionBranchTerminatorOpInterface>()
        {
            // Otherwise, try to deduce the operands from a region return-like op.
            operands = Some(region_terminator.get_successor_operands(successor));
        }

        let Some(operands) = operands else {
            // We can't reason about the data-flow
            log::debug!(target: analysis.debug_name(), "unable to reason about predecessor dataflow - setting to entry state");
            return set_all_to_entry_states(analysis, lattices);
        };

        let inputs = predecessors.successor_inputs(&op);
        assert_eq!(
            inputs.len(),
            operands.len(),
            "expected the same number of successor inputs as operands"
        );

        let mut first_index = 0;
        if inputs.len() != lattices.len() {
            log::trace!(target: analysis.debug_name(), "successor inputs and argument lattices have different lengths: {} vs {}", inputs.len(), lattices.len());
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
                let region = point.block().unwrap().parent().unwrap();
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
            let operand = operand.borrow().as_value_ref();
            log::trace!(target: analysis.debug_name(), "joining lattice for {} with {operand}", lattice.anchor());
            let operand_lattice = get_lattice_element_for::<A>(point, operand, solver);
            let change_result = lattice.join(operand_lattice.lattice());
            log::debug!(target: analysis.debug_name(), "updated lattice for {} to {:#?}: {change_result}", lattice.anchor(), lattice);
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
    let lattice: AnalysisStateGuard<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice> =
        solver.get_or_create::<_, _>(value);
    lattice
}

#[inline]
fn get_lattice_element_mut<'guard, A>(
    value: ValueRef,
    solver: &mut DataFlowSolver,
) -> AnalysisStateGuardMut<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice>
where
    A: SparseForwardDataFlowAnalysis,
{
    let lattice: AnalysisStateGuardMut<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice> =
        solver.get_or_create_mut::<_, _>(value);
    lattice
}

#[inline]
fn get_lattice_element_for<'guard, A>(
    point: ProgramPoint,
    value: ValueRef,
    solver: &mut DataFlowSolver,
) -> AnalysisStateGuard<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice>
where
    A: SparseForwardDataFlowAnalysis,
{
    solver.require::<_, _>(value, point)
}

fn get_lattice_elements_mut<'guard, A>(
    values: OpResultRange<'_>,
    solver: &mut DataFlowSolver,
) -> SmallVec<[AnalysisStateGuardMut<'guard, <A as SparseForwardDataFlowAnalysis>::Lattice>; 2]>
where
    A: SparseForwardDataFlowAnalysis,
{
    let mut results = SmallVec::with_capacity(values.len());
    for value in values.iter().copied() {
        let lattice = solver.get_or_create_mut::<_, _>(value as ValueRef);
        results.push(lattice);
    }
    results
}
