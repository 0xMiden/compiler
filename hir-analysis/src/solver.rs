mod allocator;

use alloc::{collections::VecDeque, rc::Rc};
use core::{any::TypeId, cell::RefCell, ptr::NonNull};

use midenc_hir::{
    EntityRef, FxHashMap, Operation, ProgramPoint, Report, SmallVec, hashbrown,
    pass::AnalysisManager,
};

use self::{allocator::DataFlowSolverAlloc, analysis::AnalysisStrategy};
use super::{
    analysis::state::{AnalysisStateDescriptor, AnalysisStateInfo, AnalysisStateKey},
    *,
};

pub type AnalysisQueue = VecDeque<QueuedAnalysis, DataFlowSolverAlloc>;

/// The [DataFlowSolver] is responsible for running a collection of [DataFlowAnalysis] against a
/// specific operation in the IR, such that the analyses reach a fixpoint state.
///
/// To do so, it maintains the storage for all analysis states, as well as a dependency graph which
/// is used to re-run analyses which are affected by changes to states they depend on. Every
/// [DataFlowAnalysis] implementation interacts with the solver in order to create their analysis
/// state, and request those of their dependencies. This enables the solver to reason about when
/// changes require further re-analysis to be performed - hence why it is called the "solver".
pub struct DataFlowSolver {
    /// Global configuration for the data-flow analysis being performed
    config: DataFlowConfig,
    /// The queue of dependent analyses that need to be re-applied due to changes.
    ///
    /// This works a bit like a channel primitive: any analysis states that are being mutated by
    /// the currently executing analysis will hold a reference to this queue in their
    /// [AnalysisStateGuard], so that when/if the underlying state changes, dependent analyses can
    /// be enqueued in this worklist without going through the solver.
    worklist: Rc<RefCell<AnalysisQueue>>,
    /// The set of loaded analyses maintained by the solver
    ///
    /// This set is consumed during `initialize_and_run`, to use the solver multiple times, you must
    /// ensure you re-load all the analyses you wish to run.
    child_analyses: SmallVec<[NonNull<dyn DataFlowAnalysis>; 8]>,
    /// Metadata about each unique [AnalysisState] implementation type which has had at least one
    /// instance created by the solver.
    ///
    /// The metadata for a given type is shared between all instances of that type, to avoid the
    /// overhead of allocating the data many times. It is used primarily for constructing pointers
    /// to state from the raw type-erased `AnalysisStateInfo` record.
    analysis_state_impls: FxHashMap<TypeId, NonNull<AnalysisStateDescriptor>>,
    /// The analysis states being tracked by the solver.
    ///
    /// Each key in this map represents the type of analysis state and its lattice anchor, i.e. the
    /// thing to which the analysis state is attached. The value is a pointer to the state itself,
    /// and will change over time as analyses are re-applied.
    ///
    /// This map also manages the implicit dependency graph between analyses and specific analysis
    /// states. When an analysis requires a given state at a given program point, an entry is added
    /// to the [AnalysisStateInfo] record which tracks the analysis and the dependent program point.
    ///
    /// When changes are later made to an analysis state, any dependent analyses are re-enqueued in
    /// the solver work queue, so that affected program points are re-analyzed.
    analysis_state: Rc<RefCell<FxHashMap<AnalysisStateKey, NonNull<AnalysisStateInfo>>>>,
    /// Uniqued [LatticeAnchor] values.
    ///
    /// Each lattice anchor will only be allocated a single time, uniqueness is established via Hash
    anchors: RefCell<FxHashMap<u64, LatticeAnchorRef>>,
    /// The current analysis being executed.
    current_analysis: Option<NonNull<dyn DataFlowAnalysis>>,
    /// A bump-allocator local to the solver, in which it allocates analyses, analysis states, and
    /// ad-hoc lattice anchors.
    ///
    /// Most data required by the solver gets allocated here, the exceptions are limited to certain
    /// data structures without custom allocator support, or ad-hoc items which we don't want
    /// attached to the solver lifetime.
    alloc: DataFlowSolverAlloc,
}
impl Default for DataFlowSolver {
    fn default() -> Self {
        Self::new(Default::default())
    }
}
impl DataFlowSolver {
    /// Create a new solver instance with the provided configuration
    pub fn new(config: DataFlowConfig) -> Self {
        let alloc = DataFlowSolverAlloc::default();
        let worklist = Rc::new(RefCell::new(VecDeque::with_capacity_in(64, alloc.clone())));
        Self {
            config,
            alloc,
            worklist,
            child_analyses: Default::default(),
            analysis_state_impls: Default::default(),
            analysis_state: Default::default(),
            anchors: Default::default(),
            current_analysis: None,
        }
    }

