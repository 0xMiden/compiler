#![no_std]
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

/// We require all allocations to be minimally word-aligned, i.e. 16 byte alignment
const MIN_ALIGN: usize = 16;

/// The linear memory heap must not spill over into the region reserved for procedure locals, which
/// begins at 2^30 in Miden's address space. In Rust address space it should be 2^30 * 4 but since
/// it overflows the usize which is 32-bit on wasm32 we use u32::MAX.
const HEAP_END: *mut u8 = u32::MAX as *mut u8;

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
            self.top.store(unsafe { base.byte_add(size * PAGE_SIZE) }, Ordering::Relaxed);
        }
        // TODO: Once treeify issue is fixed, switch to this implementation
        /*
        let _ = self.top.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |top| {
            if top.is_null() {
                let base = unsafe { heap_base() };
                let size = core::arch::wasm32::memory_size(0);
                Some(unsafe { base.byte_add(size * PAGE_SIZE) })
            } else {
                None
            }
        });
        */
    }

    #[cfg(not(target_family = "wasm"))]
    fn maybe_init(&self) {}
}

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Force allocations to be at minimally word-aligned. This is wasteful of memory, but
        // we don't need to be particularly conservative with memory anyway, as most, if not all,
        // Miden programs will be relatively short-lived. This makes interop at the Rust/Miden
        // call boundary less expensive, as we can typically pass pointers directly to Miden,
        // whereas without this alignment guarantee, we would have to set up temporary buffers for
        // Miden code to write to, and then copy out of that buffer to whatever Rust type, e.g.
        // `Vec`, we actually want.
        //
        // NOTE: This cannot fail, because we're always meeting minimum alignment requirements
        let layout = layout
            .align_to(core::cmp::max(layout.align(), MIN_ALIGN))
            .unwrap()
            .pad_to_align();
        let size = layout.size();
        let align = layout.align();

        self.maybe_init();

        let top = self.top.load(Ordering::Relaxed);
        let available = unsafe { HEAP_END.byte_offset_from(top) as usize };
        if available >= size {
            unsafe {
                self.top.store(top.byte_add(size), Ordering::Relaxed);
                top.byte_offset(align as isize)
            }
        } else {
            null_mut()
        }

        // TODO: Once treeify issue is fixed, switch to this implementation
        /*
        match self.top.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |top| {
            let available = HEAP_END.byte_offset_from(top) as usize;
            if available < size {
                None
            } else {
                Some(top.byte_add(size))
            }
        }) {
            Ok(prev_top) => {
                unsafe { prev_top.byte_offset(align as isize) }
            }
            Err(_) => null_mut(),
        }
         */
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[cfg(target_family = "wasm")]
unsafe extern "C" {
    #[link_name = "intrinsics::mem::heap_base"]
    fn heap_base() -> *mut u8;
}
