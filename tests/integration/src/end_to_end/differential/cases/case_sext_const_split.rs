// Passing sibling of `case_sext_const_shared.rs` (campaign 28, W0a): the plain
// multiply uses 11, so the sign-extended constant 10 of the widening multiply
// is no longer shared with a two-felt `i64` use and nothing is retyped under a
// live user.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | (input2 as u64)) as i64;
    let b = (((input2 as u64) << 32) | (input1 as u64)) as i64;
    let hi = (((a as i128).wrapping_mul(10)) >> 64) as i64;
    let p = b.wrapping_mul(11);
    let r = hi ^ p;
    (r as u32) ^ ((r >> 32) as u32)
}
