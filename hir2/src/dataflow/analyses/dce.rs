use core::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    ptr::NonNull,
};

use smallvec::SmallVec;

use super::constant_propagation::ConstantValue;
use crate::{
    adt::{SmallDenseMap, SmallSet},
    dataflow::{
        AnalysisQueue, AnalysisState, AnalysisStateGuardMut, AnalysisStateInfo,
        AnalysisStateSubscription, AnalysisStateSubscriptionBehavior, AnalysisStrategy,
        BuildableAnalysisState, BuildableDataFlowAnalysis, ChangeResult, DataFlowAnalysis,
        DataFlowSolver, Dense, Forward, Lattice, LatticeAnchor, LatticeAnchorRef, ProgramPoint,
    },
    pass::AnalysisManager,
    traits::{BranchOpInterface, ReturnLike},
    AttributeValue, Block, BlockRef, CallOpInterface, CallableOpInterface, EntityWithId, Operation,
    OperationRef, RegionBranchOpInterface, RegionBranchTerminatorOpInterface, Report, SourceSpan,
    Spanned, Symbol, SymbolManager, SymbolMap, SymbolTable, ValueRef,
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
        use crate::dataflow::solver::QueuedAnalysis;

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
        use crate::EntityWithId;
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
    fn new(_solver: &mut crate::dataflow::DataFlowSolver) -> Self {
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
    fn build(analysis: Self, _solver: &mut crate::dataflow::DataFlowSolver) -> Self {
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
        top: &crate::Operation,
        solver: &mut crate::dataflow::DataFlowSolver,
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
        solver: &mut crate::dataflow::DataFlowSolver,
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

type MaybeConstOperands = SmallVec<[Option<Box<dyn AttributeValue>>; 2]>;

impl DeadCodeAnalysis {
    /// Find and mark callable symbols with potentially unknown callsites as having overdefined
    /// predecessors. `top` is the top-level operation that the analysis is operating on.
    fn initialize_callable_symbols(&self, top: &Operation, solver: &mut DataFlowSolver) {
        log::trace!(target: self.debug_name(), "initializing callable symbols in '{}'", top.name());

        self.analysis_scope.set(Some(top.as_operation_ref()));

        let walk_fn = |sym_table: &dyn SymbolTable, all_uses_visible: bool| {
            let symbol_table_op = sym_table.as_symbol_table_operation();
            log::trace!(target: self.debug_name(), "analyzing symbol table '{}'", symbol_table_op.name());
            let symbol_table_region = symbol_table_op.region(0);
            let symbol_table_block = symbol_table_region.entry();

            let mut found_callable_symbol = false;
            for candidate in symbol_table_block.body().iter() {
                let Some(callable) = candidate.as_trait::<dyn CallableOpInterface>() else {
                    continue;
                };

                // We're only interested in callables with definitions, not declarations
                if callable.get_callable_region().is_none() {
                    continue;
                }

                // We're also only interested in callable symbols
                let Some(symbol) = callable.as_operation().as_trait::<dyn Symbol>() else {
                    continue;
                };

                // If a callable symbol has public visibility, or we are unable see all uses (for
                // example the address of a function is taken, but not called), then we have
                // potentially unknown callsites.
                let visibility = symbol.visibility();
                log::trace!(
                    target: self.debug_name(), "found callable symbol '{}' with visibility {visibility}",
                    symbol.name()
                );
                if visibility.is_public() || (!all_uses_visible && visibility.is_internal()) {
                    log::trace!(target: self.debug_name(), "marking symbol as having unknown predecessors");
                    let mut state = solver.get_or_create_mut::<PredecessorState, _>(
                        ProgramPoint::after(callable.as_operation()),
                    );
                    state.set_has_unknown_predecessors();
                }
                found_callable_symbol = true;
            }

            // Exit early if no eligible callable symbols were found in the table.
            if !found_callable_symbol {
                log::trace!(target: self.debug_name(), "no callable symbols found in this symbol table");
                return;
            }

            // Walk the symbol table to check for non-call uses of symbols.
            log::trace!(target: self.debug_name(), "looking for non-call uses of symbols in the symbol table region");
            let uses = Operation::all_symbol_uses_in_region(&symbol_table_region);
            let top_symbol_table = SymbolManager::from(top);
            for symbol_use in uses {
                let symbol_use = symbol_use.borrow();
                if symbol_use.owner.borrow().implements::<dyn CallOpInterface>() {
                    continue;
                }

                // If a callable symbol has a non-call use, then we can't be guaranteed to know all
                // callsites.
                let symbol_attr = symbol_use.symbol();
                log::trace!(
                    target: self.debug_name(), "found symbol use whose user does not implement CallOpInterface - marking \
                     symbol as having unknown predecessors"
                );
                if let Some(symbol) = top_symbol_table.lookup_symbol_ref(&symbol_attr.path) {
                    let mut state = solver
                        .get_or_create_mut::<PredecessorState, _>(ProgramPoint::after(symbol));
                    state.set_has_unknown_predecessors();
                }
            }
        };

        top.walk_symbol_tables(/*all_symbol_uses_visible=*/ top.parent().is_none(), walk_fn);
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
        let callable = call.resolve();

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

        // If the callable is unresolvable, mark the call ops predecessors as overdefined/unknown
        if callable.is_none() {
            let mut predecessors = solver
                .get_or_create_mut::<PredecessorState, _>(ProgramPoint::after(call.as_operation()));
            let change_result = predecessors.set_has_unknown_predecessors();
            log::debug!(
                target: self.debug_name(), "marking call-site return at {} as having unknown predecessors: {change_result}",
                call.as_operation()
            );
            return;
        }

        // TODO: Add support for non-symbol callables when necessary.
        //
        // If the callable has non-call uses we would mark as having reached pessimistic fixpoint,
        // otherwise allow for propagating the return values out.
        let callable = callable.unwrap();
        let callable = callable.borrow();
        // It the callable can have external callers we don't know about, we have to be conservative
        // about the set of possible predecessors.
        if !is_external_callable(callable.as_symbol_operation()) {
            // Add the live callsite
            let mut callsites = solver.get_or_create_mut::<PredecessorState, _>(
                ProgramPoint::after(callable.as_symbol_operation()),
            );
            let change_result = callsites.change(|ps| ps.join(call.as_operation_ref()));
            log::debug!(
                target: self.debug_name(), "adding call-site {} to predecessor state for its callee: {change_result}",
                call.as_operation()
            );
        } else {
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
            self.mark_edge_live(&from.borrow(), &to.borrow(), solver);
        } else {
            // Otherwise, mark all successors as executable and outgoing edges.
            for successor in branch.successors().all() {
                let block_operand = successor.block.borrow();
                let from = block_operand.predecessor();
                let to = block_operand.successor();
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

        let successors = if let Some(terminator) =
            op.as_trait::<dyn RegionBranchTerminatorOpInterface>()
        {
            let successors = terminator.get_successor_regions(&operands);
            crate::RegionSuccessorIter::new(op, successors)
        } else {
            branch
                .get_successor_regions(crate::RegionBranchPoint::Child(op.parent_region().unwrap()))
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
                let change_result = predecessors.change(|ps| ps.join(op.as_operation_ref()));
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
    let mut operands =
        SmallVec::<[Option<Box<dyn AttributeValue>>; 2]>::with_capacity(op.num_operands());
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
