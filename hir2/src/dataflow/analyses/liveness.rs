mod next_use_set;

use self::next_use_set::NextUseSet;
use super::{DeadCodeAnalysis, SparseConstantPropagation};
use crate::{
    dataflow::{
        analyses::{dce::CfgEdge, LoopState},
        dense::DenseDataFlowAnalysis,
        AnalysisState, AnalysisStateGuard, Backward, BuildableAnalysisState,
        BuildableDataFlowAnalysis, CallControlFlowAction, ChangeResult, DataFlowSolver,
        DenseBackwardDataFlowAnalysis, DenseLattice, Lattice, LatticeAnchor, LatticeAnchorRef,
    },
    dialects::hir::Function,
    dominance::DominanceInfo,
    pass::Analysis,
    BlockRef, Op, Operation, Report, Spanned, ValueRef,
};

// The distance penalty applied to an edge which exits a loop
pub const LOOP_EXIT_DISTANCE: u32 = 100_000;

/// The lattice representing liveness information for a program point.
///
/// The lattice consists of two sets of values, representing values known to be used/live at, and
/// after, the associated anchor (a program point in our case).
///
/// Each value in those sets are associated with a distance from the anchor (at and after,
/// respectively), to the next known use of that value. These distances are what provide the
/// partial order for the sets, from which we derive the lattice structure itself, with the
/// following rules:
///
/// * If a value is not in the set, it is in an unknown state. We have either not observed that
///   value yet (either a use or a definition), or the set is in its initial state. Either way, we
///   cannot reason about whether a value is dead or alive based on this state. We call this the
///   _bottom_ or _uninitialized_ state.
/// * If a value is in the set, and its next-use distance is `u32::MAX`, it is known to be unused
///   at (or after) that point in the program. Such a distance is only assigned when we reach the
///   definition for a value for which we have observed no uses. We call this the _top_ or
///   _overdefined_ state.
/// * If a value is in the set, and its next-use distance is a finite value less than `u32::MAX`,
///   it is known to be used at (or after) that point in the program, with the given distance.
///   A distance of 0 indicates that the use is at the point associated with the set. A distance of
///   1 indicates that the use is at the next operation, and so on. Special consideration is applied
///   to the distances of values across edges that exit from a loop. In these cases, the increment
///   for distances across the edge is 10,000; rather than 1, to encourage any consumers of the
///   liveness information to treat values within the loop as "closer", so as to avoid situations
///   where not doing so would result in spilling a value used inside a loop to make room for a
///   value used only when exiting the loop.
///
/// The lattice structure is given by the partial order over the next-use distances of each value.
/// We are specifically interested in the _meet semi-lattice_ of this structure, which is given by
/// computing the least-upper bound of the next-use distances in the union of two such sets. We
/// choose this over the _join semi-lattice_, because our analysis is a backwards one, working from
/// the bottom up, and at each program, what we really are interested in, are two questions:
///
/// 1. Is a given value live at (or after) some point in the program
/// 2. Given the set of live values at (or after) some point in the program, which values have the
///    closest next use?
///
/// The second question is of primary importance for spills analysis, register allocation and (in
/// the case of Miden) operand stack management. If we're going to choose what values to spill, so
/// as to keep the most important values available in registers (or the operand stack), then we
/// want to know when those values are needed.

/// The lattice representing register pressure information for a program point
#[derive(Debug, Clone)]
pub struct RegisterPressure {
    anchor: LatticeAnchorRef,
    pressure: u32,
}
impl AnalysisState for RegisterPressure {
    fn anchor(&self) -> &dyn LatticeAnchor {
        &*self.anchor
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
impl BuildableAnalysisState for RegisterPressure {
    fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            anchor,
            pressure: 0,
        }
    }
}
impl DenseLattice for RegisterPressure {
    type Lattice = Self;

    #[inline(always)]
    fn lattice(&self) -> &Self::Lattice {
        self
    }

