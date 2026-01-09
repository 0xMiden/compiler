#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::wasm32::unreachable() }
}

#[no_mangle]
pub extern "C" fn add(a: u32, b: u32) -> u32 {
    a + b
}
