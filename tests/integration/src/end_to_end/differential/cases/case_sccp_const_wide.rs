// The 64-bit half of the same question. Every shift and rotate count here is a
// literal, and the frontend truncates each 64-bit count to u32 with its own
// materialized constant, so the wide expansions are where a constant-operand
// coercion (`zext` / `trunc` / `bitcast` of a constant) can survive into HIR
// with both operands known. Value-checked because a mis-typed uniqued constant
// is a wrong-width push, not an IR-visible difference.

use core::hint::black_box;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let w = ((black_box(input1) as u64) << 32) | black_box(input2) as u64 | 1;

    let mut r = 0u64;
    r ^= w.rotate_left(13);
    r ^= w.rotate_right(29);
    r ^= w << 17;
    r ^= w >> 3;
    r ^= ((w as i64) >> 7) as u64;
    r ^= w / 3;
    r ^= w % 9;
    r ^= (w as u32 as u64).wrapping_mul(0x9e37_79b9);
    r ^= u64::from(w > 0x0000_0001_0000_0000);
    r ^= (w & 0x0000_00ff_0000_0000) >> 24;

    (r as u32) ^ ((r >> 32) as u32)
}
