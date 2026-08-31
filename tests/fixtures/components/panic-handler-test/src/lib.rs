//! Test fixture for verifying custom panic handler invocation.

#![no_std]
#![feature(alloc_error_handler)]

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    // TODO use panic infra once it was added
    miden::println!("custom panic handler invoked");
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {
    loop {}
}

/// Main entrypoint: returns `x` when `x > 100`, panics otherwise.
#[no_mangle]
pub fn entrypoint(x: u32) -> u32 {
    assert!(x > 100, "input smaller than threshold");
    x
}
