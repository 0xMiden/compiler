pub mod callable_uses;
pub mod constant_propagation;
pub mod dce;
pub mod integer_range;
pub mod liveness;
mod loops;

pub use self::{
    callable_uses::{CallableUseInfo, CallableUseSnapshot},
    constant_propagation::SparseConstantPropagation,
    dce::DeadCodeAnalysis,
    integer_range::{
        ConstantIntRange, IntegerRangeAnalysis, IntegerRangeConfidence, IntegerValueRange,
    },
    liveness::LivenessAnalysis,
    loops::{LoopAction, LoopState},
};

#[cfg(test)]
mod early_exit;
