pub mod constant_propagation;
pub mod dce;
pub mod liveness;
mod loops;
pub mod spills;

pub use self::{
    constant_propagation::SparseConstantPropagation,
    dce::DeadCodeAnalysis,
    liveness::LivenessAnalysis,
    loops::{LoopAction, LoopState},
    spills::SpillAnalysis,
};
