// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Required for no-std crates
#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {
    loop {}
}

// use miden::Felt;

#[unsafe(no_mangle)]
pub fn entrypoint(a: u32, b: u32) -> u32 {
    a + b
}