    /// Access the current solver configuration
    #[inline]
    pub fn config(&self) -> &DataFlowConfig {
        &self.config
    }

    /// Load an analysis of type `A` into the solver.
    ///
    /// This uses the information provided by the [BuildableDataFlowAnalysis] implementation to
    /// construct a valid instance of the [DataFlowAnalysis] interface which the solver will use
    /// to run the analysis.
    ///
    /// In particular, an instance of `A` is created using [BuildableDataFlowAnalysis::new], and
    /// then calls [Self::load_with_strategy] to instantiate the actual [DataFlowAnalysis]
    /// implementation, by using the associated [BuildableDataFlowAnalysis::Strategy] type.
    ///
    /// # Panics
    ///
    /// This function will panic if you attempt to load new analyses while the solver is running.
    /// It is only permitted to load analyses before calling [initialize_and_run], or after a call
    /// to that function has returned, and you are starting a new round of analysis.
    pub fn load<A>(&mut self)
    where
        A: BuildableDataFlowAnalysis + 'static,
    {
        let analysis = <A as BuildableDataFlowAnalysis>::new(self);
        self.load_with_strategy(analysis)
    }

    /// Load `analysis` into the solver.
    ///
    /// Since `analysis` might not implement [DataFlowAnalysis] itself, we must obtain an
    /// implementation using the strategy type given via [BuildableDataFlowAnalysis::Strategy].
    /// Specifically, we invoke [AnalysisStrategy::build], passing in `analysis` as the underlying
    /// implementation.
    ///
    /// Once instantiated, the resulting [DataFlowAnalysis] implementation is loaded into the solver
    /// using [Self::load_analysis].
    ///
    /// # Panics
    ///
    /// This function will panic if you attempt to load new analyses while the solver is running.
    /// It is only permitted to load analyses before calling [initialize_and_run], or after a call
    /// to that function has returned, and you are starting a new round of analysis.
    pub fn load_with_strategy<A>(&mut self, analysis: A)
    where
        A: BuildableDataFlowAnalysis + 'static,
    {
        let analysis = <<A as BuildableDataFlowAnalysis>::Strategy as AnalysisStrategy<A>>::build(
            analysis, self,
        );
        self.load_analysis(analysis);
    }

    /// Load `analysis` into the solver.
    ///
    /// The provided analysis is stored in a queue until [DataFlowSolver::initialize_and_run] is
    /// invoked.
    ///
    /// If an attempt is made to load the same analysis type twice while a previous instance is
    /// still pending, the load is a no-op.
    ///
    /// # Panics
    ///
    /// This function will panic if you attempt to load new analyses while the solver is running.
    /// It is only permitted to load analyses before calling [initialize_and_run], or after a call
    /// to that function has returned, and you are starting a new round of analysis.
    pub fn load_analysis<A>(&mut self, analysis: A)
    where
        A: DataFlowAnalysis + 'static,
    {
        assert!(
            self.worklist.borrow().is_empty() && self.current_analysis.is_none(),
            "it is not permitted to load analyses while the solver is running!"
        );
        let type_id = analysis.analysis_id();
        let already_loaded = self
            .child_analyses
            .iter()
            .any(|a| unsafe { a.as_ref().analysis_id() == type_id });
        if !already_loaded {
            let analysis = unsafe { NonNull::new_unchecked(self.alloc.put(analysis)) };
            self.child_analyses.push(analysis as NonNull<dyn DataFlowAnalysis>);
        }
    }

