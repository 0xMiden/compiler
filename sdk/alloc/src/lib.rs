#![no_std]
#![cfg_attr(target_family = "wasm", feature(linkage))]
#![deny(warnings)]

extern crate alloc;

use alloc::alloc::{GlobalAlloc, Layout};
use core::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

/// We assume the Wasm page size for purposes of initializing the heap
#[cfg(target_family = "wasm")]
const PAGE_SIZE: usize = 2usize.pow(16);

/// Keep buffers word-aligned (16 bytes) so Rust/Miden FFI can pass them directly without copies.
const MIN_ALIGN: usize = 16;

/// Exclusive byte-address limit for allocations. Heap metadata starts at VM element
/// address 2^30, beyond the 32-bit byte-addressable range. Using u32::MAX avoids
/// representing the unaddressable 2^32 endpoint on wasm32.
const HEAP_END: usize = u32::MAX as usize;

/// A very simple allocator for Miden SDK-based programs.
///
/// This allocator does not free memory, it simply grows the heap until it runs out of available
/// space for further allocations.
pub struct BumpAlloc {
    /// The address at which the available heap begins
    top: AtomicPtr<u8>,
}

impl Default for BumpAlloc {
    fn default() -> Self {
        Self::new()
    }
}

impl BumpAlloc {
    /// Create a new instance of this allocator
    ///
    /// NOTE: Only one instance of this allocator should ever be used at a time, as it is
    /// allocating from the global heap, not from memory reserved for itself.
    pub const fn new() -> Self {
        Self {
            top: AtomicPtr::new(null_mut()),
        }
    }

    /// Initialize the allocator, if it has not yet been initialized
    #[cfg(target_family = "wasm")]
    fn maybe_init(&self) {
        let top = self.top.load(Ordering::Relaxed);
        if top.is_null() {
            let base = unsafe { heap_base() };
            let size = core::arch::wasm32::memory_size(0);
            let top = size
                .checked_mul(PAGE_SIZE)
                .and_then(|size| base.addr().checked_add(size))
                .filter(|top| *top <= HEAP_END)
                .unwrap_or(HEAP_END);
            self.top.store(base.with_addr(top), Ordering::Relaxed);
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn maybe_init(&self) {}
}

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.maybe_init();

        let top = self.top.load(Ordering::Relaxed);
        let Some(range) = allocation_range(top.addr(), HEAP_END, layout) else {
            return null_mut();
        };

        // Preserve the heap pointer's provenance without requiring intermediate pointer
        // arithmetic to stay within an allocation. The checked range covers every returned byte.
        self.top.store(top.with_addr(range.end), Ordering::Relaxed);
        top.with_addr(range.start)
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

/// Reserve an aligned range in the target's byte-addressable heap.
///
/// Addresses are integers while checking bounds: the heap limit is not a pointer into the
/// allocation, so subtracting it from the heap pointer would violate pointer provenance rules.
fn allocation_range(top: usize, limit: usize, layout: Layout) -> Option<core::ops::Range<usize>> {
    if top == 0 {
        return None;
    }
    let align = core::cmp::max(layout.align(), MIN_ALIGN);
    let start = top.checked_add(align - 1)? & !(align - 1);
    let end = start.checked_add(layout.size())?;
    (end <= limit).then_some(start..end)
}

#[cfg(test)]
mod tests;

#[cfg(target_family = "wasm")]
unsafe extern "C" {
    #[linkage = "extern_weak"]
    #[link_name = "intrinsics::mem::heap_base"]
    fn heap_base() -> *mut u8;
}
