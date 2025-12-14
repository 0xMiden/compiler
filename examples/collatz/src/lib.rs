// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::vec::Vec;

// // Global allocator to use heap memory in no-std environment
// #[global_allocator]
// static ALLOC: BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_failed(_layout: core::alloc::Layout) -> ! {
    loop {}
}

// Pass up to 16 u32 inputs as entrypoint function parameters.
// The output is temporarely limited to 1 u32 value
//
// NOTE:
// The name of the entrypoint function is expected to be `entrypoint`. Do not remove the
// `#[no_mangle]` attribute, otherwise, the rustc will mangle the name and it'll not be recognized
// by the Miden compiler.
#[no_mangle]
fn entrypoint(mut n: u32) -> u32 {
    let mut steps = 0;
    while n != 1 {
        if n % 2 == 0 {
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    steps
}
