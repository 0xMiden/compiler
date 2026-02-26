//! Serialization/deserialization for felt representation.
//!
//! This crate provides traits and utilities for converting Rust types to and from
//! a sequence of [`Felt`] elements.

#![no_std]
#![deny(warnings)]

extern crate alloc;

use alloc::vec::Vec;

pub use miden_field::Felt;
/// Re-export `DeriveFromFeltRepr` as `FromFeltRepr` for `#[derive(FromFeltRepr)]` ergonomics.
pub use miden_field_repr_derive::DeriveFromFeltRepr as FromFeltRepr;
/// Re-export `DeriveToFeltRepr` as `ToFeltRepr` for `#[derive(ToFeltRepr)]` ergonomics.
pub use miden_field_repr_derive::DeriveToFeltRepr as ToFeltRepr;

/// Error returned when decoding a type from its felt representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeltReprError {
    /// Attempted to read beyond the end of the felt slice.
    UnexpectedEof {
        /// Current read position.
        pos: usize,
        /// Total number of felts available.
        len: usize,
    },
    /// A decoded value did not fit into the target Rust type.
    ValueOutOfRange {
        /// Position of the decoded value.
        pos: usize,
        /// Total number of felts available.
        len: usize,
        /// Name of the target Rust type.
        ty: &'static str,
        /// The decoded value.
        value: u64,
        /// The maximum supported value for `ty`.
        max: u64,
    },
    /// An `Option<T>` tag was neither `0` nor `1`.
    InvalidOptionTag {
        /// Position of the decoded tag.
        pos: usize,
        /// Total number of felts available.
        len: usize,
        /// The decoded tag.
        tag: u64,
    },
    /// A boolean value was neither `0` nor `1`.
    InvalidBool {
        /// Position of the decoded value.
        pos: usize,
        /// Total number of felts available.
        len: usize,
        /// The decoded value.
        value: u64,
    },
    /// An enum tag was not a valid variant ordinal.
    UnknownEnumTag {
        /// Position of the decoded tag.
        pos: usize,
        /// Total number of felts available.
        len: usize,
        /// Name of the decoded enum type.
        ty: &'static str,
        /// The decoded tag.
        tag: u32,
    },
    /// Extra data remained after decoding a value.
    TrailingData {
        /// Current read position.
        pos: usize,
        /// Total number of felts available.
        len: usize,
    },
    /// A custom decoding error provided by a downstream implementation.
    Custom(&'static str),
}

impl core::fmt::Display for FeltReprError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof { pos, len } => {
                write!(f, "unexpected end of input at felt {pos} of {len}")
            }
            Self::ValueOutOfRange {
                pos,
                len,
                ty,
                value,
                max,
            } => {
                write!(f, "value {value} out of range for {ty} at felt {pos} of {len} (max {max})")
            }
            Self::InvalidOptionTag { pos, len, tag } => {
                write!(f, "invalid Option tag at felt {pos} of {len}: {tag}")
            }
            Self::InvalidBool { pos, len, value } => {
                write!(f, "invalid bool value at felt {pos} of {len}: {value}")
            }
            Self::UnknownEnumTag { pos, len, ty, tag } => {
                write!(f, "unknown enum tag for {ty} at felt {pos} of {len}: {tag}")
            }
            Self::TrailingData { pos, len } => {
                write!(f, "trailing data starting at felt {pos} of {len}")
            }
            Self::Custom(msg) => f.write_str(msg),
        }
    }
}

