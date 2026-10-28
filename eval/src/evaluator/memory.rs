use alloc::{collections::BTreeMap, vec};

use midenc_hir::{AddressSpace, Felt, Immediate, SmallVec, SourceSpan, Type};
use midenc_session::diagnostics::{Diagnostic, miette};

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
    #[error("byte access requires a u32 memory element, got {0}")]
    #[diagnostic()]
    InvalidElement(u64),
    #[error("field element access requires an element-aligned pointer")]
    #[diagnostic()]
    UnalignedElement,
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
    #[error("a partial snapshot element must be a u32, got {0}")]
    #[diagnostic()]
    InvalidElement(u64),
    #[error("field element access requires an element-aligned pointer")]
    #[diagnostic()]
    UnalignedElement,
    #[error("element snapshot does not match the requested byte length")]
    #[diagnostic()]
    InvalidSnapshot,
}

/// A VM cell and byte offset, retaining the address units used by the originating pointer.
///
/// A cell occupies four bytes of IR address space but stores a complete field element.
#[derive(Debug, Copy, Clone)]
pub struct MemoryAddress {
    pub(super) element: u32,
    pub(super) offset: u8,
    pub(super) space: AddressSpace,
}

impl MemoryAddress {
    /// Interpret a raw pointer in byte or native element units.
    pub fn new(address: u32, space: AddressSpace) -> Self {
        let (element, offset) = match space {
            AddressSpace::Byte => (address / 4, (address % 4) as u8),
            AddressSpace::Element => (address, 0),
        };
        Self {
            element,
            offset,
            space,
        }
    }

    pub(crate) fn from_pointer(address: u32, ty: &Type) -> Self {
        let Type::Ptr(ptr) = ty else {
            panic!("expected verified pointer type")
        };
        Self::new(address, ptr.addrspace())
    }

    pub(crate) fn position(self) -> u64 {
        u64::from(self.element) * 4 + u64::from(self.offset)
    }

    pub(crate) fn is_element_aligned(self) -> bool {
        self.offset == 0
    }

    pub(super) fn raw(self) -> u32 {
        match self.space {
            AddressSpace::Byte => self.position() as u32,
            AddressSpace::Element => self.element,
        }
    }

    /// Offset a normalized address without changing the units of its originating pointer.
    pub(crate) fn checked_add(self, bytes: u64) -> Option<Self> {
        let position = self.position().checked_add(bytes)?;
        let element = u32::try_from(position / 4).ok()?;
        if self.space == AddressSpace::Byte && position > u64::from(u32::MAX) {
            return None;
        }
        Some(Self {
            element,
            offset: (position % 4) as u8,
            space: self.space,
        })
    }
}

/// Bare addresses in the evaluator's convenience API remain byte addresses.
impl From<u32> for MemoryAddress {
    fn from(address: u32) -> Self {
        Self::new(address, AddressSpace::Byte)
    }
}

/// Cell storage shared by sparse context memory and compact procedure-local buffers.
pub(super) trait Cells {
    fn get(&self, address: u32) -> Felt;
    fn set(&mut self, address: u32, value: Felt);
}

impl Cells for BTreeMap<u32, Felt> {
    fn get(&self, address: u32) -> Felt {
        BTreeMap::get(self, &address).copied().unwrap_or_default()
    }

    fn set(&mut self, address: u32, value: Felt) {
        if value == Felt::ZERO {
            self.remove(&address);
        } else {
            self.insert(address, value);
        }
    }
}

impl<const N: usize> Cells for SmallVec<[Felt; N]> {
    fn get(&self, address: u32) -> Felt {
        self.as_slice().get(address as usize).copied().unwrap_or_default()
    }

    fn set(&mut self, address: u32, value: Felt) {
        self[address as usize] = value;
    }
}

fn read_u32(memory: &impl Cells, element: u32) -> Result<u32, ReadFailed> {
    let value = memory.get(element).as_canonical_u64();
    u32::try_from(value).map_err(|_| ReadFailed::InvalidElement(value))
}

pub(super) fn read_byte(addr: MemoryAddress, memory: &impl Cells) -> Result<u8, ReadFailed> {
    Ok((read_u32(memory, addr.element)? >> (u32::from(addr.offset) * 8)) as u8)
}

fn read_bytes<const N: usize>(
    addr: MemoryAddress,
    memory: &impl Cells,
) -> Result<[u8; N], ReadFailed> {
    let mut bytes = [0; N];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        *byte = read_byte(addr.checked_add(offset as u64).expect("range was checked"), memory)?;
    }
    Ok(bytes)
}