    #[inline(always)]
    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        if self.pressure != rhs.pressure {
            self.pressure = core::cmp::max(self.pressure, rhs.pressure);
            ChangeResult::Changed
        } else {
            ChangeResult::Unchanged
        }
    }

    #[inline(always)]
    fn meet(&mut self, _rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }
}

/// This analysis computes what values are live, and the distance to next use, for all program
/// points in the given operation. It computes both live-in and live-out sets, in order to answer
/// liveness questions about the state of the program at an operation, as well as questions about
/// the state of the program immediately after an operation.
///
/// This analysis is a bit different than "standard" liveness analysis, in a few ways:
///
/// * It is not sparse, i.e. it attaches liveness information to program points, rather than values
/// * Liveness is not just a boolean state, it also represents how long the value must live until
///   its next use. This is invaluable for instruction scheduling and resource allocation (registers,
///   operand stack space).
///
/// The design for this is based on the description in _Register Spilling and Live-Range Splitting
/// for SSA-form Programs_, by Mattias Braun and Sebastian Hack. The paper also is used in our
/// algorithm for computing spills (and splitting live-ranges), as you might expect from its title.
///
/// ## The Basic Algorithm
///
/// 1. Start at the end of a block, with an empty `live_out` set
/// 2. If the block has any successors, take the `meet` of the `live_in` sets across all successors,
///    after incrementing the next-use distances in each set by 1 (for normal blocks) or by 10,000
///    (if the edge from the current block to the successor is exiting a loop). Additionally, all
///    block parameters of each successor are removed from its `live_in` set (since by definition
///    those values cannot be live before they are defined). The `meet` of these sets then becomes
///    the initial `live_out` set for the current block.
/// 3. Start visiting ops in the block bottom-up. The `live_out` set for the block terminator
///    inherits the `live_out` set computed in steps 1 and 2.
/// 4. At the block terminator, the `live_out` set is inherited from the block `live_out` set. The
///    `live_in` set is then initialized empty, and then the following steps are performed:
///    a. Any operands of the terminator are added/updated in the set with a next-use distance of 0
/// 5. Move to the previous operation in the block from the current operation, then:
///    * The `live_out` set is inherited from the successor op's `live_in` set, all results of the
///      op that are missing from the `live_out` set, are added with a distance of `u32::MAX`.
///    * The `live_in` set is populated by taking the `live_out` set, removing all of the op
///      results, incrementing the next-use distance of all values in the set by 1, and then
///      setting/adding all of the op operands with a distance of `0`.
/// 6. Repeat until we reach the top of the block. The `live_in` set for the block inherits the
///    `live_in` set of the first op in the block, but adds in any of the block parameters which
///    are missing from the set with a distance of `u32::MAX`
///
/// In essence, we work backwards from the bottom of a block to the top, propagating next-use info
/// up the CFG (and ideally visiting each block in reverse post-order, to reach fixpoint
/// efficiently).
///
/// To aid the solver in efficiently reaching fixpoint, the following are done:
///
/// 1. We drive the dense analysis using the [DominanceInfo] analysis computed for the given op,
///    this reduces the amount of extra work that needs to be done by the solver, as most blocks
///    will not change their liveness info after it is initially computed.
/// 2. Unless a block is marked live by dead code analysis, we do not compute liveness for it, and
///    we will ignore any block successors which are not marked live by dead code analysis as well.
/// 3. If the analysis is run on an operation, and it is determined that the liveness information
///    used to derive its `live_in` set has not changed since we last saw it, we do not proceed
///    with the analysis to avoid propagating changes unnecessarily
#[derive(Default)]
pub struct Liveness;

/// This type is used to compute the [LivenessAnalysis] results for an entire [Function].
///
/// Internally, it instantiates a [DataFlowSolver] with [DeadCodeAnalysis],
/// [SparseConstantPropagation], and [LivenessAnalysis], and runs them to fixpint. It additionally
/// relies on the [DominanceInfo] and [LoopForest] analyses to provide us with details about the
/// CFG structure that we then use both to optimize the work done by the solver, as well as feed
/// into the actual liveness information itself (i.e. by specifying how much distance a given
/// control flow edge adds).
#[derive(Default)]
pub struct LivenessAnalysis {
    solver: DataFlowSolver,
}

