// Signed 64-bit remainder with a dynamic divisor, including negative numerators.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = (((input1 as u64) << 32) | input2 as u64) as i64;
    // Divisor in [1, 1000] avoids native division traps.
    let d = ((input2 % 1000) as i64) + 1;
    let r = n % d;
    (r as u32) ^ ((r >> 32) as u32) ^ (n as u32)
}
