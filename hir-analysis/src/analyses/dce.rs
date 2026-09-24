use core::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    ptr::NonNull,
};

use midenc_hir::{
    AsValueRange, AttributeRef, Block, BlockRef, CallOpInterface, CallableOpInterface,
    EntityWithId, Forward, Operation, OperationRef, ProgramPoint, RegionBranchOpInterface,
    RegionBranchPoint, RegionBranchTerminatorOpInterface, RegionSuccessorIter, Report, SmallVec,
    SourceSpan, Spanned, SymbolMap, ValueRef,
    adt::{SmallDenseMap, SmallSet},
    pass::AnalysisManager,
    traits::{BranchOpInterface, ReturnLike},
};

use super::{CallableUseSnapshot, constant_propagation::ConstantValue};
use crate::{
    AnalysisQueue, AnalysisState, AnalysisStateGuardMut, AnalysisStateInfo,
    AnalysisStateSubscription, AnalysisStateSubscriptionBehavior, AnalysisStrategy,
    BuildableAnalysisState, BuildableDataFlowAnalysis, ChangeResult, DataFlowAnalysis,
    DataFlowSolver, Dense, Lattice, LatticeAnchor, LatticeAnchorRef,
};

/// This is a simple analysis state that represents whether the associated lattice anchor
/// (either a block or a control-flow edge) is live.
#[derive(Debug)]
pub struct Executable {
    anchor: LatticeAnchorRef,
    is_live: bool,
}
impl core::fmt::Display for Executable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_live {
            f.write_str("live")
        } else {
            f.write_str("dead")
        }
    }
}
impl Executable {
    #[inline(always)]
    pub const fn is_live(&self) -> bool {
        self.is_live
    }

    #[inline(always)]
    pub fn mark_live(&mut self) -> ChangeResult {
        if core::mem::replace(&mut self.is_live, true) {
            ChangeResult::Unchanged
        } else {
            ChangeResult::Changed
        }
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn mark_dead(&mut self) -> ChangeResult {
        if core::mem::replace(&mut self.is_live, false) {
            ChangeResult::Changed
        } else {
            ChangeResult::Unchanged
        }
    }
}
impl BuildableAnalysisState for Executable {
    fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            anchor,
            // Optimistically assume the anchor is dead
            is_live: false,
        }
    }
}
impl AnalysisState for Executable {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        &self.anchor
    }
}
impl AnalysisStateSubscriptionBehavior for Executable {
    fn on_require_analysis(
        &self,
        info: &mut AnalysisStateInfo,
        current_analysis: core::ptr::NonNull<dyn DataFlowAnalysis>,
        dependent: ProgramPoint,
    ) {
        // Ensure we re-run at the dependent point
        info.subscribe(AnalysisStateSubscription::AtPoint {
            analysis: current_analysis,
            point: dependent,
        });
    }

    fn on_subscribe(&self, subscriber: NonNull<dyn DataFlowAnalysis>, info: &AnalysisStateInfo) {
        info.subscribe(AnalysisStateSubscription::OnUpdate {
            analysis: subscriber,
        });
    }

    fn on_update(&self, info: &mut AnalysisStateInfo, worklist: &mut AnalysisQueue) {
        use crate::solver::QueuedAnalysis;

        // If there are no on-update subscribers, we have nothing to do
        let no_update_subscriptions = info.on_update_subscribers_count() == 0;
        if no_update_subscriptions {
            return;
        }

        // When the executable state changes, re-enqueue any of the on-update subscribers
        let anchor = info.anchor();
        if let Some(point) = anchor.as_program_point() {
            if point.is_at_block_start() {
                // Re-invoke analyses on the block itself
                for analysis in info.on_update_subscribers() {
                    worklist.push_back(QueuedAnalysis { point, analysis });
                }
                // Re-invoke analyses on all operations in the block
                let block = point.block().unwrap();
                for op in block.borrow().body() {
                    let point = ProgramPoint::after(&*op);
                    for analysis in info.on_update_subscribers() {
                        worklist.push_back(QueuedAnalysis { point, analysis });
                    }
                }
            }
        } else if let Some(edge) = (anchor as &dyn Any).downcast_ref::<CfgEdge>() {
            // Re-invoke the analysis on the successor block
            let point = ProgramPoint::before(edge.to());
            for analysis in info.on_update_subscribers() {
                worklist.push_back(QueuedAnalysis { point, analysis });
            }
        }
    }
}

/// This analysis state represents a set of live control-flow "predecessors" of a program point
/// (either an operation or a block), which are the last operations along all execution paths that
/// pass through this point.
///
/// For example, in dead-code analysis, an operation with region control-flow can be the predecessor
/// of a region's entry block or itself, the exiting terminator of a region can be the predecessor
/// of the parent operation or another region's entry block, the callsite of a callable operation
/// can be the predecessor to its entry block, and the exiting terminator of a callable operation
/// can be the predecessor of the call operation.
///
/// The state can optionally contain information about which values are propagated from each
/// predecessor to the successor point.
///
/// The state can indicate that it is underdefined, meaning that not all live control-flow
/// predecessors can be known.
pub struct PredecessorState {
    anchor: LatticeAnchorRef,
    /// The known control-flow predecessors of this program point.
    known_predecessors: SmallSet<OperationRef, 4>,
    /// The successor inputs when branching from a given predecessor.
    successor_inputs: SmallDenseMap<OperationRef, SmallVec<[ValueRef; 4]>>,
    /// Whether all predecessors are known. Optimistically assume that we know all predecessors.
    all_known: bool,
}

