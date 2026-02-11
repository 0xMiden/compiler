//! A unified `Felt` for on-chain and off-chain Miden Rust code.
//!
//! This crate provides a single `Felt` type that can be used in both on-chain (Wasm) and off-chain
//! (native) Rust code:
//! - When targeting the Miden VM via Wasm, `Felt` is backed by an on-chain felt.
//! - Otherwise, `Felt` is backed by a felt (`miden-core`'s field element).

#![no_std]
#![deny(warnings)]

use core::{fmt, hash::Hash};

/// The field modulus, `2^64 - 2^32 + 1`.
pub const MODULUS: u64 = 0xffff_ffff_0000_0001;

/// Errors returned by [`Felt::new`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FeltError {
    /// The provided value was not a valid canonical felt.
    InvalidValue,
}

/// A crate-local trait capturing the API surface shared by all `Felt` representations.
///
/// This is used to ensure the on-chain and off-chain implementations don't drift in the common
/// "core" operations, and that required operator traits are implemented consistently.
pub(crate) trait FeltImpl:
    Copy
    + Clone
    + fmt::Debug
    + fmt::Display
    + Eq
    + Ord
    + Hash
    + core::ops::Add<Output = Self>
    + core::ops::Sub<Output = Self>
    + core::ops::Mul<Output = Self>
    + core::ops::Div<Output = Self>
    + core::ops::Neg<Output = Self>
    + core::ops::AddAssign
    + core::ops::SubAssign
    + core::ops::MulAssign
    + core::ops::DivAssign
{
    /// Creates a `Felt` from `value`.
    ///
    /// # Panics
    ///
    /// Panics if `value > Felt::M`.
    fn from_u64_unchecked(value: u64) -> Self;

    /// Creates a `Felt` from a `u32` value.
    fn from_u32(value: u32) -> Self;

    /// Returns the canonical `u64` value of this felt.
    fn as_u64(self) -> u64;

    /// Returns true if this felt is odd.
    fn is_odd(self) -> bool;

    /// Returns `self^-1`. Fails if `self = 0`.
    fn inv(self) -> Self;

    /// Returns `2^self`. Fails if `self > 63`.
    fn pow2(self) -> Self;

    /// Returns `self^other`.
    fn exp(self, other: Self) -> Self;
}

#[cfg(all(target_family = "wasm", miden))]
mod wasm32;
#[cfg(all(target_family = "wasm", miden))]
pub use wasm32::Felt;

#[cfg(not(all(target_family = "wasm", miden)))]
mod native;
#[cfg(not(all(target_family = "wasm", miden)))]
pub use native::Felt;

impl Felt {
    /// Field modulus = 2^64 - 2^32 + 1.
    pub const M: u64 = MODULUS;

    /// Creates a `Felt` from `value` without range checks.
    #[inline(always)]
    pub fn from_u64_unchecked(value: u64) -> Self {
        <Self as FeltImpl>::from_u64_unchecked(value)
    }

    /// Creates a `Felt` from a `u32` value.
    #[inline(always)]
    pub fn from_u32(value: u32) -> Self {
        <Self as FeltImpl>::from_u32(value)
    }

    /// Creates a `Felt` from `value`, returning an error if it is out of range.
    #[inline(always)]
    pub fn new(value: u64) -> Result<Self, FeltError> {
        if value >= Self::M {
            Err(FeltError::InvalidValue)
        } else {
            Ok(Self::from_u64_unchecked(value))
        }
    }

    /// Returns the canonical `u64` value of this felt.
    #[inline(always)]
    pub fn as_u64(self) -> u64 {
        <Self as FeltImpl>::as_u64(self)
    }

    /// Returns true if this felt is odd.
    #[inline(always)]
    pub fn is_odd(self) -> bool {
        <Self as FeltImpl>::is_odd(self)
    }

    /// Returns `self^-1`. Fails if `self = 0`.
    #[inline(always)]
    pub fn inv(self) -> Self {
        <Self as FeltImpl>::inv(self)
    }

    /// Returns `2^self`. Fails if `self > 63`.
    #[inline(always)]
    pub fn pow2(self) -> Self {
        <Self as FeltImpl>::pow2(self)
    }

    /// Returns `self^other`.
    #[inline(always)]
    pub fn exp(self, other: Self) -> Self {
        <Self as FeltImpl>::exp(self, other)
    }
}
