pub mod constant_propagation;
pub mod dce;
pub mod liveness;

pub use self::{constant_propagation::SparseConstantPropagation, dce::DeadCodeAnalysis};
