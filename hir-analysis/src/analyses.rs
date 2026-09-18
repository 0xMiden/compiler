pub mod callable_uses;
pub mod constant_propagation;
pub mod dce;
pub mod liveness;
mod loops;
pub mod spills;

pub use self::{
    callable_uses::{CallableUseAnalysis, CallableUseInfo},
    constant_propagation::SparseConstantPropagation,
    dce::DeadCodeAnalysis,
    liveness::LivenessAnalysis,
    loops::{LoopAction, LoopState},
    spills::SpillAnalysis,
};
