use alloc::{
    alloc::{Allocator, Layout},
    rc::Rc,
};
use core::ptr::NonNull;

/// This is a simple wrapper around [blink_alloc::Blink] that allows it to be used as an allocator
/// with standard library collections such as [alloc::collections::VecDeque], without binding the
/// lifetime of the collection to the allocator.
#[derive(Default, Clone)]
pub struct DataFlowSolverAlloc(Rc<blink_alloc::Blink>);

impl core::ops::Deref for DataFlowSolverAlloc {
    type Target = blink_alloc::Blink;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

unsafe impl Allocator for DataFlowSolverAlloc {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, alloc::alloc::AllocError> {
        self.0.allocator().allocate(layout)
    }

    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        <blink_alloc::BlinkAlloc as Allocator>::deallocate(self.0.allocator(), ptr, layout)
    }

    #[inline]
    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, alloc::alloc::AllocError> {
        self.0.allocator().allocate_zeroed(layout)
    }

    #[inline]
    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, alloc::alloc::AllocError> {
        self.0.allocator().grow(ptr, old_layout, new_layout)
    }

    #[inline]
    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, alloc::alloc::AllocError> {
        self.0.allocator().shrink(ptr, old_layout, new_layout)
    }

    #[inline]
    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, alloc::alloc::AllocError> {
        self.0.allocator().grow_zeroed(ptr, old_layout, new_layout)
    }
}
