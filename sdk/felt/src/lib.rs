//! A unified `Felt` for on-chain and off-chain Miden Rust code.
//!
//! This crate provides a single `Felt` type that can be used in both on-chain (Wasm) and off-chain
//! (native) Rust code:
//! - On `wasm32` targets, `Felt` is backed by a VM intrinsic-backed `f32` representation.
//! - On non-`wasm32` targets, `Felt` is backed by `miden-core`'s field element type.
//!
//! The `true-felt` feature is reserved for future work; it is not supported on `wasm32` targets in
//! this PoC.

#![no_std]
#![deny(warnings)]

/// The field modulus, `2^64 - 2^32 + 1`.
pub const MODULUS: u64 = 0xffff_ffff_0000_0001;

/// Errors returned by [`Felt::new`].
#[derive(Debug)]
pub enum FeltError {
    /// The provided value was not a valid canonical field element.
    InvalidValue,
}

#[cfg(all(target_arch = "wasm32", feature = "true-felt"))]
compile_error!("The `true-felt` feature is not supported on `wasm32` targets in this PoC");

#[cfg(target_arch = "wasm32")]
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A field element represented as an intrinsic-backed `f32` on `wasm32` targets.
pub struct Felt {
    /// The underlying representation.
    pub inner: f32,
}

#[cfg(target_arch = "wasm32")]
mod wasm32;
#[cfg(not(target_arch = "wasm32"))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A field element represented as `miden_core::Felt` on non-`wasm32` targets.
pub struct Felt(pub miden_core::Felt);

#[cfg(not(target_arch = "wasm32"))]
mod native;

// Note: Felt assertion intrinsics live in `miden-stdlib-sys` (`sdk/stdlib-sys/src/intrinsics/felt.rs`).