impl fmt::Debug for PredecessorState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PredecessorState")
            .field_with("anchor", |f| fmt::Display::fmt(&self.anchor, f))
            .field_with("known_predecessors", |f| {
                let mut builder = f.debug_list();
                for pred in self.known_predecessors.iter() {
                    let pred = pred.borrow();
                    builder
                        .entry_with(|f| write!(f, "{} in {}", pred.name(), pred.parent().unwrap()));
                }
                builder.finish()
            })
            .field_with("successor_inputs", |f| {
                let mut builder = f.debug_list();
                for (op, inputs) in self.successor_inputs.iter() {
                    let op = op.borrow();
                    builder.entry_with(|f| {
                        f.debug_map()
                            .key_with(|f| write!(f, "{} in {}", op.name(), op.parent().unwrap()))
                            .value(inputs)
                            .finish()
                    });
                }
                builder.finish()
            })
            .field("all_known", &self.all_known)
            .finish()
    }
}

impl PredecessorState {
    #[inline(always)]
    pub const fn all_predecessors_known(&self) -> bool {
        self.all_known
    }

    #[inline(always)]
    pub fn known_predecessors(&self) -> &[OperationRef] {
        self.known_predecessors.as_slice()
    }

    /// Indicate that there are potentially unknown predecessors.
    pub fn set_has_unknown_predecessors(&mut self) -> ChangeResult {
        if core::mem::replace(&mut self.all_known, false) {
            ChangeResult::Changed
        } else {
            ChangeResult::Unchanged
        }
    }

    #[allow(unused)]
    #[inline]
    pub fn successor_inputs(&self, predecessor: &OperationRef) -> &[ValueRef] {
        &self.successor_inputs[predecessor]
    }

    pub fn join(&mut self, predecessor: OperationRef) -> ChangeResult {
        if self.known_predecessors.insert(predecessor) {
            self.known_predecessors.sort_by(stable_operation_cmp);
            self.successor_inputs.insert(predecessor, Default::default());
            ChangeResult::Changed
        } else {
            ChangeResult::Unchanged
        }
    }

    pub fn join_with_inputs(
        &mut self,
        predecessor: OperationRef,
        inputs: impl IntoIterator<Item = ValueRef>,
    ) -> ChangeResult {
        let mut result = self.join(predecessor);
        let prev_inputs = self.successor_inputs.get_mut(&predecessor).unwrap();
        let inputs = inputs.into_iter().collect::<SmallVec<[_; 4]>>();
        if prev_inputs != &inputs {
            *prev_inputs = inputs;
            result |= ChangeResult::Changed;
        }
        result
    }
}

/// Orders operations by stable IR position rather than pointer identity.
fn stable_operation_cmp(a: &OperationRef, b: &OperationRef) -> core::cmp::Ordering {
    use core::cmp::Ordering;

    if OperationRef::ptr_eq(a, b) {
        return Ordering::Equal;
    }

    match (a.parent(), b.parent()) {
        (Some(a_block), Some(b_block)) => {
            let block_order = a_block.borrow().id().cmp(&b_block.borrow().id());
            if block_order != Ordering::Equal {
                return block_order;
            }

            if a.borrow().is_before_in_block(b) {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => {
            let a = a.borrow();
            let b = b.borrow();
            a.name().cmp(&b.name()).then_with(|| a.span().cmp(&b.span()))
        }
    }
}

impl BuildableAnalysisState for PredecessorState {
    fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            anchor,
            known_predecessors: Default::default(),
            successor_inputs: Default::default(),
            all_known: true,
        }
    }
}

impl AnalysisState for PredecessorState {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        &self.anchor
    }
}

#[derive(Copy, Clone, Debug, Spanned)]
pub struct CfgEdge {
    #[span]
    span: SourceSpan,
    from: BlockRef,
    to: BlockRef,
}
impl CfgEdge {
    pub fn new(from: BlockRef, to: BlockRef, span: SourceSpan) -> Self {
        Self { span, from, to }
    }

    #[allow(unused)]
    #[inline(always)]
    pub const fn from(&self) -> BlockRef {
        self.from
    }

    #[inline(always)]
    pub const fn to(&self) -> BlockRef {
        self.to
    }
}
impl Eq for CfgEdge {}
impl PartialEq for CfgEdge {
    fn eq(&self, other: &Self) -> bool {
        self.from == other.from && self.to == other.to
    }
}
impl core::hash::Hash for CfgEdge {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.from.hash(state);
        self.to.hash(state);
    }
}
impl fmt::Display for CfgEdge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use midenc_hir::EntityWithId;
        let from = self.from.borrow().id();
        let to = self.to.borrow().id();
        write!(f, "{from} -> {to}")
    }
}
impl LatticeAnchor for CfgEdge {}

/// Dead code analysis analyzes control-flow, as understood by [RegionBranchOpInterface] and
/// [BranchOpInterface], and the callgraph, as understood by [CallableOpInterface] and
/// [CallOpInterface].
///
/// This analysis uses known constant values of operands to determine the liveness of each block and
/// each edge between a block and its predecessors. For region control-flow, this analysis
/// determines the predecessor operations for region entry blocks and region control-flow
/// operations. For the callgraph, this analysis determines the callsites and live returns of every
/// function.
pub struct DeadCodeAnalysis {
    /// The top-level operation the analysis is running on. This is used to detect
    /// if a callable is outside the scope of the analysis and thus must be
    /// considered an external callable.
    analysis_scope: Cell<Option<OperationRef>>,
    /// A symbol table used for O(1) symbol lookups during simplification.
    #[allow(unused)]
    symbol_table: RefCell<SymbolMap>,
}

