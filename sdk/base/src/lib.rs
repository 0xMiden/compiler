#![no_std]

#[cfg(feature = "wit")]
pub mod base_wit;
mod types;

pub use miden_base_macros::{component, miden_generate};
pub use types::*;
