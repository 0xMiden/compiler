pub(super) mod state;

pub use self::state::{
    AnalysisState, AnalysisStateGuard, AnalysisStateGuardMut, AnalysisStateInfo,
    AnalysisStateSubscription, AnalysisStateSubscriptionBehavior, BuildableAnalysisState, Revision,
};
use super::{DataFlowSolver, ProgramPoint};
use crate::{pass::AnalysisManager, Operation, Report};

/// Indicates whether the control enters, exits, or skips over the callee (in the case of
/// external functions).
#[derive(Debug, Copy, Clone)]
pub enum CallControlFlowAction {
    Enter,
    Exit,
    External,
}

/// [DataFlowAnalysis] is the base trait for all data-flow analyses.
///
/// In general, a data-flow analysis, is expected to visit the IR rooted at some operation, and in
/// the process, define state that represents its results, invoking transfer functions on that state
/// at each program point. In addition to its own state, it may also consume the states of other
/// analyses which it either requires, or may optimistically benefit from. In the process, a
/// dependency graph is established between the analyses being applied.
///
/// In classical data-flow analysis, the dependency graph is static and each analysis defines the
/// transfer functions between its input states and output states. In the data-flow framework
/// implemented here however, things work a bit differently:
///
/// * The set of analyses, and the dependencies between them, is dynamic and implicit.
/// * Multiple analyses can share the same state, so the transfer functions are defined as part of
///   the [AnalysisState] implementation, not the [DataFlowAnalysis] that consumes it. The analysis
///   is responsible for invoking the appropriate transfer function at each program point, but it
///   does not need to know how it is implemented.
///
/// Generally, when an analysis queries an uninitialized state, it is expected to "bail out", i.e.,
/// not provide any updates. When the value is initialized, the solver will re-invoke the analysis.
/// However, if the solver exhausts its worklist, and there are still uninitialized states, the
/// solver "nudges" the analyses by default-initializing those states.
pub trait DataFlowAnalysis {
    /// A friendly name for this analysis in diagnostics
    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }
    /// The unique type identifier of the concrete analysis type.
    fn analysis_id(&self) -> core::any::TypeId;
    /// Initialize the analysis from the provided top-level operation by building an initial
    /// dependency graph between all lattice anchors of interest. This can be implemented by calling
    /// `visit` on all program points of interest below the top-level operation.
    ///
    /// An analysis can optionally provide initial values to certain analysis states to influence
    /// the evolution of the analysis.
    fn initialize(
        &self,
        op: &Operation,
        solver: &mut DataFlowSolver,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report>;

    /// Visit the given program point.
    ///
    /// The solver will invoke this function when a state this analysis depends on has changed at
    /// the given program point.
    ///
    /// This function is similar to a transfer function - it queries analysis states that it depends
    /// on, and derives/computes other states.
    ///
    /// When an analysis state is queried (via [DataFlowSolver::require]), it establishes a
    /// dependency between this analysis, and that state, at a specific program point. As a result,
    /// this function will be invoked by the solver on that program point, if at any point in the
    /// future, the state changes.
    ///
    /// While dependencies between analysis states are generally handled automatically by the solver,
    /// implementations of an analysis may also explicitly add dependencies between some input state
    /// and a specific program point, to ensure that the solver will invoke the analysis on that
    /// program point if the input state changes.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report>;
}

/// This trait represents a type which is can be constructed into an instance of [DataFlowAnalysis]
/// by the [DataFlowSolver], by constructing an instance of its corresponding [AnalysisStrategy]
/// with an instance of the type. The strategy is responsible for adapting the specific semantics
/// of the analysis to the abstract [DataFlowAnalysis] interface.
///
/// There are two primary ways of categorizing analysis:
///
/// * dense vs sparse - dictates whether analysis states are anchored to program points (dense) or
///   values (sparse). Sparse analyses are referred to as such because SSA values can only have a
///   single definition, so the analysis state only has to be anchored to a single location per
///   value. Dense analyses on the other hand, must attach analysis state to every program point
///   (operation and block) visited by the analysis.
///
/// * forward vs backward - dictates whether analysis state is propagated forward (from the entry
///   point of a region to the exits of the region) or backward (from the exits of a region, to
///   the entry point). A forward analysis follows the CFG top-down, while a backward analysis
///   visits the CFG bottom-up.
///
/// As a result, there are four unique permutations of analysis possible, and each have different
/// semantics from the others, requiring separate traits. This trait allows loading analyses into
/// the [DataFlowSolver] without having to know the specific details of how that analysis is
/// implemented. Instead, the author of the analysis implements the specific analysis trait that
/// corresponds to the semantics it wants, and then implements this trait to specify the concrete
/// type that understands how to run that type of analysis in the context of our data-flow analysis
/// framework.
///
/// This must be separate from [DataFlowAnalysis] as the underlying type may not implement the
/// [DataFlowAnalysis] trait itself, only doing so once combined with the specified strategy.
/// However, it is expected that all concrete implementations of [DataFlowAnalysis] implement this
/// trait, enabling it to be loaded into the solver via [DataFlowSolver::load].
pub trait BuildableDataFlowAnalysis {
    /// The type which knows how to run `Self` as an instance of [DataFlowAnalysis].
    ///
    /// The dense and sparse analysis kinds have concrete types which handle the details which are
    /// universal to all such analyses. Those types would be specified as the strategy for a
    /// specific analysis of the corresponding kind (e.g. `SparseDataFlowAnalysis<T>` provides the
    /// implementation of [DataFlowAnalysis] for all sparse-(forward|backward) analyses.
    type Strategy: Sized + AnalysisStrategy<Self>;

