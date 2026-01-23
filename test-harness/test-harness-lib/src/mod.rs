#![no_std]

extern crate alloc;

#[cfg(not(target_family = "wasm"))]
pub mod libtest;

// External dependencies
// RE-EXPORTS
// ================================================================================================
pub use cfg_if::cfg_if;
pub use miden_test_harness_macros::{miden_test, miden_test_suite};

#[cfg(not(target_family = "wasm"))]
pub mod reexports {
    pub use miden_objects::utils::Deserializable;
    pub use miden_testing;

    pub use crate::libtest::*;
}
