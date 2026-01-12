//! A unified `Felt` for on-chain and off-chain Miden Rust code.
//!
//! This crate provides a single `Felt` type that can be used in both on-chain (Wasm) and off-chain
//! (native) Rust code:
//! - When targeting the Miden VM via Wasm, `Felt` is backed by an on-chain felt.
//! - Otherwise, `Felt` is backed by a felt (`miden-core`'s field element).

#![no_std]
#![deny(warnings)]

/// The field modulus, `2^64 - 2^32 + 1`.
pub const MODULUS: u64 = 0xffff_ffff_0000_0001;

/// Errors returned by [`Felt::new`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FeltError {
    /// The provided value was not a valid canonical felt.
    InvalidValue,
}

#[cfg(all(target_family = "wasm", miden))]
mod wasm32;
#[cfg(all(target_family = "wasm", miden))]
pub use wasm32::Felt;

#[cfg(not(all(target_family = "wasm", miden)))]
mod native;
#[cfg(not(all(target_family = "wasm", miden)))]
pub use native::Felt;
