//! A typed element address: a pointer in the Miden VM's element-addressable space.

use core::{cmp::Ordering, fmt, hash, marker::PhantomData};

/// An address in the element address space, with the type it points at.
///
/// Rust pointers are byte addresses; a Miden procedure that takes or returns a pointer works in
/// elements (4 bytes each). Generated bindings take and return an `ElementPtr` as it is; the
/// code whose data lives in Rust memory converts, with the two checked conversions here: see
/// [`Self::from_ptr`] and [`Self::to_ptr`].
///
/// An `ElementPtr` is a `u32` address: whatever `T` is, it is `Copy`, `Send` and `Sync`, and it
/// compares, orders and hashes by address.
#[repr(transparent)]
pub struct ElementPtr<T> {
    addr: u32,
    // `fn() -> T`, not `*mut T`: the address neither owns nor shares a `T`, so it is `Send`,
    // `Sync` and covariant in `T` whatever `T` is.
    _marker: PhantomData<fn() -> T>,
}

impl<T> ElementPtr<T> {
    /// Wraps an element address.
    pub const fn new(addr: u32) -> Self {
        Self {
            addr,
            _marker: PhantomData,
        }
    }

    /// The element address.
    pub const fn addr(self) -> u32 {
        self.addr
    }

    /// Converts a byte address to an element address.
    ///
    /// Exposes the provenance of `ptr`, so that [`Self::to_ptr`] gives back a pointer that can
    /// access the memory `ptr` could: for example a buffer handed to a procedure, at an address
    /// the procedure returned into it.
    ///
    /// # Panics
    ///
    /// If `ptr` is not 4-byte aligned: only element-aligned byte addresses have an element
    /// address. On a target with pointers wider than 32 bits, also if the element address does
    /// not fit in 32 bits (on wasm32 it always does).
    pub fn from_ptr(ptr: *mut T) -> Self {
        let addr = ptr.expose_provenance();
        assert!(addr.is_multiple_of(4), "pointer {addr:#x} is not element-aligned");
        let element = u32::try_from(addr / 4)
            .unwrap_or_else(|_| panic!("pointer {addr:#x} has no 32-bit element address"));
        Self::new(element)
    }

    /// Converts an element address back to a byte address.
    ///
    /// The pointer picks up exposed provenance, which [`Self::from_ptr`] exposes: Rust code can
    /// access memory through it when the address is inside an allocation that a pointer passed to
    /// [`Self::from_ptr`] pointed into.
    ///
    /// # Panics
    ///
    /// If the byte address does not fit in 32 bits: the upper part of the element address space
    /// is not addressable in byte space. Also if the byte address is not aligned for `T`, which
    /// only a `T` aligned above an element's 4 bytes can hit, such as a [`Word`](crate::Word) on
    /// the Miden target.
    pub fn to_ptr(self) -> *mut T {
        let addr = (self.addr as u64) * 4;
        let addr = u32::try_from(addr).unwrap_or_else(|_| {
            panic!("element address {:#x} has no 32-bit byte address", self.addr)
        });
        let align = align_of::<T>();
        assert!(
            (addr as usize).is_multiple_of(align),
            "element address {:#x} (byte address {addr:#x}) is not {align}-byte aligned",
            self.addr
        );
        core::ptr::with_exposed_provenance_mut(addr as usize)
    }
}

// The traits are implemented by hand, not derived: a derive would require `T` to implement each
// of them too, while the address is a `u32` whatever `T` is.

impl<T> Clone for ElementPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ElementPtr<T> {}

impl<T> fmt::Debug for ElementPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementPtr")
            .field("addr", &format_args!("{:#x}", self.addr))
            .finish()
    }
}

impl<T> PartialEq for ElementPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.addr == other.addr
    }
}

impl<T> Eq for ElementPtr<T> {}

impl<T> PartialOrd for ElementPtr<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for ElementPtr<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.addr.cmp(&other.addr)
    }
}

impl<T> hash::Hash for ElementPtr<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.addr.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use core::{cell::Cell, fmt::Debug, hash::Hash, ptr::without_provenance_mut};

    use super::ElementPtr;

    /// Aligned like a `Word` on the Miden target: above an element's 4 bytes.
    #[repr(align(16))]
    struct Align16;

    /// Neither `Copy` nor `Sync`.
    type NotCopyOrSync = Cell<u8>;

    /// Compiles only if an `ElementPtr` is `Sync` whatever it points at.
    static NOT_COPY_OR_SYNC: ElementPtr<NotCopyOrSync> = ElementPtr::new(0x10);

    #[test]
    fn an_aligned_byte_address_round_trips() {
        let ptr = without_provenance_mut::<u32>(0x40);
        let element = ElementPtr::from_ptr(ptr);
        assert_eq!(element.addr(), 0x10);
        assert_eq!(element.to_ptr(), ptr);

        assert_eq!(ElementPtr::<u32>::from_ptr(without_provenance_mut(0)).addr(), 0);
        assert_eq!(ElementPtr::<u8>::new(0x10).to_ptr().addr(), 0x40);
    }

    #[test]
    #[should_panic(expected = "pointer 0x41 is not element-aligned")]
    fn an_unaligned_byte_address_panics() {
        ElementPtr::<u8>::from_ptr(without_provenance_mut(0x41));
    }

    #[test]
    fn the_highest_element_address_with_a_byte_address_converts() {
        assert_eq!(ElementPtr::<u8>::new(0x3fff_ffff).to_ptr().addr(), 0xffff_fffc);
    }

    #[test]
    #[should_panic(expected = "element address 0x40000000 has no 32-bit byte address")]
    fn an_element_address_past_the_byte_space_panics() {
        ElementPtr::<u8>::new(0x4000_0000).to_ptr();
    }

    /// Host pointers can be wider than wasm32's; such an address must not be truncated.
    #[cfg(target_pointer_width = "64")]
    #[test]
    #[should_panic(expected = "pointer 0x400000000 has no 32-bit element address")]
    fn a_byte_address_past_the_element_space_panics() {
        ElementPtr::<u8>::from_ptr(without_provenance_mut(0x4_0000_0000));
    }

    #[test]
    fn an_element_address_aligned_for_the_type_converts() {
        assert_eq!(ElementPtr::<Align16>::new(0).to_ptr().addr(), 0);
        assert_eq!(ElementPtr::<Align16>::new(0x4).to_ptr().addr(), 0x10);
    }

    /// A `*mut T` must be aligned for `T`; an element address is only 4-byte aligned.
    #[test]
    #[should_panic(expected = "element address 0x2 (byte address 0x8) is not 16-byte aligned")]
    fn an_element_address_misaligned_for_the_type_panics() {
        ElementPtr::<Align16>::new(0x2).to_ptr();
    }

    #[test]
    fn the_address_is_copy_and_shareable_whatever_it_points_at() {
        fn by_address<T: Copy + Send + Sync + Debug + Ord + Hash>(value: T) -> T {
            value
        }
        fn covariant<'a>(ptr: ElementPtr<&'static u8>) -> ElementPtr<&'a u8> {
            ptr
        }

        let copy = by_address(NOT_COPY_OR_SYNC);
        assert_eq!(copy, NOT_COPY_OR_SYNC);
        assert!(copy < ElementPtr::new(0x11));
        assert_eq!(covariant(ElementPtr::new(0x10)).addr(), 0x10);
    }
}
