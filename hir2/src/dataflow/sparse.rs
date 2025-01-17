mod backward;
mod forward;
mod lattice;

pub use self::{
    backward::{set_all_to_exit_states, SparseBackwardDataFlowAnalysis},
    forward::{set_all_to_entry_states, SparseForwardDataFlowAnalysis},
    lattice::SparseLattice,
};
use super::{
    AnalysisStrategy, Backward, DataFlowAnalysis, DataFlowSolver, Forward, ProgramPoint, Sparse,
};
use crate::{pass::AnalysisManager, Operation, Report};

/// This type provides an [AnalysisStrategy] for sparse data-flow analyses.
///
/// In short, it implements the [DataFlowAnalysis] trait, and handles all of the boilerplate that
/// any well-structured sparse data-flow analysis requires. Analyses can make use of this strategy
/// by implementing one of the sparse data-flow analysis traits, which [SparseDataFlowAnalysis] will
/// use to delegate analysis-specific details to the analysis implementation. The two traits are:
///
/// * [SparseForwardDataFlowAnalysis], for forward-propagating sparse analyses
/// * [SparseBackwardDataFlowAnalysis], for backward-propagating sparse analyses
///
/// ## What is a sparse analysis?
///
/// A sparse data-flow analysis is one which associates analysis state with SSA value definitions,
/// in order to represent known facts about those values, either as a result of deriving them from
/// previous values used as operands of the defining operation (forward analysis), or as a result of
/// deriving them based on how the value is used along all possible paths that execution might take
/// (backward analysis). The state associated with a value does not change as a program executes,
/// it is fixed at the value definition, derived only from the states of other values and the
/// defining op itself.
///
/// This is in contrast to dense data-flow analysis, which associates state with program points,
/// which then evolves as the program executes. This is also where the distinction between _dense_
/// and _sparse_ comes from - program points are dense, while SSA value definitions are sparse
/// (insofar as the state associated with an SSA value only ever occurs once, while states
/// associated with program points are duplicated at each point).
///
/// Some examples of sparse analyses:
///
/// * Constant propagation - if a value is determined to be constant, the constant value is the
///   state associated with a given value definition. This determination is made based on the
///   semantics of an operation and its operands (i.e. if an operation can be constant-folded, then
///   the results of that operation are themselves constant). This is a forward analysis.
/// * Dead value analysis - determines whether or not a value is ever used. This is a backward
///   analysis, as it propagates uses to definitions. In our IR, we do not require this analysis,
///   as it is implicit in the use-def graph, however the concept is what we're interested in here.
///
/// ## Usage
///
/// This type is meant to be used indirectly, as an [AnalysisStrategy] implementation, rather than
/// directly as a [DataFlowAnalysis] implementation, as shown below:
///
/// ```rust,ignore
/// use midenc_hir2::dataflow::*;
///
/// #[derive(Default)]
/// pub struct MyAnalysis;
/// impl BuildableDataFlowAnalysis for MyAnalysis {
///     type Strategy = SparseDataFlowAnalysis<Self, Forward>;
///
///     fn new(_solver: &mut DataFlowSolver) -> Self {
///         Self
///     }
/// }
/// impl SparseForwardDataFlowAnalysis for MyAnalysis {
///     type Lattice = Lattice<u32>;
///
///     //...
/// }
/// ```
///
/// The above permits us to load `MyAnalysis` into a `DataFlowSolver` without ever mentioning the
/// `SparseDataFlowAnalysis` type at all, like so:
///
/// ```rust,ignore
/// let mut solver = DataFlowSolver::default();
/// solver.load::<MyAnalysis>();
/// solver.initialize_and_run(&op, analysis_manager);
/// ```
pub struct SparseDataFlowAnalysis<A, D> {
    analysis: A,
    _direction: core::marker::PhantomData<D>,
}

impl<A: SparseForwardDataFlowAnalysis> AnalysisStrategy<A> for SparseDataFlowAnalysis<A, Forward> {
    type Direction = Forward;
    type Kind = Sparse;

    fn build(analysis: A, _solver: &mut DataFlowSolver) -> Self {
        Self {
            analysis,
            _direction: core::marker::PhantomData,
        }
    }
}

impl<A: SparseBackwardDataFlowAnalysis> AnalysisStrategy<A>
    for SparseDataFlowAnalysis<A, Backward>
{
    type Direction = Backward;
    type Kind = Sparse;

    fn build(analysis: A, _solver: &mut DataFlowSolver) -> Self {
        Self {
            analysis,
            _direction: core::marker::PhantomData,
        }
    }
}

impl<A: SparseForwardDataFlowAnalysis> DataFlowAnalysis for SparseDataFlowAnalysis<A, Forward> {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    /// Initialize the analysis by visiting every owner of an SSA value: all operations and blocks.
    fn initialize(
        &self,
        top: &Operation,
        solver: &mut DataFlowSolver,
        _analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        // Mark the entry block arguments as having reached their pessimistic fixpoints.
        for region in top.regions() {
            if region.is_empty() {
                continue;
            }

            for argument in region.entry().arguments() {
                let argument = argument.borrow().as_value_ref();
                let mut lattice = solver.get_or_create_mut::<_, _>(argument);
                <A as SparseForwardDataFlowAnalysis>::set_to_entry_state(
                    &self.analysis,
                    &mut lattice,
                );
            }
        }

        forward::initialize_recursively(&self.analysis, top, solver)
    }

    /// Visit a program point.
    ///
    /// If this is at beginning of block and all control-flow predecessors or callsites are known,
    /// then the arguments lattices are propagated from them. If this is after call operation or an
    /// operation with region control-flow, then its result lattices are set accordingly. Otherwise,
    /// the operation transfer function is invoked.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report> {
        if !point.is_at_block_start() {
            return forward::visit_operation(
                &self.analysis,
                &point.prev_operation().unwrap().borrow(),
                solver,
            );
        }

        forward::visit_block(&self.analysis, &point.block().unwrap().borrow(), solver);

        Ok(())
    }
}

impl<A: SparseBackwardDataFlowAnalysis> DataFlowAnalysis for SparseDataFlowAnalysis<A, Backward> {
    fn analysis_id(&self) -> core::any::TypeId {
        core::any::TypeId::of::<Self>()
    }

    /// Initialize the analysis by visiting the operation and everything nested under it.
    fn initialize(
        &self,
        top: &Operation,
        solver: &mut DataFlowSolver,
        _analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        backward::initialize_recursively(&self.analysis, top, solver)
    }

    /// Visit a program point.
    ///
    /// If it is after call operation or an operation with block or region control-flow, then
    /// operand lattices are set accordingly. Otherwise, invokes the operation transfer function.
    fn visit(&self, point: &ProgramPoint, solver: &mut DataFlowSolver) -> Result<(), Report> {
        // For backward dataflow, we don't have to do any work for the blocks themselves. CFG edges
        // between blocks are processed by the BranchOp logic in `visit_operation`, and entry blocks
        // for functions are tied to the CallOp arguments by `visit_operation`.
        if point.is_at_block_start() {
            Ok(())
        } else {
            backward::visit_operation(
                &self.analysis,
                &point.prev_operation().unwrap().borrow(),
                solver,
            )
        }
    }
}