    /// Run the solver on `op`.
    ///
    /// It is expected that the caller has called [Self::load] for each analysis they wish to have
    /// applied. If no analyses have been loaded, this function returns `Ok` immediately, i.e. it
    /// is a no-op.
    ///
    /// This first initializes all of the analysis loaded via [Self::load], places them in a work
    /// queue, and then runs them to fixpoint by invoking [DataFlowAnalysis::visit].
    ///
    /// It is expected that the loaded analyses will create and attach states to anchors in the
    /// program rooted at `op` as the output of the analysis. These can then be requested by other
    /// analyses, or by the caller after this function returns.
    ///
    /// When a dependent analysis requires a specific analysis state for some anchor, it implicitly
    /// subscribes them to changes to that state by subsequent analyses. If such changes occur, the
    /// dependent analyses are re-enqueued, and the process starts again. Assuming well-formed
    /// data-flow analyses, this process is guaranteed to reach fixpoint. Currently, we do not
    /// impose a limit on the number of iterations performed, though we may introduce such limits,
    /// or other forms of sanity checks in the future.
    #[track_caller]
    pub fn initialize_and_run(
        &mut self,
        op: &Operation,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        // If we have no analyses, there is nothing to do
        if self.child_analyses.is_empty() {
            // Log a warning when this happens, since the calling code might benefit from not
            // even instantiating the solver in the first place.
            let location = core::panic::Location::caller();
            log::warn!(target: "dataflow-solver", "dataflow solver was run without any loaded analyses at {location}");
            return Ok(());
        }

        self.analyze(op, analysis_manager.clone())?;
        self.run_to_fixpoint()
    }

    /// Run the initial analysis of all loaded analyses.
    ///
    /// This is the point at which analyses are first applied to the top-level operation, `op`, and
    /// is also when dependencies between analyses are recorded in the analysis state dependency
    /// graph.
    ///
    /// Once initialization is complete, every analysis has been run exactly once, but some may have
    /// been re-enqueued due to dependencies on analysis states which changed during initialization.
    fn analyze(&mut self, op: &Operation, analysis_manager: AnalysisManager) -> Result<(), Report> {
        log::debug!(target: "dataflow-solver", "initializing loaded analyses");

        for mut analysis in core::mem::take(&mut self.child_analyses) {
            // priming analysis {analysis.debug_name()}
            assert!(self.current_analysis.is_none());
            self.current_analysis = Some(analysis);
            unsafe {
                let analysis = analysis.as_mut();
                log::debug!(target: analysis.debug_name(), "initializing analysis");
                analysis.initialize(op, self, analysis_manager.clone())?;
                log::debug!(target: analysis.debug_name(), "initialized successfully");
            }
            self.current_analysis = None;
        }

        log::debug!(target: "dataflow-solver", "initialization complete!");

        Ok(())
    }

    /// Run analysis to fixpoint.
    ///
    /// As mentioned in the docs of [Self::analyze], the initial analysis of the top-level operation
    /// may have established dependencies some analyses and the analysis states produced by others
    /// at specific program points. If any changes were made to analysis states for which there are
    /// dependent analyses, the dependents will have been re-enqueued in the solver's work queue.
    ///
    /// This function is responsible for consuming notifications from the work queue, indicating
    /// that a specific analysis should be re-applied at a given program point. This, in turn, may
    /// result in further re-analysis work.
    ///
    /// # Expected Behavior
    ///
    /// While the process described in the previous section seems like it could easily end up
    /// cycling indefinitely, re-enqueing the same analyses over and over again due to conflicts,
    /// this is not actually a risk, due to the properties of a well-formed data-flow analysis:
    ///
    /// A "well-formed"" data-flow analysis must adhere to certain rules, and those rules guarantee
    /// us that this process must reach a fixpoint, and in a bounded amount of time:
    ///
    /// A state of a data-flow analysis is required to be a valid instance of one of the following:
    ///
    /// * A join-semilattice (forward data-flow analysis)
    /// * A meet-semilattice (backward data-flow analysis)
    /// * A lattice (i.e. both a join- and meet- semilattice)
    ///
    /// Specifically this requires the following properties to be upheld:
    ///
    /// * The analysis state has a most-minimal value (i.e. under-specified, unknown, bottom)
    /// * The analysis state has a most-maximal value (i.e. over-specified, conflict, top)
    /// * The _join_ of two instances of the analysis state, produces a new state which is either
    ///   equal to the old states, or the least upper bound of the two states.
    /// * The _meet_ of two instances of the analysis state, produces a new state which is either
    ///   equal to the old states, or the greatest lower bound  of the two states.
    /// * The _join_ and _meet_ operations must be commutative, associative, and idempotent
    ///
    /// With this in mind, it becomes obvious how fixpoint is a guarantee:
    ///
    /// * Each change (via meet or join) to the analysis state, by definition, produces a new state
    ///   which is either unchanged, or has a value which is the greatest lower (or least upper)
    ///   bound of the input states. Thus changes always move in a single direction, either most-
    ///   maximal (or most-minimal).
    /// * As a result, an analysis that is re-enqueued due to a changed analysis state, is
    ///   guaranteed to observe a new, unique state. No further changes are possible after a state
    ///   reaches its most-maximal (or most-minimal) representation.
    ///
    /// For example, integer range analysis adheres to these axioms, as integer ranges form a
    /// partial order for which the `max` and `min` operators are commutative, associative, and
    /// idemptoent. A value for which we do not know any bounds, is in the most-minimal state, since
    /// the range must be treated as unbounded, i.e. it is under-specified. A value for which we know
    /// only a lower bound for, is strictly less specific than a value for which we know both a lower
    /// and upper bound for, and bounds can be further refined as analysis proceeds, potentially all
    /// the way until it is determined that the range of a value is fixed to a single integral
    /// value. The "most-maximal" state in this analysis however, is a conflict, i.e. the value is
    /// over specified, because we are able to observe a counter-example.
    fn run_to_fixpoint(&mut self) -> Result<(), Report> {
        log::debug!(target: "dataflow-solver", "running queued dataflow analyses to fixpoint..");

        // Run the analysis until fixpoint
        while let Some(QueuedAnalysis {
            point,
            mut analysis,
        }) = {
            let mut worklist = self.worklist.borrow_mut();
            worklist.pop_front()
        } {
            self.current_analysis = Some(analysis);
            unsafe {
                let analysis = analysis.as_mut();
                log::debug!(target: analysis.debug_name(), "running analysis at {point}");
                analysis.visit(&point, self)?;
            }
            self.current_analysis = None;
        }

        Ok(())
    }

