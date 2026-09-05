use alloc::vec::Vec;

use midenc_hir::{Report, SourceSpan, Type, dialects::builtin::ComponentId};
use midenc_session::diagnostics::WrapErr;

use super::memory::{self, ReadFailed, WriteFailed};
use crate::Value;

const PAGE_SIZE: usize = 64 * 1024;
// Match HEAP_END in codegen/masm/intrinsics/mem.masm: convert the last usable
// element address to the byte-addressed heap boundary used by the evaluator.
const MAX_ADDRESSABLE_HEAP: usize = (2usize.pow(30) - 1) * 4;

/// The execution context associated with Miden context boundaries
pub struct ExecutionContext {
    /// The identifier for this context, if known
    ///
    /// The root context never has an identifier
    #[allow(unused)]
    id: Option<ComponentId>,
    /// Heap memory
    memory: Vec<u8>,
    /// Pages requested through memory_grow; independent of materialized bytes.
    pages: usize,
}

impl ExecutionContext {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id: Some(id),
            ..Default::default()
        }
    }

    /// Grow the logical heap by `n` pages, preserving its contents on failure.
    ///
    /// Storage is materialized by writes. Growing does not allocate a host buffer for
    /// untouched zero-filled pages, just as ordinary reads do not materialize memory.
    pub fn memory_grow(&mut self, n: usize) -> bool {
        let Some(pages) = self.pages.checked_add(n) else {
            return false;
        };
        if pages > MAX_ADDRESSABLE_HEAP / PAGE_SIZE {
            return false;
        }
        self.pages = pages;
        true
    }

    /// Return the logical heap size in pages, excluding unrelated memory writes.
    pub fn memory_size(&self) -> usize {
        self.pages
    }

    /// Restore the initial empty logical heap and discard materialized bytes.
    pub fn reset(&mut self) {
        self.memory.clear();
        self.pages = 0;
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

    /// Read `len` bytes from memory starting at `addr`.
    ///
    /// Returns an error if `addr` or the end address is out of bounds.
    pub fn read_memory_bytes(
        &self,
        addr: u32,
        len: u32,
        at: SourceSpan,
    ) -> Result<Vec<u8>, Report> {
        let addr = addr as usize;
        if addr > MAX_ADDRESSABLE_HEAP {
            return Err(ReadFailed::AddressOutOfBounds {
                addr: addr as u32,
                at,
            })
            .wrap_err("invalid memory read");
        }

        let len = len as usize;
        let end_addr = addr.checked_add(len);
        if end_addr.is_none_or(|addr| addr > MAX_ADDRESSABLE_HEAP) {
            return Err(ReadFailed::SizeOutOfBounds {
                addr: addr as u32,
                size: len as u32,
                at,
            })
            .wrap_err("invalid memory read");
        }

        let mut bytes = Vec::with_capacity(len);
        for offset in 0..len {
            bytes.push(memory::read_byte(addr + offset, &self.memory));
        }

        Ok(bytes)
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
            memory: Vec::new(),
            pages: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_is_additive_and_preserves_data() {
        let mut context = ExecutionContext::default();
        context.memory_grow(2);
        context
            .write_memory(0, midenc_hir::Immediate::U8(17), SourceSpan::UNKNOWN)
            .unwrap();
        context.memory_grow(0);
        assert_eq!(context.memory_size(), 2);
        assert_eq!(context.memory[0], 17);
        context.memory_grow(1);
        assert_eq!(context.memory_size(), 3);
        assert_eq!(context.memory[0], 17);
    }

    #[test]
    fn materialized_bytes_do_not_change_logical_pages() {
        let mut context = ExecutionContext::default();
        context.memory.resize(2 * PAGE_SIZE, 0);
        assert_eq!(context.memory_size(), 0);
    }

    #[test]
    fn failed_growth_preserves_pages_and_bytes() {
        let mut context = ExecutionContext::default();
        assert!(context.memory_grow(1));
        context
            .write_memory(0, midenc_hir::Immediate::U8(17), SourceSpan::UNKNOWN)
            .unwrap();
        assert!(!context.memory_grow(usize::MAX));
        assert!(!context.memory_grow(MAX_ADDRESSABLE_HEAP / PAGE_SIZE));
        assert_eq!(context.memory_size(), 1);
        assert_eq!(context.memory[0], 17);
    }

    #[test]
    fn growth_uses_the_byte_addressable_heap_limit() {
        let mut context = ExecutionContext::default();
        assert!(context.memory_grow(16_384));
        assert_eq!(context.memory_size(), 16_384);
        assert!(context.memory_grow(65_535 - 16_384));
        assert_eq!(context.memory_size(), 65_535);
        assert!(!context.memory_grow(1));
        assert_eq!(context.memory_size(), 65_535);
        assert!(context.memory.is_empty());

        let mut empty = ExecutionContext::default();
        assert!(!empty.memory_grow(65_536));
        assert_eq!(empty.memory_size(), 0);
    }

    #[test]
    fn high_byte_addresses_read_as_zero_without_materializing_memory() {
        let context = ExecutionContext::default();
        for addr in [1u32 << 30, 0xffff_fff8] {
            assert_eq!(
                context.read_memory(addr, &Type::U32, SourceSpan::UNKNOWN).unwrap(),
                Value::Immediate(midenc_hir::Immediate::U32(0))
            );
            assert_eq!(context.read_memory_bytes(addr, 4, SourceSpan::UNKNOWN).unwrap(), [0; 4]);
        }
        assert!(context.read_memory(0xffff_fffc, &Type::U8, SourceSpan::UNKNOWN).is_err());
        assert!(context.memory.is_empty());
    }

    #[test]
    fn reset_restores_initial_page_count() {
        let mut context = ExecutionContext::default();
        let initial_size = context.memory_size();
        context.memory_grow(2);
        context.reset();
        assert_eq!(context.memory_size(), initial_size);
    }
}
