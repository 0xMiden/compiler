use crate::{AnalysisState, ChangeResult};

/// A [SparseLattice] represents some analysis state attached to a specific value.
///
/// It is propagated through the IR by sparse data-flow analysis.
#[allow(unused_variables)]
pub trait SparseLattice: AnalysisState + core::fmt::Debug {
    type Lattice: Clone;

    /// Get the underlying lattice value
    fn lattice(&self) -> &Self::Lattice;

    /// Join `rhs` with `self`, returning whether or not a change was made
    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }

    /// Meet `rhs` with `self`, returning whether or not a change was made
    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }
}
