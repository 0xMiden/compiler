//! A unified `Felt` for on-chain and off-chain Miden Rust code.
//!
//! This crate provides a single `Felt` type that can be used in both on-chain (Wasm) and off-chain
//! (native) Rust code:
//! - On `wasm32` targets, `Felt` is backed by an on-chain felt.
//! - On non-`wasm32` targets, `Felt` is backed by a felt (`miden-core`'s field element).
//!

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

#[cfg(target_arch = "wasm32")]
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A `Felt` represented as an on-chain felt.
pub struct Felt {
    /// The backing type is `f32` which will be treated as a felt by the compiler.
    /// We're basically hijacking the Wasm `f32` type and treat as felt.
    pub inner: f32,
}

#[cfg(target_arch = "wasm32")]
mod wasm32;
#[cfg(not(target_arch = "wasm32"))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A `Felt` represented as a felt (`miden_core::Felt`).
pub struct Felt(pub miden_core::Felt);

#[cfg(not(target_arch = "wasm32"))]
mod native;
