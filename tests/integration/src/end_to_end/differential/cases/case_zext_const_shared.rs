// The UNSIGNED twin of `case_sext_const_shared.rs` (campaign 28, W0b): a `u64`
// constant shared between a widening multiply -- whose `i64.mul_wide_u`
// operands the frontend bitcasts to `u64` and zero-extends to `u128` -- and a
// plain `u64` multiply of a different value.  This one PASSES: see the guard
// test in tests/wide.rs for why the unsigned folder does not alias the shared
// constant.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input2 as u64);
    let b = ((input2 as u64) << 32) | (input1 as u64);
    let hi = (((a as u128).wrapping_mul(10)) >> 64) as u64;
    let p = b.wrapping_mul(10);
    let r = hi ^ p;
    (r as u32) ^ ((r >> 32) as u32)
}
