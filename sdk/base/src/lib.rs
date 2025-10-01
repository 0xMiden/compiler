#![no_std]

#[cfg(feature = "wit")]
pub mod base_wit;
mod types;

pub use miden_base_macros::{component, generate, note_script, tx_script};
pub use types::*;