/// Decode integers in little-endian order; a Felt occupies one whole cell, not eight bytes.
pub(super) fn read_value(
    addr: MemoryAddress,
    ty: &Type,
    memory: &impl Cells,
) -> Result<Value, ReadFailed> {
    let imm = match ty {
        Type::I1 => Immediate::I1(read_byte(addr, memory)? & 1 != 0),
        Type::I8 => Immediate::I8(read_byte(addr, memory)? as i8),
        Type::U8 => Immediate::U8(read_byte(addr, memory)?),
        Type::I16 => Immediate::I16(i16::from_le_bytes(read_bytes(addr, memory)?)),
        Type::U16 => Immediate::U16(u16::from_le_bytes(read_bytes(addr, memory)?)),
        Type::I32 => Immediate::I32(i32::from_le_bytes(read_bytes(addr, memory)?)),
        Type::U32 | Type::Ptr(_) => Immediate::U32(u32::from_le_bytes(read_bytes(addr, memory)?)),
        Type::I64 => Immediate::I64(i64::from_le_bytes(read_bytes(addr, memory)?)),
        Type::U64 => Immediate::U64(u64::from_le_bytes(read_bytes(addr, memory)?)),
        Type::I128 => Immediate::I128(i128::from_le_bytes(read_bytes(addr, memory)?)),
        Type::U128 => Immediate::U128(u128::from_le_bytes(read_bytes(addr, memory)?)),
        Type::F64 => Immediate::F64(f64::from_le_bytes(read_bytes(addr, memory)?)),
        Type::Felt if addr.offset == 0 => Immediate::Felt(memory.get(addr.element)),
        Type::Felt => return Err(ReadFailed::UnalignedElement),
        _ => return Err(ReadFailed::UnsupportedType),
    };
    Ok(Value::Immediate(imm))
}

/// The 32-bit view of a cell for a write that keeps some of its bytes.
///
/// The cell may still hold a felt at or above 2^32 from an earlier store (a reused stack slot);
/// like the MASM backend, a store keeps its low 32 bits instead of failing. Reads keep requiring a
/// u32 cell (see [read_u32]).
fn stored_u32(memory: &impl Cells, element: u32) -> u32 {
    memory.get(element).as_canonical_u64() as u32
}

/// Write a byte window, preserving the bytes outside it in every partially covered cell.
///
/// A fully covered cell is replaced without inspecting its previous content; a partially covered
/// cell keeps the low 32 bits of it (see [stored_u32]). The caller has checked the address range.
pub(super) fn write_bytes(addr: MemoryAddress, bytes: &[u8], memory: &mut impl Cells) {
    if bytes.is_empty() {
        return;
    }
    let first = addr.position();
    let end = first + bytes.len() as u64;
    let last = (end - 1) / 4;
    for element in u64::from(addr.element)..=last {
        let start = first.max(element * 4);
        let stop = end.min(element * 4 + 4);
        let mut value = if stop - start == 4 {
            0
        } else {
            stored_u32(memory, element as u32)
        };
        for position in start..stop {
            let shift = (position % 4) as u32 * 8;
            value = (value & !(0xff << shift))
                | (u32::from(bytes[(position - first) as usize]) << shift);
        }
        memory.set(element as u32, Felt::from(value));
    }
}

/// Write an immediate at `addr` in its memory representation: integers and `f64` as little-endian
/// bytes through [write_bytes], a boolean as one bit of its cell, a felt as a whole cell.
///
/// Only an unaligned felt fails; the caller has checked the address range.
pub(super) fn write_value(
    addr: MemoryAddress,
    value: Value,
    memory: &mut impl Cells,
) -> Result<(), WriteFailed> {
    let imm = match value {
        Value::Poison { value, .. } | Value::Immediate(value) => value,
    };
    match imm {
        Immediate::Felt(_) if addr.offset != 0 => return Err(WriteFailed::UnalignedElement),
        Immediate::Felt(value) => memory.set(addr.element, value),
        Immediate::I1(value) => {
            let previous = stored_u32(memory, addr.element);
            let shift = u32::from(addr.offset) * 8;
            memory.set(
                addr.element,
                Felt::from((previous & !(1 << shift)) | ((value as u32) << shift)),
            );
        }
        Immediate::I8(value) => write_bytes(addr, &[value as u8], memory),
        Immediate::U8(value) => write_bytes(addr, &[value], memory),
        Immediate::I16(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::U16(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::I32(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::U32(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::I64(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::U64(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::I128(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::U128(value) => write_bytes(addr, &value.to_le_bytes(), memory),
        Immediate::F64(value) => write_bytes(addr, &value.to_le_bytes(), memory),
    }
    Ok(())
}