/// Convenience alias for results returned by felt-repr decoding APIs.
pub type FeltReprResult<T> = core::result::Result<T, FeltReprError>;

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

    /// Returns the current read position.
    #[inline(always)]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Returns the total number of felts in the underlying slice.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the underlying slice is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Returns the number of unread felts remaining.
    #[inline(always)]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Ensures there are no unread felts remaining.
    #[inline(always)]
    pub fn ensure_eof(&self) -> FeltReprResult<()> {
        if self.remaining() != 0 {
            return Err(FeltReprError::TrailingData {
                pos: self.pos,
                len: self.data.len(),
            });
        }
        Ok(())
    }

    /// Reads the next `Felt` element, advancing the position.
    #[inline(always)]
    pub fn read(&mut self) -> FeltReprResult<Felt> {
        if self.pos >= self.data.len() {
            return Err(FeltReprError::UnexpectedEof {
                pos: self.pos,
                len: self.data.len(),
            });
        }

        let felt = self.data[self.pos];
        self.pos += 1;
        Ok(felt)
    }

    /// Reads the next element and decodes it as a `u32`.
    #[inline(always)]
    pub fn read_u32(&mut self) -> FeltReprResult<u32> {
        let pos = self.pos;
        let len = self.data.len();
        let value = self.read()?.as_u64();
        if value > u32::MAX as u64 {
            return Err(FeltReprError::ValueOutOfRange {
                pos,
                len,
                ty: "u32",
                value,
                max: u32::MAX as u64,
            });
        }
        Ok(value as u32)
    }

    /// Reads the next element and decodes it as a `u8`.
    #[inline(always)]
    pub fn read_u8(&mut self) -> FeltReprResult<u8> {
        let pos = self.pos;
        let len = self.data.len();
        let value = self.read()?.as_u64();
        if value > u8::MAX as u64 {
            return Err(FeltReprError::ValueOutOfRange {
                pos,
                len,
                ty: "u8",
                value,
                max: u8::MAX as u64,
            });
        }
        Ok(value as u8)
    }

    /// Reads the next element and decodes it as a boolean.
    ///
    /// Only `0` and `1` are accepted.
    #[inline(always)]
    pub fn read_bool(&mut self) -> FeltReprResult<bool> {
        let pos = self.pos;
        let len = self.data.len();
        match self.read()?.as_u64() {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(FeltReprError::InvalidBool { pos, len, value }),
        }
    }

    /// Reads the next element and decodes it as a length prefix.
    ///
    /// The length is encoded as a `u32` in a single `Felt`.
    #[inline(always)]
    pub fn read_len_u32(&mut self) -> FeltReprResult<usize> {
        Ok(self.read_u32()? as usize)
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
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self>;
}

impl FromFeltRepr for Felt {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        reader.read()
    }
}

#[cfg(not(target_family = "wasm"))]
impl FromFeltRepr for miden_core::Felt {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        Ok(Self::from(reader.read()?))
    }
}

impl FromFeltRepr for u64 {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        // Encode u64 as 2 u32 limbs
        let lo = reader.read_u32()? as u64;
        let hi = reader.read_u32()? as u64;
        Ok((hi << 32) | lo)
    }
}

impl FromFeltRepr for u32 {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        reader.read_u32()
    }
}

impl FromFeltRepr for u8 {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        reader.read_u8()
    }
}

impl FromFeltRepr for bool {
    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        reader.read_bool()
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
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        let pos = reader.pos();
        let len = reader.len();
        match reader.read()?.as_u64() {
            0 => Ok(None),
            1 => Ok(Some(T::from_felt_repr(reader)?)),
            tag => Err(FeltReprError::InvalidOptionTag { pos, len, tag }),
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
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        let len = reader.read_len_u32()?;

        let mut result = Vec::with_capacity(len);

        let mut i = 0usize;
        while i < len {
            result.push(T::from_felt_repr(reader)?);
            i += 1;
        }
        Ok(result)
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

#[cfg(not(target_family = "wasm"))]
impl ToFeltRepr for miden_core::Felt {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write((*self).into());
    }
}

impl ToFeltRepr for u64 {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        let lo = (*self & 0xffff_ffff) as u32;
        let hi = (*self >> 32) as u32;
        writer.write(Felt::from_u32(lo));
        writer.write(Felt::from_u32(hi));
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