    /// Allocate a custom [LatticeAnchor] with this solver
    ///
    /// NOTE: The resulting [LatticeAnchorRef] has a lifetime that is implicitly bound to that of
    /// this solver. It is unlikely you would ever have a reason to dereference an anchor after the
    /// solver is destroyed, but it is undefined behavior to do so. See [LatticeAnchorRef] for more.
    pub fn create_lattice_anchor<A>(&self, anchor: A) -> LatticeAnchorRef
    where
        A: LatticeAnchorExt,
    {
        LatticeAnchorRef::intern(&anchor, &self.alloc, &mut self.anchors.borrow_mut())
    }

    /// Get the [AnalysisState] attached to `anchor`, or `None` if not available.
    ///
    /// This does _not_ add an edge to the analysis state dependency graph for the current analysis,
    /// as it is assumed that the analysis state is not a required dependency, but an optional one.
    /// If you wish to be notified of changes to a specific analysis state, you should use
    /// [Self::require].
    pub fn get<T, A>(&self, anchor: &A) -> Option<EntityRef<'_, T>>
    where
        T: BuildableAnalysisState,
        A: LatticeAnchorExt + Copy,
    {
        let anchor = self.create_lattice_anchor::<A>(*anchor);
        let key = AnalysisStateKey::new::<T>(anchor);
        let analysis_state_info_ptr = self.analysis_state.borrow().get(&key).copied()?;
        Some(unsafe {
            let info = analysis_state_info_ptr.as_ref();
            info.borrow_state::<T>()
        })
    }

    /// Get the [AnalysisState] attached to `anchor`, or allocate a default instance if not yet
    /// created.
    ///
    /// This is expected to be used by the current analysis to initialize state it needs. The
    /// resulting handle represents an immutable borrow of the analysis state.
    ///
    /// Because the current analysis "owns" this state in a sense, albeit readonly, it is not
    /// treated as a dependent of this state, i.e. no edge is added to the analysis state
    /// dependency graph for the current analysis, and is instead left up to the caller, to
    /// avoid unintentional cyclical dependencies.
    ///
    /// This function returns an [AnalysisStateGuard], which guards the immutable reference to
    /// the underlying [AnalysisState].
    #[track_caller]
    pub fn get_or_create<'a, T, A>(&mut self, anchor: A) -> AnalysisStateGuard<'a, T>
    where
        T: BuildableAnalysisState,
        A: LatticeAnchorExt,
    {
        use hashbrown::hash_map::Entry;

        log::trace!(target: "dataflow-solver", "computing analysis state entry key");
        log::trace!(target: "dataflow-solver", "    loc       = {}", core::panic::Location::caller());
        log::trace!(target: "dataflow-solver", "    anchor    = {anchor}");
        log::trace!(target: "dataflow-solver", "    anchor ty = {}", core::any::type_name::<A>());
        let anchor = self.create_lattice_anchor::<A>(anchor);
        log::trace!(target: "dataflow-solver", "    anchor id = {}", anchor.anchor_id());
        let key = AnalysisStateKey::new::<T>(anchor);
        log::trace!(target: "dataflow-solver", "    key       = {key:?}");
        log::trace!(target: "dataflow-solver", "    lattice   = {}", core::any::type_name::<T>());
        match self.analysis_state.borrow_mut().entry(key) {
            Entry::Occupied(entry) => {
                log::trace!(target: "dataflow-solver", "found existing analysis state entry");
                let info = *entry.get();
                unsafe { AnalysisStateGuard::<T>::new(info) }
            }
            Entry::Vacant(entry) => {
                log::trace!(target: "dataflow-solver", "creating new analysis state entry");
                use crate::analysis::state::RawAnalysisStateInfo;
                let raw_info = RawAnalysisStateInfo::<T>::alloc(
                    &self.alloc,
                    &mut self.analysis_state_impls,
                    key,
                    anchor,
                );
                let info = RawAnalysisStateInfo::as_info_ptr(raw_info);
                entry.insert(info);
                unsafe { AnalysisStateGuard::<T>::new(info) }
            }
        }
    }

