#![no_std]

// Provide a local definition for the allocator to link against, and export it
// under the MASM intrinsic path so the frontend recognizes and lowers it.

/// Returns an opaque default value from a linker stub.
#[track_caller]
#[inline(never)]
fn stub<T: Default>() -> T {
    core::hint::black_box(core::panic::Location::caller());
    core::hint::black_box(T::default())
}

#[unsafe(export_name = "intrinsics::mem::heap_base")]
#[inline(never)]
pub extern "C" fn __intrinsics_mem_heap_base_stub() -> *mut u8 {
    stub()
}
