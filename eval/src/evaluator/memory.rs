use alloc::{format, string::String, vec, vec::Vec};
use core::ops::{Index, IndexMut, Range};

use midenc_hir2::{Felt, FieldElement, Immediate, SmallVec, SourceSpan, Type};
use midenc_session::{
    diagnostics::{miette, Diagnostic},
    miden_assembly::utils::Deserializable,
};

use crate::Value;

/// An error occurred while reading a value from memory
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ReadFailed {
    #[error("attempted to read memory beyond addressable heap at {addr}")]
    #[diagnostic()]
    AddressOutOfBounds {
        addr: u32,
        #[label]
        at: SourceSpan,
    },
    #[error("attempted to read value of size {size} beyond addressable heap at {addr}")]
    #[diagnostic()]
    SizeOutOfBounds {
        addr: u32,
        size: u32,
        #[label]
        at: SourceSpan,
    },
    #[error("unsupported type")]
    #[diagnostic()]
    UnsupportedType,
    #[error("invalid field element: {0}")]
    #[diagnostic()]
    InvalidFelt(String),
}

/// An error occurred while writing a value to memory
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum WriteFailed {
    #[error("attempted to write memory beyond addressable heap at {addr}")]
    #[diagnostic()]
    AddressOutOfBounds {
        addr: u32,
        #[label]
        at: SourceSpan,
    },
    #[error("attempted to write value of size {size} beyond addressable heap at {addr}")]
    #[diagnostic()]
    SizeOutOfBounds {
        addr: u32,
        size: u32,
        #[label]
        at: SourceSpan,
    },
}

/// Read a value of type `ty`, starting from offset `addr` in `memory`
///
/// This operation can fail if `ty` is not a supported immediate type, or if the bytes in memory
/// are not valid for that type.
///
/// NOTE: If `memory` is smaller than implied by `addr`, it is presumed to be zeroed.
pub fn read_value(addr: usize, ty: &Type, memory: &[u8]) -> Result<Value, ReadFailed> {
    let imm = match ty {
        Type::I1 => {
            let byte = read_byte(addr, memory);
            Immediate::I1((byte & 0x1) == 1)
        }
        Type::I8 => {
            let value = read_byte(addr, memory) as i8;
            Immediate::I8(value)
        }
        Type::U8 => {
            let value = read_byte(addr, memory);
            Immediate::U8(value)
        }
        Type::I16 => {
            let value = i16::from_be_bytes(read_bytes(addr, memory));
            Immediate::I16(value)
        }
        Type::U16 => {
            let value = u16::from_be_bytes(read_bytes(addr, memory));
            Immediate::U16(value)
        }
        Type::I32 => {
            let value = i32::from_be_bytes(read_bytes(addr, memory));
            Immediate::I32(value)
        }
        Type::U32 => {
            let value = u32::from_be_bytes(read_bytes(addr, memory));
            Immediate::U32(value)
        }
        Type::I64 => {
            let value = i64::from_be_bytes(read_bytes(addr, memory));
            Immediate::I64(value)
        }
        Type::U64 => {
            let value = u64::from_be_bytes(read_bytes(addr, memory));
            Immediate::U64(value)
        }
        Type::I128 => {
            let value = i128::from_be_bytes(read_bytes(addr, memory));
            Immediate::I128(value)
        }
        Type::U128 => {
            let value = u128::from_be_bytes(read_bytes(addr, memory));
            Immediate::U128(value)
        }
        Type::F64 => {
            let value = f64::from_be_bytes(read_bytes(addr, memory));
            Immediate::F64(value)
        }
        Type::Felt => {
            const FELT_SIZE: usize = Felt::ELEMENT_BYTES;
            let bytes = read_bytes::<FELT_SIZE>(addr, memory);
            Felt::read_from_bytes(&bytes).map(Immediate::Felt).map_err(|err| {
                ReadFailed::InvalidFelt(format!("failed to decode felt at {addr}: {err}"))
            })?
        }
        Type::Ptr(_) => {
            let value = u32::from_be_bytes(read_bytes(addr, memory));
            Immediate::U32(value)
        }
        _ => {
            return Err(ReadFailed::UnsupportedType);
        }
    };

    Ok(Value::Immediate(imm))
}

/// Read a single byte from `addr` in `memory`.
///
/// Returns a zero byte if `addr` is not in bounds of `memory`.
#[inline]
pub fn read_byte(addr: usize, memory: &[u8]) -> u8 {
    memory.get(addr).copied().unwrap_or_default()
}

