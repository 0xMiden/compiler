// Runtime u128 remainder via compiler-builtins `__umodti3` (compiled into the
// guest wasm): a full-128-bit dividend against a small (u64-range) divisor and
// a full-width divisor, so both remainder paths run on every input. Divisors
// are non-zero by construction (`| 1`).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) ^ (input2 as u64).wrapping_mul(0x0101_0193);
    let b: u64 = (input2 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ input1 as u64;
    let n: u128 = ((a as u128) << 64) | b as u128;

    // Small divisor (fits in u64).
    let r1 = n % ((a | 1) as u128);
    // Full-width divisor (limbs swapped relative to `n`).
    let d2: u128 = (((b as u128) << 64) | a as u128) | 1;
    let r2 = n % d2;

    let m = (r1 as u64) ^ ((r1 >> 64) as u64) ^ (r2 as u64) ^ ((r2 >> 64) as u64);
    (m as u32) ^ ((m >> 32) as u32)
}
