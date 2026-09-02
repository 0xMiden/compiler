// Straight-line spill boundary: a right-leaning non-reassociable tree over
// sixteen u64 leaves (32 felts if all were live at once) mixing xor,
// wrapping_sub and a leaf-derived rotate count, in one block. The single-
// block spill path (MIN with same-block reloads) handles twice the window
// without a cliff, extending the `stack_pressure` (~20 felts) guard.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (input1 | 1) as u64;
    let b = ((input2 ^ 0x9e37_79b9) as u64).rotate_left(7) | 2;
    let r = (a.wrapping_mul(0x9e3779b97f4a7c15).rotate_left(1) ^ b.wrapping_add(0x0) ^ (a.wrapping_mul(0x9e3779b97f4a7c17).rotate_left(8) ^ b.wrapping_add(0x123456789)).wrapping_sub((a.wrapping_mul(0x9e3779b97f4a7c19).rotate_left(15) ^ b.wrapping_add(0x2468acf12)).rotate_left(((a.wrapping_mul(0x9e3779b97f4a7c1b).rotate_left(22) ^ b.wrapping_add(0x369d0369b) ^ (a.wrapping_mul(0x9e3779b97f4a7c1d).rotate_left(29) ^ b.wrapping_add(0x48d159e24)).wrapping_sub((a.wrapping_mul(0x9e3779b97f4a7c1f).rotate_left(36) ^ b.wrapping_add(0x5b05b05ad)).rotate_left(((a.wrapping_mul(0x9e3779b97f4a7c21).rotate_left(43) ^ b.wrapping_add(0x6d3a06d36) ^ (a.wrapping_mul(0x9e3779b97f4a7c23).rotate_left(50) ^ b.wrapping_add(0x7f6e5d4bf)).wrapping_sub((a.wrapping_mul(0x9e3779b97f4a7c25).rotate_left(57) ^ b.wrapping_add(0x91a2b3c48)).rotate_left(((a.wrapping_mul(0x9e3779b97f4a7c27).rotate_left(1) ^ b.wrapping_add(0xa3d70a3d1) ^ (a.wrapping_mul(0x9e3779b97f4a7c29).rotate_left(8) ^ b.wrapping_add(0xb60b60b5a)).wrapping_sub((a.wrapping_mul(0x9e3779b97f4a7c2b).rotate_left(15) ^ b.wrapping_add(0xc83fb72e3)).rotate_left(((a.wrapping_mul(0x9e3779b97f4a7c2d).rotate_left(22) ^ b.wrapping_add(0xda740da6c) ^ (a.wrapping_mul(0x9e3779b97f4a7c2f).rotate_left(29) ^ b.wrapping_add(0xeca8641f5)).wrapping_sub((a.wrapping_mul(0x9e3779b97f4a7c31).rotate_left(36) ^ b.wrapping_add(0xfedcba97e)).rotate_left(((a.wrapping_mul(0x9e3779b97f4a7c33).rotate_left(43) ^ b.wrapping_add(0x1111111107))) as u32 & 31)))) as u32 & 31)))) as u32 & 31)))) as u32 & 31)))) as u32 & 31)));
    (r as u32) ^ ((r >> 32) as u32)
}