/// Read `N` bytes starting from `addr` in `memory`.
///
/// Any bytes that are out of bounds of `memory` are presumed to be zeroed.
pub fn read_bytes<const N: usize>(addr: usize, memory: &[u8]) -> [u8; N] {
    match memory.get(addr..(addr + N)) {
        Some(bytes) => <[u8; N]>::try_from(bytes).unwrap(),
        None if memory.len() <= addr => {
            // No memory at `addr` has been written yet, return all zeros
            [0; N]
        }
        None => {
            // Some bytes are available, but not all, read them individually
            let mut buf = [0; N];
            for (byte, addr) in (addr..(addr + N)).enumerate() {
                buf[byte] = read_byte(addr, memory);
            }
            buf
        }
    }
}

/// This trait exists so as to abstract over the buffer type being used to represent memory.
///
/// For now, it is implemented for `Vec` and `SmallVec`.
pub trait Buffer:
    Index<usize, Output = u8>
    + Index<Range<usize>, Output = [u8]>
    + IndexMut<usize, Output = u8>
    + IndexMut<Range<usize>, Output = [u8]>
{
    fn get_mut(&mut self, index: usize) -> Option<&mut u8>;
    fn get_slice_mut(&mut self, index: Range<usize>) -> Option<&mut [u8]>;
    fn resize(&mut self, len: usize, value: u8);
}

impl Buffer for Vec<u8> {
    #[inline(always)]
    fn get_mut(&mut self, index: usize) -> Option<&mut u8> {
        self.as_mut_slice().get_mut(index)
    }

    #[inline(always)]
    fn get_slice_mut(&mut self, index: Range<usize>) -> Option<&mut [u8]> {
        self.as_mut_slice().get_mut(index)
    }

    #[inline(always)]
    fn resize(&mut self, len: usize, value: u8) {
        self.resize(len, value)
    }
}

impl<const N: usize> Buffer for SmallVec<[u8; N]> {
    #[inline(always)]
    fn get_mut(&mut self, index: usize) -> Option<&mut u8> {
        self.as_mut_slice().get_mut(index)
    }

    #[inline(always)]
    fn get_slice_mut(&mut self, index: Range<usize>) -> Option<&mut [u8]> {
        self.as_mut_slice().get_mut(index)
    }

    #[inline(always)]
    fn resize(&mut self, len: usize, value: u8) {
        self.resize(len, value)
    }
}

/// Write `value` to `memory` starting at offset `addr`.
///
/// If `addr`, or the resulting write, would go out of bounds of `memory`, it is resized such that
/// there is sufficient space for the write, i.e. a write never fails unless allocating the
/// underlying storage would fail.
pub fn write_value<B: Buffer>(addr: usize, value: Value, memory: &mut B) {
    let imm = match value {
        Value::Poison { value, .. } | Value::Immediate(value) => value,
    };

    match imm {
        Immediate::I1(value) => write_byte(addr, value as u8, memory),
        Immediate::I8(value) => write_byte(addr, value as u8, memory),
        Immediate::U8(value) => write_byte(addr, value, memory),
        Immediate::I16(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::U16(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::I32(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::U32(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::I64(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::U64(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::I128(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::U128(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::F64(value) => write_bytes(addr, &value.to_be_bytes(), memory),
        Immediate::Felt(value) => write_bytes(addr, Felt::elements_as_bytes(&[value]), memory),
    }
}

/// Write `byte` to `memory` at offset `addr`.
///
/// If `addr` is out of bounds of `memory`, it is resized such that there is sufficient space for
/// the write, i.e. a write never fails unless allocating the underlying storage would fail.
pub fn write_byte<B: Buffer>(addr: usize, byte: u8, memory: &mut B) {
    match memory.get_mut(addr) {
        Some(slot) => *slot = byte,
        None => {
            memory.resize(addr + 8, 0);
            memory[addr] = byte;
        }
    }
}

/// Write `bytes` to `memory` at offset `addr`.
///
/// If `addr`, or the resulting write, would go out of bounds of `memory`, it is resized such that
/// there is sufficient space for the write, i.e. a write never fails unless allocating the
/// underlying storage would fail.
pub fn write_bytes<B: Buffer>(addr: usize, bytes: &[u8], memory: &mut B) {
    match memory.get_slice_mut(addr..(addr + bytes.len())) {
        Some(target_bytes) => {
            target_bytes.copy_from_slice(bytes);
        }
        None => {
            memory.resize(addr + bytes.len() + 8, 0);
            memory[addr..(addr + bytes.len())].copy_from_slice(bytes);
        }
    }
}
