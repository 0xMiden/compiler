//! Fixture for observing whether a Rust panic invokes the custom handler.

#![no_std]
#![feature(alloc_error_handler)]

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    miden::println!("custom panic handler invoked");
    if let Some(message) = info.message().as_str() {
        miden::println!(message);
    }
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    core::arch::wasm32::unreachable()
}

/// Returns the input above the threshold and panics otherwise.
#[unsafe(no_mangle)]
pub fn entrypoint(x: u32) -> u32 {
    assert!(x > 100, "input smaller than threshold");
    x
}
