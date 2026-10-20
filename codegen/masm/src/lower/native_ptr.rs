/// This represents a descriptor for a pointer translated from the IR into a form suitable for
/// referencing data in Miden's linear memory.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NativePtr {
    /// This is the address of the element containing the first byte of data
    ///
    /// Each element is assumed to be a 32-bit value/chunk
    pub addr: u32,
    /// This is the byte offset into the 32-bit chunk referenced by `index`
    ///
    /// This offset is where the data referenced by the pointer actually starts.
    pub offset: u8,
    /// This is the assumed address space of the pointer value.
    ///
    /// This address space is unknown by default, but can be specified if known statically.
    /// The address space determines whether the pointer is valid in certain contexts. For
    /// example, attempting to load a pointer with address space 0 is invalid if not operating
    /// in the root context.
    ///
    /// Currently this has no effect, but is here as we expand support for multiple memories.
    pub addrspace: midenc_hir::AddressSpace,
}

impl NativePtr {
    pub fn new(addr: u32, offset: u8) -> Self {
        Self {
            addr,
            offset,
            addrspace: Default::default(),
        }
    }

    /// Translates a raw pointer (assumed to be in a byte-addressable address space) to a native
    /// pointer value, in the default [midenc_hir::AddressSpace].
    pub fn from_ptr(addr: u32) -> Self {
        // The native word address for `addr` is derived by splitting the byte-addressable space
        // into 32-bit chunks, each chunk belonging to a single field element, i.e. each element
        // of the native address space represents 32 bits of byte-addressable memory.
        //
        // By dividing `addr` by 4, we get the element address where the data starts.
        let eaddr = addr / 4;
        // If our address is not element-aligned, we need to determine what byte offset contains
        // the first byte of the data.
        let offset = (addr % 4) as u8;
        Self {
            addr: eaddr,
            offset,
            addrspace: Default::default(),
        }
    }

    /// Returns true if this pointer is aligned to a word boundary
    pub const fn is_word_aligned(&self) -> bool {
        self.offset == 0 && self.addr.is_multiple_of(4)
    }

    /// Returns true if this pointer is aligned to a field element boundary
    pub const fn is_element_aligned(&self) -> bool {
        self.offset == 0
    }

    /// Returns true if this pointer is not element aligned
    pub const fn is_unaligned(&self) -> bool {
        self.offset > 0
    }

    /// Returns the byte alignment implied by this pointer value.
    ///
    /// For example, a pointer to the first word in linear memory, i.e. address 1, with an offset
    /// of 2, is equivalent to an address in byte-addressable memory of 6, which has an implied
    /// alignment of 2 bytes.
    ///
    /// The result is capped at the largest power of two a `u32` holds, including for address 0.
    pub const fn alignment(&self) -> u32 {
        let byte_position = (self.addr as u64) * 4 + self.offset as u64;
        let trailing_zeros = byte_position.trailing_zeros();
        if trailing_zeros < u32::BITS {
            1 << trailing_zeros
        } else {
            1 << (u32::BITS - 1)
        }
    }

    /// Converts this native pointer back to a byte-addressable pointer value
    pub const fn as_ptr(&self) -> u32 {
        (self.addr * 4) + self.offset as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_preserves_byte_domain_results() {
        for (byte, expected) in [
            (0, 1 << 31),
            (1, 1),
            (2, 2),
            (3, 1),
            (4, 4),
            (8, 8),
            (12, 4),
            (16, 16),
            (0x7fff_fffc, 4),
            (0x8000_0000, 1 << 31),
            (u32::MAX, 1),
        ] {
            assert_eq!(NativePtr::from_ptr(byte).alignment(), expected, "byte address {byte}");
        }
    }

    #[test]
    fn alignment_covers_native_domain_and_normalized_offsets() {
        for (addr, aligned) in [
            (0, 1 << 31),
            (1, 4),
            (2, 8),
            (3, 4),
            (1 << 29, 1 << 31),
            (1 << 30, 1 << 31),
            (1 << 31, 1 << 31),
            (u32::MAX, 4),
        ] {
            for (offset, expected) in [(0, aligned), (1, 1), (2, 2), (3, 1)] {
                assert_eq!(
                    NativePtr::new(addr, offset).alignment(),
                    expected,
                    "element {addr}, offset {offset}"
                );
            }
        }
    }

    #[test]
    fn alignment_remains_const() {
        const ZERO: u32 = NativePtr {
            addr: 0,
            offset: 0,
            addrspace: midenc_hir::AddressSpace::Element,
        }
        .alignment();
        const HIGH: u32 = NativePtr {
            addr: 1 << 30,
            offset: 0,
            addrspace: midenc_hir::AddressSpace::Element,
        }
        .alignment();
        const TERMINAL_HALFWORD: u32 = NativePtr {
            addr: u32::MAX,
            offset: 2,
            addrspace: midenc_hir::AddressSpace::Element,
        }
        .alignment();
        assert_eq!(ZERO, 1 << 31);
        assert_eq!(HIGH, 1 << 31);
        assert_eq!(TERMINAL_HALFWORD, 2);
    }
}
