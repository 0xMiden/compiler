#![no_std]
#![feature(allocator_api)]
#![feature(coerce_unsized)]
#![feature(debug_closure_helpers)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![feature(unsize)]
// Specialization
#![allow(incomplete_features)]
#![deny(warnings)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod analyses;
mod analysis;
mod anchor;
mod change_result;
mod config;
pub mod dense;
mod lattice;
mod solver;
pub mod sparse;

use self::anchor::LatticeAnchorExt;
pub use self::{
    analysis::{
        AnalysisKind, AnalysisState, AnalysisStateGuard, AnalysisStateGuardMut, AnalysisStateInfo,
        AnalysisStateSubscription, AnalysisStateSubscriptionBehavior, AnalysisStrategy,
        BuildableAnalysisState, BuildableDataFlowAnalysis, CallControlFlowAction, DataFlowAnalysis,
        Dense, Revision, Sparse,
    },
    anchor::{LatticeAnchor, LatticeAnchorRef},
    change_result::ChangeResult,
    config::DataFlowConfig,
    dense::{DenseBackwardDataFlowAnalysis, DenseForwardDataFlowAnalysis, DenseLattice},
    lattice::{Lattice, LatticeLike},
    solver::{AnalysisQueue, DataFlowSolver},
    sparse::{SparseBackwardDataFlowAnalysis, SparseForwardDataFlowAnalysis, SparseLattice},
};