impl Analysis for LivenessAnalysis {
    type Target = Function;

    fn name(&self) -> &'static str {
        "liveness"
    }

    fn analyze(&mut self, op: &Self::Target, analysis_manager: crate::pass::AnalysisManager) {
        self.solver.load::<DeadCodeAnalysis>();
        self.solver.load::<SparseConstantPropagation>();
        self.solver.load::<Liveness>();
        self.solver
            .initialize_and_run(op.as_operation(), analysis_manager)
            .expect("liveness analysis failed");
    }

    fn invalidate(&self, preserved_analyses: &mut crate::pass::PreservedAnalyses) -> bool {
        !preserved_analyses.is_preserved::<DominanceInfo>()
    }
}

impl BuildableDataFlowAnalysis for Liveness {
    type Strategy = DenseDataFlowAnalysis<Self, Backward>;

    fn new(_solver: &mut DataFlowSolver) -> Self {
        Self
    }
}

/// Liveness is computed as a dense, backward-propagating data-flow analysis:
///
/// * Liveness information is attached to each operation, and the start and end of each block
/// * Liveness is computed by visiting the CFG of an operation in postorder, this ensures
///   that next-use information is propagated upwards in the CFG.
/// * Liveness is _not_ interprocedural
/// * Liveness _does_ take into account region control flow, i.e. a value which is used inside a
///   nested region of an operation will have a next-use distance that is either:
///   * Treated as the distance to the containing operation, if the containing op does not involve
///     region control flow (i.e. implements `RegionBranchOpInterface`)
///   * Computed as if the nested region was flattened into the current one, in cases where region
///     control flow is involved. This ensures that structured control flow ops have useful next-
///     use distances computed, e.g. values used after a `scf.while` are not considered "closer"
///     than values inside the loop.
impl DenseBackwardDataFlowAnalysis for Liveness {
    type Lattice = Lattice<NextUseSet>;

    fn symbol_table(&self) -> Option<&dyn crate::SymbolTable> {
        None
    }

    /// This is invoked when visiting a block with successors, once for each successor.
    ///
    /// This is where we handle determining what information to propagate from the successor's
    /// live-in set into the predecessor block's live-out set.
    ///
    /// * `from` is the block we're currently visiting
    /// * `to` is the successor that this control-flow edge transfers control to
    /// * `live_in_to` is the live-in state of the successor as computed by the analysis so far
    /// * `live_out_from` is the live-out state of the current block that we've computed thus far,
    ///   and which we are extending with values used across the edge, and not defined by the
    ///   successor block's parameters.
    ///
    /// This is where we take into account loop state information, if available, to determine how
    /// to increment next-use distances from the successor.
    fn visit_branch_control_flow_transfer(
        &self,
        from: &crate::Block,
        to: crate::BlockRef,
        live_in_to: &Self::Lattice,
        live_out_from: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        // Start with the live-in set
        let mut live_out = live_in_to.value().clone();

        // Remove successor params from the set
        let succ = to.borrow();
        for param in succ.arguments() {
            let param = param.borrow().as_value_ref();
            live_out.remove(&param);
        }

        // Increment the next-use distances by LOOP_EXIT_DISTANCE if this edge exits
        // a loop
        let edge = CfgEdge::new(from.as_block_ref(), to.clone(), from.span());
        let is_loop_exit =
            solver.get::<LoopState, _>(&edge).is_some_and(|state| state.is_exiting_loop());
        if is_loop_exit {
            for next_use in live_out.iter_mut() {
                next_use.distance = next_use.distance.saturating_add(LOOP_EXIT_DISTANCE);
            }
        }

        // We use `join` here, not `meet`, because we want the union of the sets, albeit the minimum
        // distances of values in both sets. The `meet` implementation for NextUseSet performs set
        // intersection, which is not what we want here
        live_out_from.join(&live_out);
    }

