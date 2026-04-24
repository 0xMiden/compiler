#![no_std]
#![feature(alloc_error_handler)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_failed(_layout: core::alloc::Layout) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    input1.wrapping_sub(input2)
}
