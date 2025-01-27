// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// use miden::Felt;

#[no_mangle]
pub fn entrypoint(a: i32, b: i32) -> i32 {
    a + b
}
