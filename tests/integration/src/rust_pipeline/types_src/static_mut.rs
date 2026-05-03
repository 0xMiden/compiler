#![no_std]
#![no_main]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

static mut G1: [u8; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];

#[inline(never)]
#[unsafe(no_mangle)]
fn global_var_update() {
    unsafe {
        G1[0] = G1[1] + 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __main() -> u32 {
    global_var_update();
    unsafe { G1.into_iter().sum::<u8>() as u32 }
}
