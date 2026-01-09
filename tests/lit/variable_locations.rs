#![no_std]
#![no_main]
#![allow(unused_unsafe)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::wasm32::unreachable() }
}

#[no_mangle]
pub extern "C" fn entrypoint(n: u32) -> u32 {
    let mut sum = 0u32;
    let mut i = 0u32;
    while i <= n {
        sum = sum + i;
        i = i + 1;
    }
    sum
}
