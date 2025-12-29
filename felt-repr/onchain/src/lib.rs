//! Deserialization from felt representation for on-chain execution.

#![no_std]
#![deny(warnings)]

/// Re-export the derive macro with the same name as the trait.
pub use miden_felt_repr_derive::DeriveFromFeltRepr as FromFeltRepr;
use miden_stdlib_sys::Felt;

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
