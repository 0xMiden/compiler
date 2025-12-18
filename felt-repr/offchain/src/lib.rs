//! Serialization/deserialization for felt representation in off-chain use.
//!
//! This crate provides traits and utilities for converting Rust types to and from
//! a sequence of `Felt` elements, suitable for preparing data to send to on-chain code.

#![no_std]
#![deny(warnings)]

extern crate alloc;

mod account_id;

use alloc::vec::Vec;

pub use account_id::AccountIdFeltRepr;
use miden_core::Felt;
/// Re-export the derive macros with the same name as the traits.
pub use miden_felt_repr_derive::DeriveFromFeltReprOffchain as FromFeltRepr;
pub use miden_felt_repr_derive::DeriveToFeltReprOffchain as ToFeltRepr;

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
    ///
    /// # Panics
    ///
    /// Panics if there are no more elements to read.
    pub fn read(&mut self) -> Felt {
        assert!(self.pos < self.data.len(), "FeltReader: no more elements to read");
        let felt = self.data[self.pos];
        self.pos += 1;
        felt
    }
}

/// A writer that wraps a `Vec<Felt>` and appends elements to it.
pub struct FeltWriter<'a> {
    data: &'a mut Vec<Felt>,
}

impl<'a> FeltWriter<'a> {
    /// Creates a new `FeltWriter` from a mutable reference to a `Vec<Felt>`.
    pub fn new(data: &'a mut Vec<Felt>) -> Self {
        Self { data }
    }

    /// Writes a `Felt` element to the output.
    pub fn write(&mut self, felt: Felt) {
        self.data.push(felt);
    }
}

/// Trait for deserialization from felt memory representation.
pub trait FromFeltRepr: Sized {
    /// Deserializes from a `FeltReader`, consuming the required elements.
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self;
}

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

impl FromFeltRepr for bool {
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_int() != 0
    }
}

/// Encodes an `Option<T>` as a 1-felt tag followed by the payload (if present).
///
/// Format:
/// - `None` => `[0]`
/// - `Some(x)` => `[1, x...]`
impl<T> FromFeltRepr for Option<T>
where
    T: FromFeltRepr,
{
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        match reader.read().as_int() {
            0 => None,
            1 => Some(T::from_felt_repr(reader)),
            _ => panic!("Option: invalid tag"),
        }
    }
}

/// Encodes a `Vec<T>` as a length prefix followed by elements.
///
/// Format: `[len, elem0..., elemN-1...]` where `len` is a `u32` encoded in a single `Felt`.
impl<T> FromFeltRepr for Vec<T>
where
    T: FromFeltRepr,
{
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        let len = reader.read().as_int();
        assert!(len <= u32::MAX as u64, "Vec: length out of range");
        let len = len as usize;

        let mut result = Vec::with_capacity(len);
        for _ in 0..len {
            result.push(T::from_felt_repr(reader));
        }
        result
    }
}

/// Trait for serializing a type into its felt memory representation.
pub trait ToFeltRepr {
    /// Writes this value's felt representation to the writer.
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>);

    /// Convenience method that allocates and returns a `Vec<Felt>`.
    fn to_felt_repr(&self) -> Vec<Felt> {
        // Allocate ahead to avoid reallocations
        let mut data = Vec::with_capacity(256);
        self.write_felt_repr(&mut FeltWriter::new(&mut data));
        data
    }
}

impl ToFeltRepr for Felt {
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(*self);
    }
}

impl ToFeltRepr for u64 {
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::new(*self));
    }
}

impl ToFeltRepr for u32 {
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::new(*self as u64));
    }
}

impl ToFeltRepr for u8 {
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::new(*self as u64));
    }
}

impl ToFeltRepr for bool {
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::new(*self as u64));
    }
}

/// Encodes an `Option<T>` as a 1-felt tag followed by the payload (if present).
///
/// Format:
/// - `None` => `[0]`
/// - `Some(x)` => `[1, x...]`
impl<T> ToFeltRepr for Option<T>
where
    T: ToFeltRepr,
{
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        match self {
            None => writer.write(Felt::new(0)),
            Some(value) => {
                writer.write(Felt::new(1));
                value.write_felt_repr(writer);
            }
        }
    }
}

/// Encodes a `Vec<T>` as a length prefix followed by elements.
///
/// Format: `[len, elem0..., elemN-1...]` where `len` is a `u32` encoded in a single `Felt`.
impl<T> ToFeltRepr for Vec<T>
where
    T: ToFeltRepr,
{
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        assert!(self.len() <= u32::MAX as usize, "Vec: length out of range");
        writer.write(Felt::new(self.len() as u64));
        for elem in self {
            elem.write_felt_repr(writer);
        }
    }
}
