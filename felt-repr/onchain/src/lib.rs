//! Serialization/deserialization for felt representation in on-chain execution.

#![no_std]
#![deny(warnings)]

extern crate alloc;

use alloc::{vec, vec::Vec};

/// Re-export the derive macros with the same name as the traits.
pub use miden_felt_repr_derive::DeriveFromFeltRepr as FromFeltRepr;
pub use miden_felt_repr_derive::DeriveToFeltReprOnchain as ToFeltRepr;
use miden_stdlib_sys::Felt;

/// A reader that wraps a slice of `Felt` elements and tracks the current position.
pub struct FeltReader<'a> {
    data: &'a [Felt],
    pos: usize,
}

impl<'a> FeltReader<'a> {
    /// Creates a new `FeltReader` from a slice of `Felt` elements.
    #[inline(always)]
    pub fn new(data: &'a [Felt]) -> Self {
        Self { data, pos: 0 }
    }

    /// Reads the next `Felt` element, advancing the position.
    #[inline(always)]
    pub fn read(&mut self) -> Felt {
        let felt = self.data[self.pos];
        self.pos += 1;
        felt
    }
}

/// Trait for deserialization from felt memory representation.
pub trait FromFeltRepr: Sized {
    /// Deserializes from a `FeltReader`, consuming the required elements.
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self;
}

/// Base implementation for `Felt` itself.
impl FromFeltRepr for Felt {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read()
    }
}

/// Trait for serializing a type into its felt memory representation.
pub trait ToFeltRepr {
    /// Serializes this value into a vector of `Felt` elements.
    fn to_felt_repr(&self) -> Vec<Felt>;
}

/// Base implementation for `Felt` itself.
impl ToFeltRepr for Felt {
    #[inline(always)]
    fn to_felt_repr(&self) -> Vec<Felt> {
        vec![*self]
    }
}