impl BuildableDataFlowAnalysis for DeadCodeAnalysis {
    type Strategy = Self;

    #[inline(always)]
    fn new(_solver: &mut crate::DataFlowSolver) -> Self {
        Self {
            analysis_scope: Cell::new(None),
            symbol_table: Default::default(),
        }
    }
}

impl AnalysisStrategy<DeadCodeAnalysis> for DeadCodeAnalysis {
    type Direction = Forward;
    type Kind = Dense;

    #[inline(always)]
    fn build(analysis: Self, _solver: &mut crate::DataFlowSolver) -> Self {
        analysis
    }
}

impl DataFlowAnalysis for DeadCodeAnalysis {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    fn debug_name(&self) -> &'static str {
        "dead-code"
    }

    fn initialize(
        &self,
        top: &Operation,
        solver: &mut crate::DataFlowSolver,
        _analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        // Mark the top-level blocks as executable.
        log::trace!(target: self.debug_name(), "marking all non-empty region entry blocks as executable");
        for region in top.regions() {
            if region.is_empty() {
                continue;
            }

            let entry = ProgramPoint::at_start_of(region.entry_block_ref().unwrap());
            let mut state = solver.get_or_create_mut::<Executable, _>(entry);
            let change_result = state.change(|exec| exec.mark_live());
            log::debug!(
                target: self.debug_name(),
                "marking region {} at {entry} as executable: {change_result}",
                region.region_number()
            );
        }

        // Mark as overdefined the predecessors of callable symbols with potentially unknown
        // predecessors.
        self.initialize_callable_symbols(top, solver);

        self.initialize_recursively(top, solver)?;

        Ok(())
    }

    fn visit(
        &self,
        point: &ProgramPoint,
        solver: &mut crate::DataFlowSolver,
    ) -> Result<(), Report> {
        if point.is_at_block_start() {
            log::debug!(target: self.debug_name(), "not visiting {point} as it is at block start");
            return Ok(());
        }

        let operation = point.prev_operation().unwrap();
        let op = operation.borrow();

        log::debug!(target: self.debug_name(), "analyzing op preceding program point {point}: {op}");

        // If the parent block is not executable, there is nothing to do.
        if operation.parent().is_none_or(|block| {
            !solver
                .get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block))
                .is_live()
        }) {
            log::debug!(target: self.debug_name(), "skipping analysis at {point} as parent block is dead/non-executable");
            return Ok(());
        }

        if let Some(call) = op.as_trait::<dyn CallOpInterface>() {
            // We have a live call op. Add this as a live predecessor of the callee.
            self.visit_call_operation(call, solver);
        }

        // Visit the regions.
        if op.has_regions() {
            // Check if we can reason about the region control-flow.
            if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
                self.visit_region_branch_operation(branch, solver);
            } else if op.implements::<dyn CallableOpInterface>() {
                log::debug!(
                    target: self.debug_name(),
                    "{} is a callable operation: resolving call site predecessors..",
                    op.name()
                );
                let callsites = solver.require::<PredecessorState, _>(
                    ProgramPoint::after(&*op),
                    ProgramPoint::after(&*op),
                );
                log::trace!(target: self.debug_name(), "found {} call sites", callsites.known_predecessors().len());

                // If the callsites could not be resolved or are known to be non-empty, mark the
                // callable as executable.
                if !callsites.all_predecessors_known() || !callsites.known_predecessors().is_empty()
                {
                    log::trace!(
                        target: self.debug_name(),
                        "not all call site predecessors are known - marking callable entry blocks \
                         as live"
                    );
                    self.mark_entry_blocks_live(&op, solver);
                }
            } else {
                // Otherwise, conservatively mark all entry blocks as executable.
                log::debug!(
                    target: self.debug_name(),
                    "op has regions, but is not a call or region control flow op: conservatively \
                     marking entry blocks live"
                );
                self.mark_entry_blocks_live(&op, solver);
            }
        }

        if is_region_or_callable_return(&op) {
            log::debug!(target: self.debug_name(), "op is a return-like operation from a region or callable");
            let parent_op = op.parent_op().unwrap();
            let parent_op = parent_op.borrow();
            if let Some(branch) = parent_op.as_trait::<dyn RegionBranchOpInterface>() {
                // Visit the exiting terminator of a region.
                self.visit_region_terminator(&op, branch, solver);
            } else if let Some(callable) = parent_op.as_trait::<dyn CallableOpInterface>() {
                // Visit the exiting terminator of a callable.
                self.visit_callable_terminator(&op, callable, solver);
            }
        }

        // Visit the successors.
        if op.has_successors() {
            log::debug!(target: self.debug_name(), "visiting successors of {}", op.name());
            // Check if we can reason about the control-flow.
            //
            // Otherwise, conservatively mark all successors as exectuable.
            if let Some(branch) = op.as_trait::<dyn BranchOpInterface>() {
                log::trace!(
                    target: self.debug_name(), "we can reason about op's successors as it implements BranchOpInterface"
                );
                self.visit_branch_operation(branch, solver);
            } else {
                log::trace!(
                    target: self.debug_name(), "we can't reason about op's successors, so conservatively marking them live"
                );
                for successor in op.successors().all() {
                    let succ = successor.successor();
                    let successor_block = succ.borrow();
                    let op_block = operation.parent().unwrap();
                    self.mark_edge_live(&op_block.borrow(), &successor_block, solver);
                }
            }
        }

        log::debug!(target: self.debug_name(), "finished analysis for {}", op.name());

        Ok(())
    }
}

