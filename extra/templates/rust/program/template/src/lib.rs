// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn my_alloc_error(_layout: core::alloc::Layout) -> ! {
    loop {}
}

// The entrypoint function must be named `entrypoint` and keep the
// `#[unsafe(no_mangle)]` attribute, otherwise rustc will mangle the name and the
// Miden compiler will not recognize it. Pass up to 16 u32 inputs as parameters;
// the output is currently limited to a single u32 value.
#[unsafe(no_mangle)]
pub fn entrypoint(a: u32, b: u32) -> u32 {
    // TODO: Implement your program logic here
    a + b
}
