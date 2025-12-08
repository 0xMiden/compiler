//! Serialization/deserialization for felt representation in off-chain use.

#![no_std]
#![deny(warnings)]

extern crate alloc;

mod account_id;

use alloc::{vec, vec::Vec};

pub use account_id::AccountIdFeltRepr;
use miden_core::Felt;
/// Re-export the derive macros with the same name as the traits.
pub use miden_felt_repr_derive::DeriveFromFeltReprOffchain as FromFeltRepr;
pub use miden_felt_repr_derive::DeriveToFeltRepr as ToFeltRepr;

/// A reader that wraps a slice of `Felt` elements and tracks the current position.
pub struct FeltReader<'a> {
    data: &'a [Felt],
    pos: usize,
}

impl<'a> FeltReader<'a> {
    /// Creates a new `FeltReader` from a slice of `Felt` elements.
    pub fn new(data: &'a [Felt]) -> Self {
        Self { data, pos: 0 }
    }

    /// Reads the next `Felt` element, advancing the position.
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
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read()
    }
}

impl FromFeltRepr for u64 {
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_int()
    }
}

impl FromFeltRepr for u32 {
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_int() as u32
    }
}

impl FromFeltRepr for u8 {
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_int() as u8
    }
}

/// Trait for serializing a type into its felt memory representation.
pub trait ToFeltRepr {
    /// Serializes this value into a vector of `Felt` elements.
    fn to_felt_repr(&self) -> Vec<Felt>;
}

/// Base implementation for `Felt` itself.
impl ToFeltRepr for Felt {
    fn to_felt_repr(&self) -> Vec<Felt> {
        vec![*self]
    }
}

impl ToFeltRepr for u64 {
    fn to_felt_repr(&self) -> Vec<Felt> {
        vec![Felt::new(*self)]
    }
}

impl ToFeltRepr for u32 {
    fn to_felt_repr(&self) -> Vec<Felt> {
        vec![Felt::new(*self as u64)]
    }
}

impl ToFeltRepr for u8 {
    fn to_felt_repr(&self) -> Vec<Felt> {
        vec![Felt::new(*self as u64)]
    }
}