type MaybeConstOperands = SmallVec<[Option<AttributeRef>; 2]>;

impl DeadCodeAnalysis {
    /// Find and mark callable symbols with potentially unknown callsites as having overdefined
    /// predecessors. `top` is the top-level operation that the analysis is operating on.
    fn initialize_callable_symbols(&self, top: &Operation, solver: &mut DataFlowSolver) {
        log::trace!(target: self.debug_name(), "initializing callable symbols in '{}'", top.name());

        self.analysis_scope.set(Some(top.as_operation_ref()));

        let uses = CallableUseSnapshot::new(top);
        for (target, info) in uses.iter() {
            if target.callable_region().is_none() || !info.has_unknown_callers() {
                continue;
            }
            let mut state = solver.get_or_create_mut::<PredecessorState, _>(ProgramPoint::after(
                target.as_operation_ref(),
            ));
            state.set_has_unknown_predecessors();
        }
    }

    /// Recursively Initialize the analysis on nested regions.
    fn initialize_recursively(
        &self,
        op: &Operation,
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        // Initialize the analysis by visiting every op with control-flow semantics.
        if op.has_regions()
            || op.has_successors()
            || is_region_or_callable_return(op)
            || op.implements::<dyn CallOpInterface>()
        {
            // When the liveness of the parent block changes, make sure to re-invoke the analysis on
            // the op.
            if let Some(block) = op.parent() {
                let exec =
                    solver.get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(block));
                log::trace!(
                    target: self.debug_name(), "subscribing {} to changes in liveness of {block} (currently={})",
                    self.debug_name(),
                    exec.is_live()
                );
                AnalysisStateGuardMut::subscribe(&exec, self);
            }

            // Visit the op.
            let point = ProgramPoint::after(op);
            self.visit(&point, solver)?;
        }

