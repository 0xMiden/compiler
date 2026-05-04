#![no_std]
#![feature(optimize_attribute)]

// Provide a local definition for the allocator to link against, and export it
// under the MASM intrinsic path so the frontend recognizes and lowers it.

/// Unreachable stub for `intrinsics::mem::heap_base`.
#[unsafe(export_name = "intrinsics::mem::heap_base")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn __intrinsics_mem_heap_base_stub() -> *mut u8 {
    unsafe { core::hint::unreachable_unchecked() }
}
