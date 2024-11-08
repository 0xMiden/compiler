pub mod constant_propagation;
pub mod dce;
pub mod liveness;
mod loops;

pub use self::{
    constant_propagation::SparseConstantPropagation,
    dce::DeadCodeAnalysis,
    loops::{LoopAction, LoopState},
};
