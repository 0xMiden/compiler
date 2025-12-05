//! Deserialization from felt representation for on-chain execution.

#![no_std]
#![deny(warnings)]

use miden_stdlib_sys::Felt;

/// Trait for deserialization from felt memory representation.
pub trait FromFeltRepr: Sized {
    /// Deserializes from a slice of `Felt` elements.
    fn from_felt_repr(felts: &[Felt]) -> Self;
}
