// Runtime i128 signed division via compiler-builtins `__divti3` (sign fixup
// around the unsigned core): a dynamic ODD numerator (odd => never i128::MIN)
// divided by a dynamic positive and a dynamic negative divisor, so both
// quotient sign-fixup paths execute. Divisors are non-zero by construction and
// MIN/-1 overflow is impossible (the numerator is never MIN).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = (input2 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ input1 as u64;
    // Odd 128-bit numerator; its sign varies with input1's top bit.
    let n: i128 = ((((a as u128) << 64) | b as u128) as i128) | 1;

    let dp: i128 = ((input2 % 0xF_FFFF) as i128) + 1; // in [1, 0xF_FFFF]
    let dn: i128 = -(((input1 % 0xFFFD) as i128) + 1); // in [-0xFFFD, -1]

    let q1 = n / dp;
    let q2 = n / dn;

    let m = (q1 as u64) ^ ((q1 >> 64) as u64) ^ (q2 as u64) ^ ((q2 >> 64) as u64);
    (m as u32) ^ ((m >> 32) as u32)
}