    /// Get the [AnalysisState] attached to `anchor`, or allocate a default instance if not yet
    /// created.
    ///
    /// This is expected to be used by the current analysis to write changes it computes. In a
    /// sense, this implies ownership by the current analysis - however in some cases multiple
    /// analyses share ownership over some state (i.e. they can all make changes to it). Any
    /// writer to some state is considered an owner for our purposes here.
    ///
    /// Because the current analysis "owns" this state, it is not treated as a dependent of this
    /// state, i.e. no edge is added to the analysis state dependency graph for the current
    /// analysis. This is because the current analysis will necessarily be changing the state, and
    /// thus re-enqueueing it when changes occur would result in a cyclical dependency on itself.
    ///
    /// Instead, it is expected that the current analysis will be re-enqueued only if it depends
    /// on _other_ analyses which run and make changes to their states of which this analysis
    /// is a dependent.
    ///
    /// This function returns an [AnalysisStateGuardMut], which guards both the mutable reference
    /// to this solver, as well as a mutable reference to the underlying [AnalysisState]. When the
    /// guard is dropped (or consumed to produce an immutable reference), it will have the solver
    /// re-enqueue any dependent analyses, if changes were made to the state since they last
    /// observed it. See the docs of [AnalysisStateGuardMut] for more details on proper usage.
    #[track_caller]
    pub fn get_or_create_mut<'a, T, A>(&mut self, anchor: A) -> AnalysisStateGuardMut<'a, T>
    where
        T: BuildableAnalysisState,
        A: LatticeAnchorExt,
    {
        use hashbrown::hash_map::Entry;

        log::trace!(target: "dataflow-solver", "computing analysis state entry key");
        log::trace!(target: "dataflow-solver", "    loc       = {}", core::panic::Location::caller());
        log::trace!(target: "dataflow-solver", "    anchor    = {anchor}");
        log::trace!(target: "dataflow-solver", "    anchor ty = {}", core::any::type_name::<A>());
        let anchor = self.create_lattice_anchor::<A>(anchor);
        log::trace!(target: "dataflow-solver", "    anchor id = {}", anchor.anchor_id());
        let key = AnalysisStateKey::new::<T>(anchor);
        log::trace!(target: "dataflow-solver", "    key       = {key:?}");
        log::trace!(target: "dataflow-solver", "    lattice   = {}", core::any::type_name::<T>());
        match self.analysis_state.borrow_mut().entry(key) {
            Entry::Occupied(entry) => {
                log::trace!(target: "dataflow-solver", "found existing analysis state entry");
                let info = *entry.get();
                unsafe { AnalysisStateGuardMut::<T>::new(info, self.worklist.clone()) }
            }
            Entry::Vacant(entry) => {
                log::trace!(target: "dataflow-solver", "creating new analysis state entry");
                use crate::analysis::state::RawAnalysisStateInfo;
                let raw_info = RawAnalysisStateInfo::<T>::alloc(
                    &self.alloc,
                    &mut self.analysis_state_impls,
                    key,
                    anchor,
                );
                let info = RawAnalysisStateInfo::as_info_ptr(raw_info);
                entry.insert(info);
                unsafe { AnalysisStateGuardMut::<T>::new(info, self.worklist.clone()) }
            }
        }
    }

