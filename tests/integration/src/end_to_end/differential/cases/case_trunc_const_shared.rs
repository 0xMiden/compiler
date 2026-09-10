// The TRUNCATING twin of `case_sext_const_shared.rs` (campaign 28, W0c): a
// 64-bit rotate COUNT constant -- which the frontend truncates to `u32` in
// `mask_movement_count` -- shared with a plain `i64` use of the same constant.
// This one PASSES: see the guard test in tests/wide.rs.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input2 as u64) | 1;
    let b = ((input2 as u64) << 32) | (input1 as u64) | 3;
    let r = a.rotate_left(24) ^ b.wrapping_add(24);
    (r as u32) ^ ((r >> 32) as u32)
}