    /// This will be called on each operation in a block, once the initial live-out state for the
    /// block has been computed, either by default-initializing the state, or by invoking the
    /// `visit_branch_control_flow_transfer` function above for each successor and meeting the
    /// sets.
    ///
    /// * `live_out` - the live-out set of the operation, computed when the successor op in the same
    ///   block was visited, or by inheriting the live-out set of the block if it is the terminator
    /// * `live_in` - the live-in set of the operation, must be recomputed here
    ///
    /// This function is responsible for computing the live-out set for the preceding operation in
    /// the block OR the live-in set for the block itself if it is the first operation in the block.
    fn visit_operation(
        &self,
        op: &crate::Operation,
        live_out: &Self::Lattice,
        live_in: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        // If this op is orphaned, skip the analysis
        let Some(parent_block) = op.parent() else {
            return Ok(());
        };

        // To compute the live-in set, we must start with a copy of the live-out set
        let mut temp_live_in = live_out.value().clone();

        // Increment all next-use distances by 1
        for next_use in temp_live_in.iter_mut() {
            next_use.distance = next_use.distance.saturating_add(1);
        }

        // Remove the op results from the set
        for result in op.results().all().iter() {
            let result = result.borrow().as_value_ref();
            temp_live_in.remove(&result);
        }

        // Set the next-use distance of any operands to 0
        for operand in op.operands().all().iter() {
            if let Some(next_use) = temp_live_in.get_mut(&operand.borrow().as_value_ref()) {
                next_use.distance = 0;
            }
        }

        // Determine if the state has changed, if so, then overwrite `live_in` with what we've
        // computed. Otherwise, do nothing to avoid triggering re-analysis.
        if live_in.value() == &temp_live_in {
            return Ok(());
        } else {
            *live_in.value_mut() = temp_live_in;
        }

        self.propagate_live_in_to_prev_live_out(op, parent_block, live_in.value(), solver);

        Ok(())
    }

    /// Called to set the lattice state to its "overdefined" state, for our purposes, we use the
    /// empty set, i.e. nothing is live.
    fn set_to_exit_state(
        &self,
        lattice: &mut AnalysisStateGuard<'_, Self::Lattice>,
        _solver: &mut DataFlowSolver,
    ) {
        lattice.value_mut().clear();
    }

    /// This will be invoked differently depending on various situations in which call control-flow
    /// occurs:
    ///
    /// * If the solver is configured for inter-procedural analysis, and the callable op definition
    ///   is resolvable, then `CallControlFlowAction::Enter` indicates that we are propagating
    ///   liveness information from the entry block of the callee (`after`) to before the call
    ///   operation (`before`).
    /// * If the solver is not configured for inter-procedural analysis, or the callable op
    ///   is a unresolvable or resolves to a declaration, then `CallControlFlowAction::External`
    ///   will be passed, and it is up to us to decide how to handle the call op, `after` refers to
    ///   the liveness state after `call`, and `before` refers to the liveness state before `call`,
    ///   just like when [Self::visit_operation] is called.
    /// * If the analysis is visiting a block of an operation with region control-flow, and that
    ///   block exits back to the parent operation, _and_ the parent operation implements
    ///   `CallableOpInterface`, then this function will be invoked for all callsites in order to
    ///   propagate liveness information from after the call to the end of the exit block. Thus:
    ///   * `action` will be set to `CallControlFlowAction::Exit`
    ///   * `after` refers to the liveness information after the call operation
    ///   * `before` refers to the liveness information at the end of an exit block in the callee,
    ///     the specific block can be obtained via the lattice anchor.
    ///
    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn crate::CallOpInterface,
        action: CallControlFlowAction,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        assert!(
            !solver.config().is_interprocedural(),
            "support for interprocedural liveness analysis is not yet complete"
        );