        // Recurse on nested operations.
        let regions = op.regions();
        if !regions.is_empty() {
            log::trace!(target: self.debug_name(), "visiting regions of '{}'", op.name());
            for region in regions {
                if region.is_empty() {
                    continue;
                }
                for block in region.body() {
                    log::trace!(target: self.debug_name(), "visiting body of {} top-down", block.id());
                    for op in block.body() {
                        self.initialize_recursively(&op, solver)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Mark the edge between `from` and `to` as executable.
    fn mark_edge_live(&self, from: &Block, to: &Block, solver: &mut DataFlowSolver) {
        let mut state = solver.get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(to));
        let change_result = state.change(|exec| exec.mark_live());
        log::debug!(target: self.debug_name(), "marking control flow edge successor {} live: {change_result}", to.id());

        // Ensure change notifications for the block are flushed first
        drop(state);

        let mut edge_state = solver.get_or_create_mut::<Executable, _>(CfgEdge::new(
            from.as_block_ref(),
            to.as_block_ref(),
            from.span(),
        ));
        let change_result = edge_state.change(|exec| exec.mark_live());
        log::debug!(
            target: self.debug_name(), "marking control flow edge live: {} -> {}: {change_result}",
            from.id(),
            to.id()
        );
    }

    /// Mark the entry blocks of the operation as executable.
    fn mark_entry_blocks_live(&self, op: &Operation, solver: &mut DataFlowSolver) {
        for region in op.regions() {
            if let Some(entry) = region.entry_block_ref() {
                let mut state =
                    solver.get_or_create_mut::<Executable, _>(ProgramPoint::at_start_of(entry));
                let change_result = state.change(|exec| exec.mark_live());
                log::trace!(target: self.debug_name(), "marking entry block {entry} live: {change_result}");
            }
        }
    }

    /// Visit the given call operation and compute any necessary lattice state.
    fn visit_call_operation(&self, call: &dyn CallOpInterface, solver: &mut DataFlowSolver) {
        log::debug!(target: self.debug_name(), "visiting call operation: {}", call.as_operation().name());

        // TODO: Update this when symbol table changes are complete, e.g. call.resolve_in_symbol_table(&self.symbol_table_collection)
        let targets = call.possible_callees();

        // A call to a externally-defined callable has unknown predecessors.
        let is_external_callable = |op: &Operation| -> bool {
            // A callable outside the analysis scope is an external callable.
            if !self.with_analysis_scope(|scope| scope.is_ancestor_of(op)) {
                return true;
            }
            // Otherwise, check if the callable region is defined.
            if let Some(callable) = op.as_trait::<dyn CallableOpInterface>() {
                callable.get_callable_region().is_none()
            } else {
                false
            }
        };

        // If the possible-callee set is unknown, mark the call ops predecessors as
        // overdefined/unknown
        let Some(targets) = targets else {
            let mut predecessors = solver
                .get_or_create_mut::<PredecessorState, _>(ProgramPoint::after(call.as_operation()));
            let change_result = predecessors.set_has_unknown_predecessors();
            log::debug!(
                target: self.debug_name(), "marking call-site return at {} as having unknown predecessors: {change_result}",
                call.as_operation()
            );
            return;
        };

        // TODO: Add support for non-symbol callables when necessary.
        //
        // Register this call site with every possible callee that is defined in the analysis
        // scope; the callees' return ops then register themselves back as predecessors of this
        // call's return point. A callee outside the scope (or without a body) cannot be
        // analyzed, so one such target makes the call's return conservatively unknown, even
        // while arguments still flow into the analyzable targets. An empty target set means the
        // call never transfers control (every dispatch traps), and registers nothing.
        let mut has_external_target = false;
        for callable in targets {
            let symbol = callable.as_symbol_ref();
            let callable = symbol.borrow();
            if !is_external_callable(callable.as_symbol_operation()) {
                // Add the live callsite
                let mut callsites = solver.get_or_create_mut::<PredecessorState, _>(
                    ProgramPoint::after(callable.as_symbol_operation()),
                );
                let change_result = callsites.change(|ps| {
                    ps.join_with_inputs(call.as_operation_ref(), call.arguments().as_value_range())
                });
                log::debug!(
                    target: self.debug_name(), "adding call-site {} to predecessor state for callee '{}': {change_result}",
                    call.as_operation(),
                    callable.name(),
                );
            } else {
                has_external_target = true;
            }
        }
        if has_external_target {
            // Mark this call op's predecessors as overdefined
            let mut predecessors = solver
                .get_or_create_mut::<PredecessorState, _>(ProgramPoint::after(call.as_operation()));
            let change_result = predecessors.change(|ps| ps.set_has_unknown_predecessors());
            log::debug!(
                target: self.debug_name(), "marking call-site return for external callable at {} as having unknown \
                 predecessors: {change_result}",
                call.as_operation()
            );
        }
    }

    /// Visit the given branch operation with successors and try to determine
    /// which are live from the current block.
    fn visit_branch_operation(&self, branch: &dyn BranchOpInterface, solver: &mut DataFlowSolver) {
        // Try to deduce a single successor for the branch.
        let Some(operands) = self.get_operand_values(branch.as_operation(), solver) else {
            log::trace!(target: self.debug_name(), "unable to prove liveness of successor blocks");
            return;
        };

        if let Some(successor) = branch.get_successor_for_operands(&operands) {
            let (from, to) = {
                let succ = successor.block.borrow();
                (succ.predecessor(), succ.successor())
            };
            let point = ProgramPoint::at_start_of(to);
            let mut predecessors = solver.get_or_create_mut::<PredecessorState, _>(point);
            let change_result = predecessors.change(|ps| {
                ps.join_with_inputs(branch.as_operation_ref(), successor.successor_operands())
            });
            log::debug!(
                target: self.debug_name(), "adding {} as predecessor for {point}: {change_result}",
                branch.as_operation().name()
            );
            self.mark_edge_live(&from.borrow(), &to.borrow(), solver);
        } else {
            // Otherwise, mark all successors as executable and outgoing edges.
            for successor in branch.successors().all() {
                let block_operand = successor.block.borrow();
                let from = block_operand.predecessor();
                let to = block_operand.successor();
                let point = ProgramPoint::at_start_of(to);
                let mut predecessors = solver.get_or_create_mut::<PredecessorState, _>(point);
                let change_result = predecessors.change(|ps| {
                    ps.join_with_inputs(branch.as_operation_ref(), successor.successor_operands())
                });
                log::debug!(
                    target: self.debug_name(), "adding {} as predecessor for {point}: {change_result}",
                    branch.as_operation().name()
                );
                self.mark_edge_live(&from.borrow(), &to.borrow(), solver);
            }
        }
    }

    /// Visit the given region branch operation, which defines regions, and
    /// compute any necessary lattice state. This also resolves the lattice state
    /// of both the operation results and any nested regions.
    fn visit_region_branch_operation(
        &self,
        branch: &dyn RegionBranchOpInterface,
        solver: &mut DataFlowSolver,
    ) {
        log::trace!(target: self.debug_name(), "visiting region branch operation: {}", branch.as_operation().name());

        // Try to deduce which regions are executable.
        let Some(operands) = self.get_operand_values(branch.as_operation(), solver) else {
            log::debug!(target: self.debug_name(), "unable to prove liveness of entry successor regions");
            return;
        };

        log::trace!(target: self.debug_name(), "processing entry successor regions");
        for successor in branch.get_entry_successor_regions(&operands) {
            // The successor can be either an entry block or the parent operation.
            let point = if let Some(succ) = successor.successor() {
                ProgramPoint::at_start_of(succ.borrow().entry_block_ref().unwrap())
            } else {
                ProgramPoint::after(branch.as_operation())
            };
            // Mark the entry block as executable.
            let mut state = solver.get_or_create_mut::<Executable, _>(point);
            let change_result = state.change(|exec| exec.mark_live());
            log::debug!(target: self.debug_name(), "marking region successor {point} live: {change_result}");
            // Add the parent op as a predecessor
            let mut predecessors = solver.get_or_create_mut::<PredecessorState, _>(point);
            let change_result = predecessors.change(|ps| {
                ps.join_with_inputs(branch.as_operation_ref(), successor.successor_inputs().iter())
            });
            log::debug!(
                target: self.debug_name(), "adding {} as predecessor for {point}: {change_result}",
                branch.as_operation().name()
            );
        }
    }

    /// Visit the given terminator operation that exits a region under an
    /// operation with control-flow semantics. These are terminators with no CFG
    /// successors.
    fn visit_region_terminator(
        &self,
        op: &Operation,
        branch: &dyn RegionBranchOpInterface,
        solver: &mut DataFlowSolver,
    ) {
        log::debug!(target: self.debug_name(), "visiting region terminator: {op}");
        let Some(operands) = self.get_operand_values(op, solver) else {
            log::debug!(target: self.debug_name(), "unable to prove liveness of region terminator successors");
            return;
        };

        let successors =
            if let Some(terminator) = op.as_trait::<dyn RegionBranchTerminatorOpInterface>() {
                let successors = terminator.get_successor_regions(&operands);
                RegionSuccessorIter::new(op, successors)
            } else {
                branch.get_successor_regions(RegionBranchPoint::Child(op.parent_region().unwrap()))
            };

        // Mark successor region entry blocks as executable and add this op to the list of
        // predecessors.
        for successor in successors {
            let (mut predecessors, point) = if let Some(region) = successor.successor() {
                let entry = region.borrow().entry_block_ref().unwrap();
                let point = ProgramPoint::at_start_of(entry);
                let mut state = solver.get_or_create_mut::<Executable, _>(point);
                let change_result = state.change(|exec| exec.mark_live());
                log::debug!(
                    target: self.debug_name(), "marking region successor {} entry {point} as live: {change_result}",
                    successor.branch_point()
                );
                (solver.get_or_create_mut::<PredecessorState, _>(point), point)
            } else {
                // Add this terminator as a predecessor to the parent op.
                let point = ProgramPoint::after(branch.as_operation());
                (solver.get_or_create_mut::<PredecessorState, _>(point), point)
            };
            let change_result = predecessors.change(|ps| {
                ps.join_with_inputs(op.as_operation_ref(), successor.successor_inputs().iter())
            });
            log::debug!(target: self.debug_name(), "adding {} as predecessor for {point}: {change_result}", op.name());
        }
    }

    /// Visit the given terminator operation that exits a callable region. These
    /// are terminators with no CFG successors.
    fn visit_callable_terminator(
        &self,
        op: &Operation,
        callable: &dyn CallableOpInterface,
        solver: &mut DataFlowSolver,
    ) {
        log::debug!(target: self.debug_name(), "visiting callable op terminator: {op}");
        // Add as predecessors to all callsites this return op.
        let callsites = solver.require::<PredecessorState, _>(
            ProgramPoint::after(callable.as_operation()),
            ProgramPoint::after(op),
        );
        let can_resolve = op.implements::<dyn ReturnLike>();
        for predecessor in callsites.known_predecessors().iter() {
            let predecessor = predecessor.borrow();
            assert!(predecessor.implements::<dyn CallOpInterface>());
            let point = ProgramPoint::after(&*predecessor);
            let mut predecessors = solver.get_or_create_mut::<PredecessorState, _>(point);
            if can_resolve {
                let change_result = predecessors.change(|ps| {
                    ps.join_with_inputs(op.as_operation_ref(), op.operands().as_value_range())
                });
                log::debug!(target: self.debug_name(), "adding {} as predecessor for {point}: {change_result}", op.name())
            } else {
                // If the terminator is not a return-like, then conservatively assume we can't
                // resolve the predecessor.
                let change_result = predecessors.change(|ps| ps.set_has_unknown_predecessors());
                log::debug!(target: self.debug_name(), "marking {point} as having unknown predecessors: {change_result}")
            }
        }
    }

    /// Get the constant values of the operands of the operation.
    ///
    /// Returns `None` if any of the operand lattices are uninitialized.
    fn get_operand_values(
        &self,
        op: &Operation,
        solver: &mut DataFlowSolver,
    ) -> Option<MaybeConstOperands> {
        get_operand_values(op, |value: &ValueRef| {
            let lattice = solver.get_or_create_mut::<Lattice<ConstantValue>, _>(*value);
            log::trace!(
                target: self.debug_name(), "subscribing to constant propagation changes of operand {value} (current={})",
                lattice.value()
            );
            AnalysisStateGuardMut::subscribe(&lattice, self);
            lattice
        })
    }

    /// Invoke a closure with the current analysis scope operation, or panic if no scope was set.
    #[inline]
    fn with_analysis_scope<F, T>(&self, mut callback: F) -> T
    where
        F: FnMut(&Operation) -> T,
    {
        let scope = self.analysis_scope.get().expect("expected analysis scope to be set");
        callback(&scope.borrow())
    }
}

/// Returns true if `op` is a returning terminator in a inter-region control flow op, or of a
/// callable region (i.e. return from a function).
fn is_region_or_callable_return(op: &Operation) -> bool {
    let block = op.parent();
    !op.has_successors()
        && block.is_some_and(|block| {
            let is_region_or_callable_op = block.grandparent().is_some_and(|parent_op| {
                let parent_op = parent_op.borrow();
                parent_op.implements::<dyn RegionBranchOpInterface>()
                    || parent_op.implements::<dyn CallableOpInterface>()
            });
            is_region_or_callable_op && block.borrow().terminator() == Some(op.as_operation_ref())
        })
}

/// Get the constant values of the operands of an operation.
///
/// If any of the constant value lattices are uninitialized, return None to indicate the analysis
/// should bail out.
fn get_operand_values<F>(op: &Operation, mut get_lattice: F) -> Option<MaybeConstOperands>
where
    F: FnMut(&ValueRef) -> AnalysisStateGuardMut<'_, Lattice<ConstantValue>>,
{
    let mut operands = SmallVec::<[Option<AttributeRef>; 2]>::with_capacity(op.num_operands());
    for operand in op.operands().all() {
        let operand = operand.borrow();
        let value = operand.as_value_ref();
        let lattice = get_lattice(&value);
        // If any of the operand's values are uninitialized, bail out.
        if lattice.value().is_uninitialized() {
            return None;
        }
        operands.push(lattice.value().constant_value());
    }
    Some(operands)
}

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString, vec::Vec};

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        CallOpInterface, ImmediateAttr, Op, OperationRef, ProgramPoint, SourceSpan, Symbol, Type,
        Usable, ValueRef,
        diagnostics::Uri,
        dialects::builtin::{Function, FunctionAlias, ModuleBuilder},
        parse::{self, ParserConfig},
        pass::AnalysisManager,
        testing::Test,
    };

