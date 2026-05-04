#![no_std]
#![no_main]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_assertion(x: u32) -> u32 {
    assert!(x > 100, "x should be greater than 100");

    x
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn entrypoint(x: u32) -> u32 {
    test_assertion(x)
}
