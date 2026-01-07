//! Serialization/deserialization for felt representation.
//!
//! This crate provides traits and utilities for converting Rust types to and from
//! a sequence of [`Felt`] elements.

#![no_std]
#![deny(warnings)]

extern crate alloc;

use alloc::vec::Vec;

pub use miden_felt::Felt;
/// Re-export `DeriveFromFeltRepr` as `FromFeltRepr` for `#[derive(FromFeltRepr)]` ergonomics.
pub use miden_felt_repr_derive::DeriveFromFeltRepr as FromFeltRepr;
/// Re-export `DeriveToFeltRepr` as `ToFeltRepr` for `#[derive(ToFeltRepr)]` ergonomics.
pub use miden_felt_repr_derive::DeriveToFeltRepr as ToFeltRepr;

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
    ///
    /// # Panics
    ///
    /// Panics if there are no more elements to read.
    #[inline(always)]
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
    #[inline(always)]
    pub fn new(data: &'a mut Vec<Felt>) -> Self {
        Self { data }
    }

    /// Writes a `Felt` element to the output.
    #[inline(always)]
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
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FromFeltRepr for miden_core::Felt {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        Self::from(reader.read())
    }
}

impl FromFeltRepr for u64 {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_u64()
    }
}

impl FromFeltRepr for u32 {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_u64() as u32
    }
}

impl FromFeltRepr for u8 {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_u64() as u8
    }
}

impl FromFeltRepr for bool {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        reader.read().as_u64() != 0
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
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        match reader.read().as_u64() {
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
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> Self {
        let len = reader.read().as_u64();
        assert!(len <= u32::MAX as u64, "Vec: length out of range");
        let len = len as usize;

        let mut result = Vec::with_capacity(len);

        let mut i = 0usize;
        while i < len {
            result.push(T::from_felt_repr(reader));
            i += 1;
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
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(*self);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ToFeltRepr for miden_core::Felt {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write((*self).into());
    }
}

impl ToFeltRepr for u64 {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::from_u64_unchecked(*self));
    }
}

impl ToFeltRepr for u32 {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::from_u64_unchecked(*self as u64));
    }
}

impl ToFeltRepr for u8 {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::from_u64_unchecked(*self as u64));
    }
}

impl ToFeltRepr for bool {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::from_u64_unchecked(*self as u64));
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ToFeltRepr for miden_objects::account::AccountId {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(self.prefix().as_felt().into());
        writer.write(self.suffix().into());
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
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        match self {
            None => writer.write(Felt::from_u64_unchecked(0)),
            Some(value) => {
                writer.write(Felt::from_u64_unchecked(1));
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
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        let len = self.len();
        assert!(len <= u32::MAX as usize, "Vec: length out of range");
        writer.write(Felt::from_u64_unchecked(len as u64));

        let mut i = 0usize;
        while i < len {
            self[i].write_felt_repr(writer);
            i += 1;
        }
    }
}