    use super::*;
    use crate::{
        DataFlowConfig, DataFlowSolver, Lattice,
        analyses::{SparseConstantPropagation, constant_propagation::ConstantValue},
    };

    #[test]
    fn predecessor_state_orders_known_predecessors_by_ir_position() {
        let mut test = Test::new(
            "predecessor_state_orders_known_predecessors_by_ir_position",
            &[Type::U32, Type::U32],
            &[],
        );
        let (first_op, second_op, point) = {
            let mut builder = test.function_builder();
            let block = builder.current_block();
            let lhs = block.borrow().arguments()[0] as ValueRef;
            let rhs = block.borrow().arguments()[1] as ValueRef;
            let first = builder.add_unchecked(lhs, rhs, SourceSpan::UNKNOWN).unwrap();
            let first_op = first.borrow().get_defining_op().unwrap();
            let second = builder.add_unchecked(first, rhs, SourceSpan::UNKNOWN).unwrap();
            let second_op = second.borrow().get_defining_op().unwrap();

            (first_op, second_op, ProgramPoint::after(second_op))
        };

        let mut solver = DataFlowSolver::default();
        let mut predecessors = solver.get_or_create_mut::<PredecessorState, _>(point);
        predecessors.join(second_op);
        predecessors.join(first_op);

        assert_eq!(predecessors.known_predecessors(), &[first_op, second_op]);
    }

