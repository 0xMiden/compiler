//! Bindings for Miden protocol
//!
//! # Word Field Ordering
//!
//! The Miden protocol MASM procedures expect and/or return Word on the stack with the least
//! significant felt on top of the stack.
//!
//! - In Rust: Word fields are stored as [e0, e1, e2, e3]
//! - In MASM procedures: These are pushed/popped from the stack in reverse order [e3, e2, e1, e0]

pub mod active_account;
pub mod active_note;
pub mod asset;
pub mod faucet;
pub mod input_note;
pub mod native_account;
pub mod output_note;
pub mod storage;
pub mod tx;
mod types;

pub use miden_felt_repr_onchain::{FromFeltRepr, ToFeltRepr};
pub use types::*;
