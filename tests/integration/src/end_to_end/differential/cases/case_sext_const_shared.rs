// MINIMAL, LOOP-FREE REPRODUCER of F18 (campaign 28, W0a): an `i64` constant
// shared between a widening multiply -- whose `i64.mul_wide_s` operands the
// frontend sign-extends to `i128` -- and a plain `i64` multiply of a different
// value.  Only the HIGH word of the wide product is consumed, which is the
// guest-toolchain-correct shape (wasmtime agrees with native), so any
// divergence is midenc's.  See the ignored test in tests/wide.rs.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | (input2 as u64)) as i64;
    let b = (((input2 as u64) << 32) | (input1 as u64)) as i64;
    let hi = (((a as i128).wrapping_mul(10)) >> 64) as i64;
    let p = b.wrapping_mul(10);
    let r = hi ^ p;
    (r as u32) ^ ((r >> 32) as u32)
}
