#![no_std]
#![no_main]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sum_arr(arr: &[u32]) -> u32 {
    arr.iter().sum()
}

#[unsafe(no_mangle)]
pub extern "C" fn __main() -> u32 {
    sum_arr(&[1, 2, 3, 4, 5]) + sum_arr(&[6, 7, 8, 9, 10])
}
