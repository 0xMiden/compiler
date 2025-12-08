//! Test fixture for verifying Rust assert! macro source location preservation.
//!
//! This test verifies that debug source location information is correctly
//! preserved from Rust source code through to MASM compilation and execution.

#![no_std]
#![feature(alloc_error_handler)]

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {
    loop {}
}

/// Main entrypoint that tests assertion with source location tracking.
/// When x > 100, returns x. When x <= 100, panics with assertion failure.
#[no_mangle]
pub fn entrypoint(x: u32) -> u32 {
    assert!(x > 100);
    x
}
