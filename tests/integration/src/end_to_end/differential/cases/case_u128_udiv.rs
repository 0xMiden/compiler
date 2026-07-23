// Runtime u128 unsigned division via compiler-builtins `__udivti3`, compiled
// into the guest wasm as an ordinary function (deep u64 clz/shift/sub loops).
// A full-128-bit dividend is divided by a small (u64-range) divisor and by a
// full-width divisor, so the narrow-divisor delegation and the wide
// long-division path both execute on every input. Divisors are non-zero by
// construction (`| 1`).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = (input1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b: u64 = ((input2 as u64) << 32) | input1 as u64;
    let n: u128 = ((a as u128) << 64) | b as u128;

    // Small divisor (fits in u64): the two-word-by-one-word path.
    let q1 = n / ((b | 1) as u128);
    // Full-width divisor: the wide compare/subtract path.
    let d2: u128 = (((b as u128) << 64) | a as u128) | 1;
    let q2 = n / d2;

    let m = (q1 as u64) ^ ((q1 >> 64) as u64) ^ (q2 as u64) ^ ((q2 >> 64) as u64);
    (m as u32) ^ ((m >> 32) as u32)
}