    /// Get the [AnalysisState] attached to `anchor`, indicating to the solver that it is required
    /// by the current analysis at `dependent`, the program point at which the dependency is needed.
    ///
    /// In addition to returning the requested state, this function also adds an edge to the
    /// analysis state dependency graph for the current analysis, so that any changes to the
    /// state at `anchor` by later analyses, will cause the current analysis to be re-run at
    /// the given program point.
    ///
    /// If an instance of the requested state has not been created yet, a default one is allocated
    /// and returned. Typically, the resulting state will not be very useful, however, as mentioned
    /// above, this analysis will be re-run if the state is ever modified, at which point it may
    /// be able to do something more useful with the results.
    #[track_caller]
    pub fn require<'a, T, A>(
        &mut self,
        anchor: A,
        dependent: ProgramPoint,
    ) -> AnalysisStateGuard<'a, T>
    where
        T: BuildableAnalysisState,
        A: LatticeAnchorExt,
    {
        use hashbrown::hash_map::Entry;

        use crate::{
            AnalysisStateSubscriptionBehavior,
            analysis::state::{RawAnalysisStateInfo, RawAnalysisStateInfoHandle},
        };

        log::trace!(target: "dataflow-solver", "computing analysis state entry key");
        log::trace!(target: "dataflow-solver", "    loc       = {}", core::panic::Location::caller());
        log::trace!(target: "dataflow-solver", "    anchor    = {anchor}");
        log::trace!(target: "dataflow-solver", "    anchor ty = {}", core::any::type_name::<A>());
        let anchor = self.create_lattice_anchor::<A>(anchor);
        log::trace!(target: "dataflow-solver", "    anchor id = {}", anchor.anchor_id());
        let key = AnalysisStateKey::new::<T>(anchor);
        log::trace!(target: "dataflow-solver", "    key       = {key:?}");
        log::trace!(target: "dataflow-solver", "    lattice   = {}", core::any::type_name::<T>());
        log::trace!(target: "dataflow-solver", "    dependent = {dependent}");
        let (info, mut handle) = match self.analysis_state.borrow_mut().entry(key) {
            Entry::Occupied(entry) => {
                log::trace!(target: "dataflow-solver", "found existing analysis state entry");
                let info = *entry.get();
                (info, unsafe { RawAnalysisStateInfoHandle::new(info) })
            }
            Entry::Vacant(entry) => {
                log::trace!(target: "dataflow-solver", "creating new analysis state entry");
                let raw_info = RawAnalysisStateInfo::<T>::alloc(
                    &self.alloc,
                    &mut self.analysis_state_impls,
                    key,
                    anchor,
                );
                let info = RawAnalysisStateInfo::as_info_ptr(raw_info);
                entry.insert(info);
                (info, unsafe { RawAnalysisStateInfoHandle::new(info) })
            }
        };

        let current_analysis = self.current_analysis.unwrap();
        handle.with(|state, info| {
            <T as AnalysisStateSubscriptionBehavior>::on_require_analysis(
                state,
                info,
                current_analysis,
                dependent,
            );
        });

        unsafe { AnalysisStateGuard::new(info) }
    }

    /// Erase any cached analysis states attached to `anchor`
    pub fn erase_state<A>(&mut self, anchor: &A)
    where
        A: LatticeAnchorExt,
    {
        let anchor_id = anchor.anchor_id();
        self.analysis_state.borrow_mut().retain(|_, v| unsafe {
            let analysis_anchor_id = v.as_ref().anchor_ref().anchor_id();
            analysis_anchor_id == anchor_id
        });
    }
}

/// Represents an analysis that has derived facts at a specific program point from the state of
/// another analysis, that has since changed. As a result, the dependent analysis must be re-applied
/// at that program point to determine if the state changes have any effect on the state of its
/// previous analysis.
#[derive(Copy, Clone)]
pub struct QueuedAnalysis {
    /// The dependent program point
    pub point: ProgramPoint,
    /// The dependent analysis
    pub analysis: NonNull<dyn DataFlowAnalysis>,
}

impl Eq for QueuedAnalysis {}

impl PartialEq for QueuedAnalysis {
    fn eq(&self, other: &Self) -> bool {
        self.point == other.point
            && core::ptr::addr_eq(self.analysis.as_ptr(), other.analysis.as_ptr())
    }
}
