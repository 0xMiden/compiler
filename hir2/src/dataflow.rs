pub mod analyses;
mod analysis;
mod anchor;
mod change_result;
mod config;
pub mod dense;
mod lattice;
mod program_point;
mod solver;
pub mod sparse;

use self::anchor::LatticeAnchorExt;
pub use self::{
    analysis::{
        AnalysisDirection, AnalysisKind, AnalysisState, AnalysisStateGuard, AnalysisStateInfo,
        AnalysisStateSubscription, AnalysisStateSubscriptionBehavior, AnalysisStrategy, Backward,
        BuildableAnalysisState, BuildableDataFlowAnalysis, CallControlFlowAction, DataFlowAnalysis,
        Dense, Forward, Revision, Sparse,
    },
    anchor::{LatticeAnchor, LatticeAnchorRef},
    change_result::ChangeResult,
    config::DataFlowConfig,
    dense::{DenseBackwardDataFlowAnalysis, DenseForwardDataFlowAnalysis, DenseLattice},
    lattice::{Lattice, LatticeLike},
    program_point::{Position, ProgramPoint},
    solver::{AnalysisQueue, DataFlowSolver},
    sparse::{SparseBackwardDataFlowAnalysis, SparseForwardDataFlowAnalysis, SparseLattice},
};