    fn analyze(source: &str) -> (Test, OperationRef, DataFlowSolver) {
        let test = Test::default();
        test.context().get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
        test.context().get_or_register_dialect::<midenc_dialect_arith::ArithDialect>();
        let module = parse::parse_any(
            ParserConfig::new(test.context_rc()),
            Uri::new("function_alias.hir"),
            source,
        )
        .expect("fixture must parse and verify");
        let mut config = DataFlowConfig::default();
        config.set_interprocedural(true);
        let mut solver = DataFlowSolver::new(config);
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        solver
            .initialize_and_run(&module.borrow(), AnalysisManager::new(module, None))
            .expect("analysis must converge");
        (test, module, solver)
    }

    fn alias_module(visibility: &str, extra_use: &str) -> alloc::string::String {
        format!(
            r#"
builtin.module public @test {{
    builtin.function private extern("C") @target(%x: u32) -> u32 {{
        builtin.ret %x : (u32);
    }};
    builtin.function_alias private @first -> @target;
    builtin.function_alias {visibility} @alias -> @first;
    builtin.function public extern("C") @caller() -> u32 {{
        {extra_use}
        %arg = arith.constant 42 : u32;
        %result = hir.exec @alias(%arg) : extern("C") (u32) -> u32;
        builtin.ret %result : (u32);
    }};
}};
"#
        )
    }

    /// Get the constant-propagated value of `value` or `None` if it is not a known constant.
    fn constant(solver: &DataFlowSolver, value: ValueRef) -> Option<u32> {
        let lattice = solver.get::<Lattice<ConstantValue>, _>(&value).expect("value has a lattice");
        assert!(!lattice.value().is_uninitialized(), "call flow must initialize the value");
        lattice
            .value()
            .constant_value()
            .map(|attr| attr.borrow().downcast_ref::<ImmediateAttr>().unwrap().as_u32().unwrap())
    }

    /// A chain of private aliases is fully resolved by the analysis: call/return flow and constant
    /// propagation work as if called directly, without rewriting any symbol references in the IR
    #[test]
    fn private_alias_chain_resolves_callsites_and_propagates_constants() {
        let (_test, module, solver) = analyze(&alias_module("private", ""));
        let mb = ModuleBuilder::new(
            module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
        );
        let target = mb.get_function("target").unwrap();
        let alias = mb.get_function_alias("alias").unwrap();
        let first = mb.get_function_alias("first").unwrap();
        let caller = mb.get_function("caller").unwrap();
        let call = caller
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .iter()
            .find_map(|op| op.downcast_ref::<midenc_dialect_hir::Exec>().map(Op::as_operation_ref))
            .unwrap();

        let callsites = solver
            .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
            .unwrap();
        assert!(callsites.all_predecessors_known(), "private aliases do not escape");
        assert_eq!(callsites.known_predecessors(), &[call]);
        let returns = solver.get::<PredecessorState, _>(&ProgramPoint::after(call)).unwrap();
        assert_eq!(
            returns.known_predecessors(),
            &[target.borrow().entry_block().borrow().terminator().unwrap(),]
        );
        assert_eq!(
            constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
            Some(42)
        );
        assert_eq!(constant(&solver, call.borrow().results()[0] as ValueRef), Some(42));

        // Analysis resolves identity without rewriting any of the alias symbol uses.
        assert_eq!(alias.borrow().iter_uses().count(), 1);
        assert_eq!(first.borrow().iter_uses().count(), 1);
        assert_eq!(target.borrow().iter_uses().count(), 1);
        assert!(alias.borrow().visibility().is_private());
        let call = call.borrow();
        let call = call.downcast_ref::<midenc_dialect_hir::Exec>().unwrap();
        assert_eq!(call.callee().path().to_string(), "alias");
        assert_eq!(call.callee().user().borrow().owner, call.as_operation_ref());
        assert_eq!(first.borrow().target().user().borrow().owner, first.as_operation_ref());
        assert_eq!(
            call.resolve().map(|c| c.as_symbol_ref()),
            target.borrow().as_operation().as_symbol_ref()
        );
    }

