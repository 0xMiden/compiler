use serde::{Deserialize, Serialize};

/// This represents a descriptor for a pointer translated from the IR into a form suitable for
/// referencing data in Miden's linear memory.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePtr {
    /// This is the address of the word containing the first byte of data
    pub waddr: u32,
    /// This is the element index of the word referenced by `waddr` containing the first byte of
    /// data
    ///
    /// Each element is assumed to be a 32-bit value/chunk
    pub index: u8,
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
    pub fn new(waddr: u32, index: u8, offset: u8) -> Self {
        Self {
            waddr,
            index,
            offset,
            addrspace: Default::default(),
        }
    }

    /// Translates a raw pointer (assumed to be in a byte-addressable address space) to
    /// a native pointer value, in the default [hir::AddressSpace].
    pub fn from_ptr(addr: u32) -> Self {
        // The native word address for `addr` is derived by splitting
        // the byte-addressable space into 32-bit chunks, each chunk
        // belonging to a single field element. Thus, each word of the
        // native address space represents 128 bits of byte-addressable
        // memory.
        //
        // By dividing `addr` by 16, we get the word index (i.e. address)
        // where the data starts.
        let waddr = addr / 16;
        // If our address is not word-aligned, we need to determine what
        // element index contains the 32-bit chunk where the data begins
        let woffset = addr % 16;
        let index = (woffset / 4) as u8;
        // If our address is not element-aligned, we need to determine
        // what byte offset contains the first byte of the data
        let offset = (woffset % 4) as u8;
        Self {
            waddr,
            index,
            offset,
            addrspace: Default::default(),
        }
    }

    /// Returns true if this pointer is aligned to a word boundary
    pub const fn is_word_aligned(&self) -> bool {
        self.index == 0 && self.offset == 0
    }

    /// Returns true if this pointer is aligned to a field element boundary
    pub const fn is_element_aligned(&self) -> bool {
        self.offset == 0
    }

    /// Returns true if this pointer is not word or element aligned
    pub const fn is_unaligned(&self) -> bool {
        self.offset > 0
    }

    /// Returns the byte alignment implied by this pointer value.
    ///
    /// For example, a pointer to the first word in linear memory, i.e. address 0,
    /// with an element index of 1, and offset of 16, is equivalent to an address
    /// in byte-addressable memory of 48, which has an implied alignment of 16 bytes.
    pub const fn alignment(&self) -> u32 {
        2u32.pow(self.as_ptr().trailing_zeros())
    }

    /// Converts this native pointer back to a byte-addressable pointer value
    pub const fn as_ptr(&self) -> u32 {
        (self.waddr * 16) + (self.index as u32 * 4) + self.offset as u32
    }
}
