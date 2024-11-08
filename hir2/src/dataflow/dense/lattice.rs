use crate::dataflow::{AnalysisState, ChangeResult};

/// A [DenseLattice] represents some program state at a specific program point.
///
/// It is propagated through the IR by dense data-flow analysis.
#[allow(unused_variables)]
pub trait DenseLattice: AnalysisState {
    type Lattice;

    fn lattice(&self) -> &Self::Lattice;

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }
    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }
}