        match action {
            CallControlFlowAction::Enter | CallControlFlowAction::Exit => {
                unimplemented!("interprocedural liveness analysis")
            }
            CallControlFlowAction::External => {
                self.visit_operation(call.as_operation(), after, before, solver)
                    .expect("unexpected failure computing liveness");
            }
        }
    }

    /// This is invoked in order to propagate liveness information across region control-flow
    /// transfers of `branch`, and can be invoked differently depending on the source/target of the
    /// branch itself:
    ///
    /// 1. If `region_from` is `None`, we're branching into a region of `branch`:
    ///    * `after` is the live-in set of the entry block of `region_to`
    ///    * `before` is the live-in set of the branch op itself
    /// 2. If `region_from` is `Some`, but `region_to` is `None`, we're branching out of a region of
    ///    `branch`, to `branch` itself:
    ///    * `after` is the live-out set of `branch`
    ///    * `before` is the live-out set of the exit block of `region_from`
    /// 3. If `region_from` and `region_to` are `Some`, we're branching between regions of `branch`:
    ///    * `after` is the live-in set of the entry block of `region_to`
    ///    * `before` is the live-out set of the exit block of `region_from`
    /// 4. It should not be the case that both regions are `None`, however if this does occur, we
    ///    will just delegate to `visit_operation`, as it implies that the regions of `branch` are
    ///    not going to be entered.
    ///
    /// In short, each of the above corresponds to the following:
    ///
    /// 1. We're propagating liveness out of `region_to` to the `branch` op live-in set
    /// 2. We're propagating liveness from the live-out set of `branch` op into the live-out set of
    ///    an exit from `region_from`.
    /// 3. We're propagating liveness up the region tree of `branch`, from the live-in set of one
    ///    region to the live-out set of another, just like normal branching control flow.
    /// 4. We're propagating liveness from the live-out set to the live-in set of op, the same as
    ///    is done in `visit_operation`
    fn visit_region_branch_control_flow_transfer(
        &self,
        branch: &dyn crate::RegionBranchOpInterface,
        region_from: Option<crate::RegionRef>,
        region_to: Option<crate::RegionRef>,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuard<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        match (region_from, region_to) {
            // 4.
            (None, None) => {
                self.visit_operation(branch.as_operation(), after, before, solver)
                    .expect("unexpected failure during liveness analysis");
            }
            // 1.
            (None, Some(region_to)) => {
                // We're only interested in propagating liveness out of `region_to` for values that
                // are defined in an ancestor region. We are guaranteed that removing the block
                // parameters for the entry block of `region_to` from `after`, will give us only
                // the set of values which are used in `region_to`, but not defined in `region_to`.
                //
                // This is sufficient for our needs, so the more expensive dominance check is not
                // needed.
                let mut live_in = after.value().clone();
                let region_to = region_to.borrow();
                for arg in region_to.entry().arguments() {
                    let arg = arg.clone() as ValueRef;
                    live_in.remove(&arg);
                }

                // Add in all of the op operands with next-use distance of 0
                let op = branch.as_operation();
                for operand in op.operands().iter() {
                    let operand = operand.borrow().as_value_ref();
                    live_in.insert(operand, 0);
                }

                // Determine if the state has changed, if so, then overwrite `before` with what
                // we've computed. Otherwise, do nothing to avoid triggering re-analysis.
                if before.value() == &live_in {
                    return;
                } else {
                    *before.value_mut() = live_in;
                }

                let parent_block = op.parent().unwrap();
                self.propagate_live_in_to_prev_live_out(op, parent_block, before.value(), solver);
            }
            // 2.
            (Some(region_from), None) => {
                // We're starting with the live-out set of `op`, which contains its results. We must
                // do two things here to propagate liveness to the live-out set of the exit block:
                //
                // 1. Remove `op`'s results from the set
                // 2. If `op` contains a loop, we need to determine if the region we're exiting
                //    from contains, or is part of, that loop. If so, we need to increment the
                //    remaining next-use distances by LOOP_EXIT_DISTANCE
                let op = branch.as_operation();
                let mut live_out = after.value().clone();
                for result in op.results().iter() {
                    let result = result.clone() as ValueRef;
                    live_out.remove(&result);
                }

                let is_loop_exit = if branch.has_loop() {
                    let region_index = region_from.borrow().region_number();
                    branch.is_repetitive_region(region_index)
                } else {
                    false
                };
                if is_loop_exit {
                    for next_use in live_out.iter_mut() {
                        next_use.distance = next_use.distance.saturating_add(LOOP_EXIT_DISTANCE);
                    }
                }

                // Determine if the state has changed, if so, then overwrite `before` with what
                // we've computed. Otherwise, do nothing to avoid triggering re-analysis.
                if before.value() != &live_out {
                    *before.value_mut() = live_out;
                }
            }
            // 3.
            (Some(region_from), Some(region_to)) => {
                // We're starting with the live-in set of `region_to`'s entry block, and propagating
                // to the live-out set of `region_from`'s exit to `region_to`. We must do the
                // following:
                //
                // 1. Remove the region parameters of `region_to` from the live-in set
                // 2. If region_from is part of a loop, and region_to is not, increment the next-use
                //    distance of all live values by LOOP_EXIT_DISTANCE
                let mut live_out = after.value().clone();
                let region_to = region_to.borrow();
                for arg in region_to.entry().arguments().iter() {
                    let arg = arg.clone() as ValueRef;
                    live_out.remove(&arg);
                }

                let is_loop_exit = if branch.has_loop() {
                    let region_from_index = region_from.borrow().region_number();
                    let region_to_index = region_to.region_number();
                    branch.is_repetitive_region(region_from_index)
                        && !branch.is_repetitive_region(region_to_index)
                } else {
                    false
                };
                if is_loop_exit {
                    for next_use in live_out.iter_mut() {
                        next_use.distance = next_use.distance.saturating_add(LOOP_EXIT_DISTANCE);
                    }
                }

                // Determine if the state has changed, if so, then overwrite `before` with what
                // we've computed. Otherwise, do nothing to avoid triggering re-analysis.
                if before.value() != &live_out {
                    *before.value_mut() = live_out;
                }
            }
        }
    }
}

