mod next_use_set;

use core::borrow::Borrow;

use midenc_hir::{
    dominance::DominanceInfo,
    pass::{Analysis, AnalysisManager, PreservedAnalyses},
    Backward, Block, BlockRef, CallOpInterface, EntityRef, Operation, ProgramPoint,
    RegionBranchOpInterface, RegionBranchPoint, RegionRef, Report, Spanned, SymbolTable, ValueRef,
};

pub use self::next_use_set::NextUseSet;
use super::{dce::Executable, DeadCodeAnalysis, SparseConstantPropagation};
use crate::{
    analyses::{dce::CfgEdge, LoopState},
    dense::DenseDataFlowAnalysis,
    AnalysisState, AnalysisStateGuardMut, BuildableDataFlowAnalysis, CallControlFlowAction,
    DataFlowSolver, DenseBackwardDataFlowAnalysis, DenseLattice, Lattice, LatticeLike,
};

/// The distance penalty applied to an edge which exits a loop
pub const LOOP_EXIT_DISTANCE: u32 = 100_000;

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

impl LivenessAnalysis {
    #[inline]
    pub fn solver(&self) -> &DataFlowSolver {
        &self.solver
    }

    /// Returns true if `value` is live on entry to `block`
    #[inline]
    pub fn is_live_at_start<V>(&self, value: V, block: BlockRef) -> bool
    where
        V: Borrow<ValueRef>,
    {
        let next_uses = self.next_uses_at(&ProgramPoint::at_start_of(block));
        next_uses.is_some_and(|nu| nu.is_live(value))
    }

    /// Returns true if `value` is live at the block terminator of `block`
    #[inline]
    pub fn is_live_at_end<V>(&self, value: V, block: BlockRef) -> bool
    where
        V: Borrow<ValueRef>,
    {
        let next_uses = self.next_uses_at(&ProgramPoint::at_end_of(block));
        next_uses.is_some_and(|nu| nu.is_live(value))
    }

    /// Returns true if `value` is live at the entry of `op`
    #[inline]
    pub fn is_live_before<V>(&self, value: V, op: &Operation) -> bool
    where
        V: Borrow<ValueRef>,
    {
        let next_uses = self.next_uses_at(&ProgramPoint::before(op));
        next_uses.is_some_and(|nu| nu.is_live(value))
    }

    /// Returns true if `value` is live on exit from `op`
    #[inline]
    pub fn is_live_after<V>(&self, value: V, op: &Operation) -> bool
    where
        V: Borrow<ValueRef>,
    {
        let next_uses = self.next_uses_at(&ProgramPoint::after(op));
        next_uses.is_some_and(|nu| nu.is_live(value))
    }

    /// Returns true if `value` is live after entering `op`, i.e. when executing any of its child
    /// regions.
    ///
    /// This will return true if `value` is live after exiting from any of `op`'s regions, as well
    /// as in the case where none of `op`'s regions are executed and control is transferred to the
    /// next op in the containing block.
    #[inline]
    pub fn is_live_after_entry<V>(&self, value: V, op: &Operation) -> bool
    where
        V: Borrow<ValueRef>,
    {
        let value = value.borrow();
        if self.is_live_after(value, op) {
            return true;
        }

        if let Some(br_op) = op.as_trait::<dyn RegionBranchOpInterface>() {
            for succ in br_op.get_successor_regions(RegionBranchPoint::Parent) {
                if let Some(region) = succ.into_successor() {
                    let entry = region.borrow().entry_block_ref().unwrap();
                    if !self.is_block_executable(entry) {
                        // Ignore dead regions
                        continue;
                    }

                    if self.is_live_at_start(value, entry) {
                        return true;
                    }
                }
            }
        }

        false
    }

    #[inline]
    pub fn next_use_after<V>(&self, value: V, op: &Operation) -> u32
    where
        V: Borrow<ValueRef>,
    {
        let next_uses = self.next_uses_at(&ProgramPoint::after(op));
        next_uses.map(|nu| nu.distance(value)).unwrap_or(u32::MAX)
    }

    #[inline]
    pub fn is_block_executable(&self, block: BlockRef) -> bool {
        self.solver
            .get::<Executable, _>(&ProgramPoint::at_start_of(block))
            .is_none_or(|state| state.is_live())
    }

