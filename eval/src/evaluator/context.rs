use alloc::{collections::BTreeMap, vec::Vec};

use midenc_hir::{AddressSpace, Felt, Report, SourceSpan, SymbolName, Type};
use midenc_session::diagnostics::WrapErr;

use super::memory::{self, Cells, MemoryAddress, ReadFailed, WriteFailed};
use crate::Value;

const PAGE_SIZE: usize = 64 * 1024;
// Match HEAP_END in codegen/masm/intrinsics/mem.masm: convert the last usable
// element address to the byte-addressed heap boundary used by the evaluator.
const MAX_ADDRESSABLE_HEAP: usize = (2usize.pow(30) - 1) * 4;

/// The execution context associated with Miden context boundaries
#[derive(Default)]
pub struct ExecutionContext {
    /// The name of the component this context belongs to, if known
    ///
    /// The root context never has an identifier
    #[allow(unused)]
    id: Option<SymbolName>,
    /// Heap memory
    memory: BTreeMap<u32, Felt>,
    /// Pages requested through memory_grow; independent of materialized bytes.
    pages: usize,
}

impl ExecutionContext {
    /// Creates an empty execution context for the component named `id`.
    pub fn new(id: SymbolName) -> Self {
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

    /// Bounds are expressed in the originating pointer's units. Native addresses span the full
    /// VM cell space; the dynamic heap limit applies only to heap growth and byte pointers.
    fn range_is_valid(addr: MemoryAddress, len: usize) -> bool {
        let limit = match addr.space {
            AddressSpace::Byte => MAX_ADDRESSABLE_HEAP as u64,
            AddressSpace::Element => (u64::from(u32::MAX) + 1) * 4,
        };
        addr.position().checked_add(len as u64).is_some_and(|end| end <= limit)
    }

    pub fn check_read_bounds(
        &self,
        addr: MemoryAddress,
        len: usize,
        at: SourceSpan,
    ) -> Result<(), Report> {
        if !Self::range_is_valid(addr, 0) {
            return Err(ReadFailed::AddressOutOfBounds {
                addr: addr.raw(),
                at,
            })
            .wrap_err("invalid memory read");
        }
        if !Self::range_is_valid(addr, len) {
            return Err(ReadFailed::SizeOutOfBounds {
                addr: addr.raw(),
                size: len as u32,
                at,
            })
            .wrap_err("invalid memory read");
        }
        Ok(())
    }

    pub fn check_write_bounds(
        &self,
        addr: impl Into<MemoryAddress>,
        len: usize,
        at: SourceSpan,
    ) -> Result<(), Report> {
        let addr = addr.into();
        if !Self::range_is_valid(addr, 0) {
            return Err(WriteFailed::AddressOutOfBounds {
                addr: addr.raw(),
                at,
            })
            .wrap_err("invalid memory write");
        }
        if !Self::range_is_valid(addr, len) {
            return Err(WriteFailed::SizeOutOfBounds {
                addr: addr.raw(),
                size: len as u32,
                at,
            })
            .wrap_err("invalid memory write");
        }
        Ok(())
    }

    /// Read a typed value, interpreting the address according to its pointer address space.
    pub fn read_memory(
        &self,
        addr: impl Into<MemoryAddress>,
        ty: &Type,
        at: SourceSpan,
    ) -> Result<Value, Report> {
        let addr = addr.into();
        self.check_read_bounds(addr, ty.size_in_bytes(), at)?;
        memory::read_value(addr, ty, &self.memory).wrap_err("invalid memory read")
    }

    /// Read bytes from the u32 view of memory, rejecting cells which do not fit that view.
    pub fn read_memory_bytes(
        &self,
        addr: impl Into<MemoryAddress>,
        len: u32,
        at: SourceSpan,
    ) -> Result<Vec<u8>, Report> {
        let addr = addr.into();
        self.check_read_bounds(addr, len as usize, at)?;
        (0..len)
            .map(|offset| {
                memory::read_byte(
                    addr.checked_add(u64::from(offset)).expect("range was checked"),
                    &self.memory,
                )
                .wrap_err("invalid memory read")
            })
            .collect()
    }

    /// Write a typed value. A Felt replaces one complete cell; integers use little-endian limbs.
    pub fn write_memory(
        &mut self,
        addr: impl Into<MemoryAddress>,
        value: impl Into<Value>,
        at: SourceSpan,
    ) -> Result<(), Report> {
        let addr = addr.into();
        let value = value.into();
        self.check_write_bounds(addr, value.ty().size_in_bytes(), at)?;
        memory::write_value(addr, value, &mut self.memory).wrap_err("invalid memory write")
    }

    /// Write bytes without modifying neighboring bytes. Validate the whole write before mutation.
    pub fn write_memory_bytes(
        &mut self,
        addr: impl Into<MemoryAddress>,
        bytes: &[u8],
        at: SourceSpan,
    ) -> Result<(), Report> {
        let addr = addr.into();
        self.check_write_bounds(addr, bytes.len(), at)?;
        memory::write_bytes(addr, bytes, &mut self.memory).wrap_err("invalid memory write")
    }

    /// Snapshot complete cells containing an element-aligned byte range, including hidden values.
    pub fn read_memory_elements(
        &self,
        addr: MemoryAddress,
        len: u32,
        at: SourceSpan,
    ) -> Result<Vec<Felt>, Report> {
        self.check_read_bounds(addr, len as usize, at)?;
        if addr.offset != 0 {
            return Err(ReadFailed::UnalignedElement).wrap_err("invalid memory read");
        }
        Ok((0..len.div_ceil(4))
            .map(|offset| Cells::get(&self.memory, addr.element + offset))
            .collect())
    }

    /// Write a snapshot to an element-aligned byte range. The exact length excludes snapshot
    /// padding: complete cells are replaced, while a partial final cell preserves its other bytes.
    pub fn write_memory_elements(
        &mut self,
        addr: MemoryAddress,
        len: u32,
        elements: &[Felt],
        at: SourceSpan,
    ) -> Result<(), Report> {
        self.check_write_bounds(addr, len as usize, at)?;
        if addr.offset != 0 {
            return Err(WriteFailed::UnalignedElement).wrap_err("invalid memory write");
        }
        if elements.len() != len.div_ceil(4) as usize {
            return Err(WriteFailed::InvalidSnapshot).wrap_err("invalid memory write");
        }
        // Write the only fallible part first, so a bad boundary cell leaves the whole range intact.
        let tail = (len % 4) as usize;
        if tail != 0 {
            let value = elements[elements.len() - 1].as_canonical_u64();
            let bytes = u32::try_from(value)
                .map_err(|_| WriteFailed::InvalidElement(value))
                .wrap_err("invalid memory write")?
                .to_le_bytes();
            let tail_addr = addr.checked_add(u64::from(len - len % 4)).expect("range was checked");
            memory::write_bytes(tail_addr, &bytes[..tail], &mut self.memory)
                .wrap_err("invalid memory write")?;
        }
        for (offset, &element) in elements[..(len / 4) as usize].iter().enumerate() {
            self.memory.set(addr.element + offset as u32, element);
        }
        Ok(())
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
        assert_eq!(Cells::get(&context.memory, 0), Felt::from(17u32));
        context.memory_grow(1);
        assert_eq!(context.memory_size(), 3);
        assert_eq!(Cells::get(&context.memory, 0), Felt::from(17u32));
    }

    #[test]
    fn materialized_bytes_do_not_change_logical_pages() {
        let mut context = ExecutionContext::default();
        context
            .write_memory(
                (2 * PAGE_SIZE) as u32,
                midenc_hir::Immediate::U8(17),
                SourceSpan::UNKNOWN,
            )
            .unwrap();
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
        assert_eq!(Cells::get(&context.memory, 0), Felt::from(17u32));
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

    /// Checks that `write_memory_bytes` writes every byte of the range it is given.
    #[test]
    fn write_memory_bytes_writes_the_whole_range() {
        let mut context = ExecutionContext::default();
        let bytes = [1u8, 2, 3, 4, 5, 6, 7];
        context.write_memory_bytes(13, &bytes, SourceSpan::UNKNOWN).unwrap();
        assert_eq!(
            context.read_memory_bytes(13, bytes.len() as u32, SourceSpan::UNKNOWN).unwrap(),
            bytes
        );
    }

    /// Checks that a `write_memory_bytes` range crossing the end of the addressable heap is
    /// rejected without writing any of its bytes.
    #[test]
    fn write_memory_bytes_out_of_bounds_writes_nothing() {
        let mut context = ExecutionContext::default();
        let addr = (MAX_ADDRESSABLE_HEAP - 2) as u32;
        assert!(context.write_memory_bytes(addr, &[0xab; 8], SourceSpan::UNKNOWN).is_err());
        assert!(context.memory.is_empty());
    }
}