    #[test]
    fn public_alias_keeps_target_arguments_unknown() {
        let (_test, module, solver) = analyze(&alias_module("public", ""));
        let mb = ModuleBuilder::new(
            module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
        );
        let target = mb.get_function("target").unwrap();
        let state = solver
            .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
            .unwrap();
        assert!(!state.all_predecessors_known(), "external callers may use the public alias");
        assert_eq!(state.known_predecessors().len(), 1, "retain the known call through the alias");
        assert_eq!(
            constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
            None
        );
    }

    #[test]
    fn taking_alias_address_keeps_target_arguments_unknown() {
        let (_test, module, solver) = analyze(&alias_module(
            "private",
            "%root0, %root1, %root2, %root3 = hir.procedure_root @alias;",
        ));
        let mb = ModuleBuilder::new(
            module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
        );
        let target = mb.get_function("target").unwrap();
        let state = solver
            .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
            .unwrap();
        assert!(
            !state.all_predecessors_known(),
            "address-taking through an alias escapes its target"
        );
        assert_eq!(
            constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
            None
        );
    }

    #[test]
    fn indirect_alias_calls_share_target_and_return_flow() {
        let (_test, module, solver) = analyze(
            r#"
builtin.module public @test {
    builtin.function private extern("C") @target() -> u32 {
        %value = arith.constant 42 : u32;
        builtin.ret %value : (u32);
    };
    builtin.function_alias private @first -> @target;
    builtin.function_alias private @alias -> @first;
    builtin.function_table private @table : 2 {
        builtin.function_table_entry 0 @first tag 1;
        builtin.function_table_entry 1 @alias tag 1;
    };
    builtin.function public extern("C") @caller(%index: u32) -> u32 {
        %result = hir.exec_indirect @table[%index]() : extern("C") () -> u32 tag 1;
        builtin.ret %result : (u32);
    };
};
"#,
        );

        let mut calls = Vec::new();
        module.borrow().prewalk_all(|op| {
            if let Some(call) = op.downcast_ref::<midenc_dialect_hir::ExecIndirect>() {
                calls.push(call.as_operation_ref());
            }
        });
        assert_eq!(calls.len(), 1, "the fixture contains a single indirect call");
        let call_ref = calls[0];
        let call = call_ref.borrow();
        let call = call.downcast_ref::<midenc_dialect_hir::ExecIndirect>().unwrap();
        let callees = call.possible_callees().unwrap();
        assert_eq!(callees.len(), 1, "deduplicate after resolving aliases");

        let target_ref = callees[0].as_operation_ref();
        let target = target_ref.borrow();
        assert!(target.is::<Function>());
        assert!(!target.is::<FunctionAlias>());
        let entry = target.downcast_ref::<Function>().unwrap().entry_block();
        assert!(
            solver
                .get::<Executable, _>(&ProgramPoint::at_start_of(entry))
                .unwrap()
                .is_live()
        );

        let callsites =
            solver.get::<PredecessorState, _>(&ProgramPoint::after(target_ref)).unwrap();
        assert!(!callsites.all_predecessors_known(), "table entries take the aliases' address");
        assert_eq!(
            callsites.known_predecessors(),
            &[call_ref],
            "the indirect call registers as the only known call site of the shared target"
        );

        let returns = solver.get::<PredecessorState, _>(&ProgramPoint::after(call_ref)).unwrap();
        assert_eq!(
            returns.known_predecessors(),
            &[entry.borrow().terminator().unwrap()],
            "the target's return flows back to the indirect call"
        );
        assert_eq!(
            constant(&solver, call.results()[0] as ValueRef),
            Some(42),
            "constant is propagated"
        );
    }

    /// Dispatching through a function table takes the aliases' address, so the target may have
    /// callers the analysis cannot see and its arguments stay unknown even though the call site
    /// passes a constant.
    #[test]
    fn table_dispatch_through_alias_keeps_target_arguments_unknown() {
        let (_test, module, solver) = analyze(
            r#"
builtin.module public @test {
    builtin.function private extern("C") @target(%x: u32) -> u32 {
        builtin.ret %x : (u32);
    };
    builtin.function_alias private @first -> @target;
    builtin.function_alias private @alias -> @first;
    builtin.function_table private @table : 2 {
        builtin.function_table_entry 0 @first tag 1;
        builtin.function_table_entry 1 @alias tag 1;
    };
    builtin.function public extern("C") @caller(%index: u32) -> u32 {
        %arg = arith.constant 42 : u32;
        %result = hir.exec_indirect @table[%index](%arg) : extern("C") (u32) -> u32 tag 1;
        builtin.ret %result : (u32);
    };
};
"#,
        );

        let mb = ModuleBuilder::new(
            module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
        );
        let target = mb.get_function("target").unwrap();
        let state = solver
            .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
            .unwrap();
        assert!(!state.all_predecessors_known(), "table entries take the aliases' address");
        assert_eq!(
            state.known_predecessors().len(),
            1,
            "retain the known indirect call through the table"
        );
        assert_eq!(
            constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
            None,
            "unknown callers keep the target's arguments from becoming constant"
        );
    }
}