    /// Construct a fresh instance of the underlying analysis type.
    ///
    /// The current [DataFlowSolver] instance is provided, allowing an implementation to access the
    /// global [DataFlowConfig], as well as load any other analyses that it depends on. Any analysis
    /// that has already been loaded prior to calling this function, will be ignored.
    fn new(solver: &mut DataFlowSolver) -> Self;
}

/// This trait represents a type that adapts some primitive analysis `T` to the abstract
/// [DataFlowAnalysis] interface.
///
/// It is intended to be used in conjunction with [BuildableDataFlowAnalysis]. See the documentation
/// of that trait for more details on how these work together and why they exist.
pub trait AnalysisStrategy<T: ?Sized>: DataFlowAnalysis {
    /// The kind (dense vs sparse) of the analysis being performed
    type Kind: AnalysisKind;
    /// The direction in which analysis state is propagated (forward vs backward) by this analysis.
    type Direction: AnalysisDirection;

    /// Construct a valid [DataFlowAnalysis] instance using this strategy, by providing an instance
    /// of the underlying analysis type.
    ///
    /// The current [DataFlowSolver] instance is provided, allowing an implementation to access the
    /// global [DataFlowConfig], as well as load any analyses that it depends on. Any analysis that
    /// has already been loaded prior to calling this function, will be ignored.
    ///
    /// The `analysis` instance is expected to have been constructed using
    /// [BuildableDataFlowAnalysis::new], if `T` implements the trait.
    fn build(analysis: T, solver: &mut DataFlowSolver) -> Self;
}

/// A marker trait for abstracting/specializing over the abstract kind of an analysis: dense or
/// sparse.
///
/// This trait is sealed as there are only two supported kinds.
#[allow(private_bounds)]
pub trait AnalysisKind: sealed::AnalysisKind {
    fn is_dense() -> bool {
        Self::IS_DENSE
    }
    fn is_sparse() -> bool {
        Self::IS_SPARSE
    }
}

impl<K: sealed::AnalysisKind> AnalysisKind for K {}

/// A marker trait for abstracting over the direction in which information is propagated by an
/// analysis: forward or backward.
///
/// This trait is sealed as there are only two possible directions.
#[allow(private_bounds)]
pub trait AnalysisDirection: sealed::AnalysisDirection {
    fn is_forward() -> bool {
        Self::IS_FORWARD
    }
    fn is_backward() -> bool {
        !Self::IS_FORWARD
    }
}

impl<D: sealed::AnalysisDirection> AnalysisDirection for D {}

mod sealed {
    pub(super) trait AnalysisKind {
        const IS_DENSE: bool;
        const IS_SPARSE: bool;
    }

    #[derive(Debug, Copy, Clone)]
    pub struct Dense;
    impl AnalysisKind for Dense {
        const IS_DENSE: bool = true;
        const IS_SPARSE: bool = false;
    }

    #[derive(Debug, Copy, Clone)]
    pub struct Sparse;
    impl AnalysisKind for Sparse {
        const IS_DENSE: bool = false;
        const IS_SPARSE: bool = true;
    }

    pub(super) trait AnalysisDirection {
        const IS_FORWARD: bool;
    }

    #[derive(Debug, Copy, Clone)]
    pub struct Forward;
    impl AnalysisDirection for Forward {
        const IS_FORWARD: bool = true;
    }

    #[derive(Debug, Copy, Clone)]
    pub struct Backward;
    impl AnalysisDirection for Backward {
        const IS_FORWARD: bool = false;
    }
}

pub use self::sealed::{Backward, Dense, Forward, Sparse};