impl Liveness {
    // Propagate live-in from `op`, to the live-out of its predecessor op, or the live-in of the
    // containing block if we've reached the start of the block.
    //
    // NOTE: `parent_block` must be the containing block of `op`
    fn propagate_live_in_to_prev_live_out(
        &self,
        op: &Operation,
        parent_block: BlockRef,
        live_in: &NextUseSet,
        solver: &mut DataFlowSolver,
    ) {
        // Is this the first op in the block?
        if let Some(prev) = op.as_operation_ref().prev() {
            // No, in which case we need to compute the live-out set for the preceding op in the
            // block, by taking the live-in set for this op, and adding entries for all of the
            // op results not yet in the set
            let mut live_out_prev = live_in.clone();
            let prev_op = prev.borrow();
            for result in prev_op.results().iter() {
                let result = result.borrow().as_value_ref();
                if live_out_prev.contains(&result) {
                    continue;
                }
                // This op result has no known uses
                live_out_prev.insert(result, u32::MAX);
            }

            // Ensure the analysis state for `prev` is initialized with `live_out_prev`
            let point = solver.program_point_after(prev);
            let mut prev_liveness = solver.get_or_create_mut::<Lattice<NextUseSet>, _>(point);
            if prev_liveness.value() != &live_out_prev {
                *prev_liveness.value_mut() = live_out_prev;
            }
        } else {
            // Yes, in which case we need to compute the live-in set for the block by taking the
            // live-in set for this op, and ensure entries for all of the block parameters
            let mut live_in_block = live_in.clone();
            let block = parent_block.borrow();
            for arg in block.arguments() {
                let arg = arg.borrow().as_value_ref();
                if live_in_block.contains(&arg) {
                    continue;
                }
                // This block argument has no known uses
                live_in_block.insert(arg, u32::MAX);
            }

            let point = solver.program_point_before(parent_block);
            let mut block_liveness = solver.get_or_create_mut::<Lattice<NextUseSet>, _>(point);
            if block_liveness.value() != &live_in_block {
                *block_liveness.value_mut() = live_in_block;
            }
        }
    }
}
