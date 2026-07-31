// Runtime i128 signed remainder via compiler-builtins `__modti3`
// (truncate-toward-zero: remainder takes the dividend's sign): a dynamic ODD
// numerator (odd => never i128::MIN) against a dynamic positive and a dynamic
// negative divisor. Divisors are non-zero by construction and MIN % -1 is
// impossible (the numerator is never MIN).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input2 as u64) << 32) | input1 as u64;
    let b: u64 = (input1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ input2 as u64;
    // Odd 128-bit numerator; its sign varies with input2's top bit.
    let n: i128 = ((((a as u128) << 64) | b as u128) as i128) | 1;

    let dp: i128 = ((input1 % 0xE_EEED) as i128) + 1; // in [1, 0xE_EEED]
    let dn: i128 = -(((input2 % 0xFFF1) as i128) + 1); // in [-0xFFF1, -1]

    let r1 = n % dp;
    let r2 = n % dn;

    let m = (r1 as u64) ^ ((r1 >> 64) as u64) ^ (r2 as u64) ^ ((r2 >> 64) as u64);
    (m as u32) ^ ((m >> 32) as u32)
}