    #[inline]
    pub fn next_uses_at(&self, anchor: &ProgramPoint) -> Option<EntityRef<'_, NextUseSet>> {
        self.solver
            .get::<Lattice<NextUseSet>, _>(anchor)
            .map(|next_uses| EntityRef::map(next_uses, |nu| nu.value()))
    }
}

impl Analysis for LivenessAnalysis {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "liveness"
    }

    fn analyze(
        &mut self,
        op: &Self::Target,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        self.solver.load::<DeadCodeAnalysis>();
        self.solver.load::<SparseConstantPropagation>();
        self.solver.load::<Liveness>();
        self.solver
            .initialize_and_run(op, analysis_manager)
            .expect("liveness analysis failed");

        Ok(())
    }

    fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) -> bool {
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

    fn debug_name(&self) -> &'static str {
        "liveness"
    }

    fn symbol_table(&self) -> Option<&dyn SymbolTable> {
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
        from: &Block,
        to: BlockRef,
        live_in_to: &Self::Lattice,
        live_out_from: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        // Start with the live-in set
        let mut live_out = live_in_to.value().clone();

        // Remove successor params from the set
        let succ = to.borrow();
        for param in succ.arguments() {
            let param = param.borrow().as_value_ref();
            live_out.remove(param);
        }

        // Increment the next-use distances by LOOP_EXIT_DISTANCE if this edge exits
        // a loop
        let edge = CfgEdge::new(from.as_block_ref(), to, from.span());
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
        op: &Operation,
        live_out: &Self::Lattice,
        live_in: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        // If this op is orphaned, skip the analysis
        let Some(parent_block) = op.parent() else {
            return Ok(());
        };
        log::trace!(
            target: self.debug_name(),
            "deriving live-in for {op} from live-out at {}: {:#?}",
            live_out.anchor(),
            live_out.value()
        );

        // To compute the live-in set, we must start with a copy of the live-out set
        let mut temp_live_in = live_out.value().clone();

        // Increment all next-use distances by 1
        for next_use in temp_live_in.iter_mut() {
            next_use.distance = next_use.distance.saturating_add(1);
        }

        // Remove the op results from the set
        for result in op.results().all().iter() {
            let result = result.borrow().as_value_ref();
            temp_live_in.remove(result);
        }

        // Set the next-use distance of any operands to 0
        for operand in op.operands().all().iter() {
            temp_live_in.insert(operand.borrow().as_value_ref(), 0);
        }

        // Determine if the state has changed, if so, then overwrite `live_in` with what we've
        // computed. Otherwise, do nothing to avoid triggering re-analysis.
        log::trace!(target: self.debug_name(), "computed live-in for {op}: {:#?}", &temp_live_in);
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
        lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        _solver: &mut DataFlowSolver,
    ) {
        lattice.value_mut().clear();
        log::trace!(target: self.debug_name(), "set lattice for {} to exit state: {:#?}", lattice.anchor(), lattice.value());
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
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
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
        branch: &dyn RegionBranchOpInterface,
        region_from: Option<RegionRef>,
        region_to: Option<RegionRef>,
        after: &Self::Lattice,
        before: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        log::trace!(target: self.debug_name(), "visit region branch operation: {}", branch.as_operation());
        log::trace!(
            target: self.debug_name(),
            "propagating liveness backwards along control flow edge {} -> {}",
            before.anchor(),
            after.anchor()
        );
        log::trace!(target: self.debug_name(), "source lattice: {:#?}", after.value());
        log::trace!(target: self.debug_name(), "target lattice: {:#?}", before.value());

        match (region_from, region_to) {
            // 4.
            (None, None) => {
                log::debug!(
                    "control flow does not visit any child regions, visiting like a regular op"
                );
                self.visit_operation(branch.as_operation(), after, before, solver)
                    .expect("unexpected failure during liveness analysis");
            }
            // 1.
            (None, Some(region_to)) => {
                log::debug!(
                    "propagating live-in set of region entry block to live-in of region branch op"
                );
                // We're only interested in propagating liveness out of `region_to` for values that
                // are defined in an ancestor region. We are guaranteed that removing the block
                // parameters for the entry block of `region_to` from `after`, will give us only
                // the set of values which are used in `region_to`, but not defined in `region_to`.
                //
                // This is sufficient for our needs, so the more expensive dominance check is not
                // needed.
                let mut live_in = after.value().clone();
                let region_to = region_to.borrow();
                let region_to_entry = region_to.entry();
                let op = branch.as_operation();

                // Remove region entry arguments for `region_to`
                for arg in region_to_entry.arguments() {
                    live_in.remove(*arg as ValueRef);
                }

                // Remove operation results of `branch`
                for result in op.results().iter() {
                    live_in.remove(*result as ValueRef);
                }

                // Set next-use distance of all operands 0
                for operand in op.operands().iter() {
                    let operand = operand.borrow().as_value_ref();
                    live_in.insert(operand, 0);
                }

                // Join the before/after lattices to ensure we propagate liveness from multi-exit
                // regions, e.g. `hir.if`
                let before_live_in = before.value().join(&live_in);

                // Determine if the state has changed, if so, then overwrite `before` with what
                // we've computed. Otherwise, do nothing to avoid triggering re-analysis.
                log::trace!(
                    target: self.debug_name(),
                    "joined live-in lattice of {} with live-in of {}: {:#?}",
                    before.anchor(),
                    after.anchor(),
                    &before_live_in
                );
                if before.value() == &before_live_in {
                    return;
                } else {
                    *before.value_mut() = before_live_in;
                }

                let parent_block = op.parent().unwrap();
                self.propagate_live_in_to_prev_live_out(op, parent_block, before.value(), solver);
            }
            // 2.
            (Some(region_from), None) => {
                log::debug!(
                    "propagating live-out set of region branch op to live-out set of region exit \
                     block"
                );
                // We're starting with the live-out set of `op`, which contains some/all of its
                // results. We must do two things here to propagate liveness to the live-out set of
                // the exit block (which is derived from its terminator op):
                //
                // 1. Remove `op`'s results from the set
                // 2. If `region_from` is a repetitive region (i.e. part of a loop), we need to
                //    increment the remaining next-use distances by LOOP_EXIT_DISTANCE
                //let op = branch.as_operation();
                let mut live_out = after.value().clone();
                let is_loop_exit =
                    branch.is_repetitive_region(region_from.borrow().region_number());
                log::debug!(
                    "exit region is part of a loop, so this control flow edge represents a loop \
                     exit"
                );
                if is_loop_exit {
                    for value in live_out.iter_mut() {
                        value.distance = value.distance.saturating_add(LOOP_EXIT_DISTANCE);
                    }
                }

                // Remove results of branch op
                for result in branch.as_operation().results().iter() {
                    live_out.remove(*result as ValueRef);
                }

                // Take the join of before and after, so that we take the minimum distance across
                // all successors of `branch_op`
                let before_live_out = before.value().join(&live_out);

                // Determine if the state has changed, if so, then overwrite `before` with what
                // we've computed. Otherwise, do nothing to avoid triggering re-analysis.
                log::trace!(
                    target: self.debug_name(),
                    "joined live-out lattice of {} with live-out lattice of {}: {:#?}",
                    before.anchor(),
                    after.anchor(),
                    &before_live_out
                );
                if before.value() != &before_live_out {
                    *before.value_mut() = before_live_out.clone();
                }

                // We need to attach the live-out information to the terminator of `region_from`
                // that is exiting to the parent op so that it is picked up by the dense dataflow
                // analysis framework prior to visit_operation being invoked
                let pp = before.anchor().as_program_point().unwrap();
                let terminator = pp.operation().expect("expected block terminator");

                // Ensure the analysis state for `terminator` is initialized with `before_live_out`
                let point = ProgramPoint::after(terminator);
                log::debug!("propagating live-out lattice of {pp} to live-out of {point}");
                let mut term_liveness = solver.get_or_create_mut::<Lattice<NextUseSet>, _>(point);
                if term_liveness.value() != &before_live_out {
                    *term_liveness.value_mut() = before_live_out;
                }
            }
            // 3.
            (Some(region_from), Some(region_to)) => {
                log::trace!(
                    target: self.debug_name(),
                    "propagating live-in lattice to live-out lattice for cross-region control flow",
                );
                // We're starting with the live-in set of `region_to`'s entry block, and propagating
                // to the live-out set of `region_from`'s exit to `region_to`. We must do the
                // following:
                //
                // 1. Remove the region parameters of `region_to` from the live-in set
                // 2. If region_from is part of a loop, and region_to is not, increment the next-use
                //    distance of all live values by LOOP_EXIT_DISTANCE
                let mut live_in = after.value().clone();
                let region_to = region_to.borrow();
                let region_to_entry = region_to.entry();
                let is_loop_exit = branch
                    .is_repetitive_region(region_from.borrow().region_number())
                    && !branch.is_repetitive_region(region_to.region_number());
                for arg in region_to_entry.arguments() {
                    live_in.remove(*arg as ValueRef);
                }
                if is_loop_exit {
                    log::debug!(
                        "predecessor region is part of a loop, but successor is not, so this \
                         control flow edge represents a loop exit"
                    );
                    for value in live_in.iter_mut() {
                        value.distance = value.distance.saturating_add(LOOP_EXIT_DISTANCE);
                    }
                }

                // Take the join of before and after, so that we take the minimum distance across
                // all successors of `branch_op`
                let before_live_out = before.value().join(&live_in);

                // Determine if the state has changed, if so, then overwrite `before` with what
                // we've computed. Otherwise, do nothing to avoid triggering re-analysis.
                log::trace!(
                    target: self.debug_name(),
                    "joined live-out lattice of {} with live-in lattice of {}: {:#?}",
                    before.anchor(),
                    after.anchor(),
                    &before_live_out
                );
                if before.value() != &before_live_out {
                    *before.value_mut() = before_live_out.clone();
                }

                // We need to attach the live-out information to the terminator of `region_from`
                // that is exiting to the parent op so that it is picked up by the dense dataflow
                // analysis framework prior to visit_operation being invoked
                let pp = before.anchor().as_program_point().unwrap();
                let terminator = pp.operation().expect("expected block terminator");

                // Ensure the analysis state for `terminator` is initialized with `before_live_out`
                let point = ProgramPoint::after(terminator);
                log::debug!("propagating live-out lattice of {pp} to live-out of {point}");
                let mut term_liveness = solver.get_or_create_mut::<Lattice<NextUseSet>, _>(point);
                if term_liveness.value() != &before_live_out {
                    *term_liveness.value_mut() = before_live_out;
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
            log::debug!(
                "propagating live-in of {} to live-out of {}",
                ProgramPoint::before(op),
                ProgramPoint::after(prev)
            );
            // No, in which case we need to compute the live-out set for the preceding op in the
            // block, by taking the live-in set for this op, and adding entries for all of the
            // op results not yet in the set
            let mut live_out_prev = live_in.clone();
            let prev_op = prev.borrow();
            for result in prev_op.results().iter() {
                let result = result.borrow().as_value_ref();
                if live_out_prev.contains(result) {
                    continue;
                }
                // This op result has no known uses
                live_out_prev.insert(result, u32::MAX);
            }

            // Ensure the analysis state for `prev` is initialized with `live_out_prev`
            let point = ProgramPoint::after(prev);
            log::trace!(
                target: self.debug_name(),
                "joined live-out lattice of {} with live-in lattice of {}: {:#?}",
                point,
                ProgramPoint::before(op),
                &live_out_prev
            );
            let mut prev_liveness = solver.get_or_create_mut::<Lattice<NextUseSet>, _>(point);
            if prev_liveness.value() != &live_out_prev {
                *prev_liveness.value_mut() = live_out_prev;
            }
        } else {
            log::debug!(
                "propagating live-in of {} to live-in of {}",
                ProgramPoint::before(op),
                ProgramPoint::at_start_of(parent_block)
            );
            // Yes, in which case we need to compute the live-in set for the block by taking the
            // live-in set for this op, and ensure entries for all of the block parameters
            let mut live_in_block = live_in.clone();
            let block = parent_block.borrow();
            for arg in block.arguments().iter().copied() {
                let arg = arg as ValueRef;
                if live_in_block.contains(arg) {
                    continue;
                }
                // This block argument has no known uses
                live_in_block.insert(arg, u32::MAX);
            }

            let point = ProgramPoint::at_start_of(parent_block);
            log::trace!(
                target: self.debug_name(),
                "joined live-in lattice of {} to live-in lattice of {}: {:#?}",
                point,
                ProgramPoint::before(op),
                &live_in_block
            );
            let mut block_liveness = solver.get_or_create_mut::<Lattice<NextUseSet>, _>(point);
            if block_liveness.value() != &live_in_block {
                *block_liveness.value_mut() = live_in_block;
            }
        }
    }
}
