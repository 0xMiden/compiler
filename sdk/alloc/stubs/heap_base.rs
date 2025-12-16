#![no_std]

// Provide a local definition for the allocator to link against, and export it
// under the MASM intrinsic path so the frontend recognizes and lowers it.

#[unsafe(export_name = "intrinsics::mem::heap_base")]
pub extern "C" fn __intrinsics_mem_heap_base_stub() -> *mut u8 {
    unsafe { core::hint::unreachable_unchecked() }
}
