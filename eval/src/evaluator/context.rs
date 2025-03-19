use alloc::vec::Vec;

use midenc_hir::{dialects::builtin::ComponentId, Report, SourceSpan, Type};
use midenc_session::diagnostics::WrapErr;

use super::memory::{self, ReadFailed, WriteFailed};
use crate::Value;

const PAGE_SIZE: usize = 64 * 1024;
const MAX_ADDRESSABLE_HEAP: usize = 2usize.pow(30) - 1;

/// The execution context associated with Miden context boundaries
pub struct ExecutionContext {
    /// The identifier for this context, if known
    ///
    /// The root context never has an identifier
    #[allow(unused)]
    id: Option<ComponentId>,
    /// Heap memory
    memory: Vec<u8>,
}

impl ExecutionContext {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id: Some(id),
            ..Default::default()
        }
    }

    /// Grow the heap of this context to be at least `n` pages
    pub fn memory_grow(&mut self, n: usize) {
        assert!(((n * PAGE_SIZE) as u32) < u32::MAX, "cannot grow heap larger than u32::MAX");
        self.memory.resize(n * PAGE_SIZE, 0);
    }

    /// Return the size of this context's heap in pages
    pub fn memory_size(&self) -> usize {
        self.memory.len() / PAGE_SIZE
    }

    /// Reset the memory of this context to its initial state
    pub fn reset(&mut self) {
        self.memory.truncate(0);
        self.memory.resize(4 * PAGE_SIZE, 0);
    }

    /// Read a value of type `ty` from `addr`
    ///
    /// Returns an error if `addr` is invalid, `ty` is not a valid immediate type, or the specified
    /// type could not be read from `addr` (either the encoding is invalid, or the read would be
    /// out of bounds).
    pub fn read_memory(&self, addr: u32, ty: &Type, at: SourceSpan) -> Result<Value, Report> {
        let addr = addr as usize;
        if addr > MAX_ADDRESSABLE_HEAP {
            return Err(ReadFailed::AddressOutOfBounds {
                addr: addr as u32,
                at,
            })
            .wrap_err("invalid memory read");
        }

        let size = ty.size_in_bytes();
        let end_addr = addr.checked_add(size);
        if end_addr.is_none_or(|addr| addr > MAX_ADDRESSABLE_HEAP) {
            return Err(ReadFailed::SizeOutOfBounds {
                addr: addr as u32,
                size: size as u32,
                at,
            })
            .wrap_err("invalid memory read");
        }

        memory::read_value(addr, ty, &self.memory).wrap_err("invalid memory read")
    }

    /// Write `value` to `addr` in heap memory.
    ///
    /// Returns an error if `addr` is invalid, or `value` could not be written to `addr` (either the
    /// value is poison, or the write would go out of bounds).
    pub fn write_memory(
        &mut self,
        addr: u32,
        value: impl Into<Value>,
        at: SourceSpan,
    ) -> Result<(), Report> {
        let addr = addr as usize;
        if addr > MAX_ADDRESSABLE_HEAP {
            return Err(WriteFailed::AddressOutOfBounds {
                addr: addr as u32,
                at,
            })
            .wrap_err("invalid memory write");
        }

        let value = value.into();
        let ty = value.ty();
        let size = ty.size_in_bytes();
        let end_addr = addr.checked_add(size);
        if end_addr.is_none_or(|addr| addr > MAX_ADDRESSABLE_HEAP) {
            return Err(WriteFailed::SizeOutOfBounds {
                addr: addr as u32,
                size: size as u32,
                at,
            })
            .wrap_err("invalid memory write");
        }

        memory::write_value(addr, value, &mut self.memory);

        Ok(())
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            id: None,
            memory: Vec::with_capacity(4 * PAGE_SIZE),
        }
    }
}
