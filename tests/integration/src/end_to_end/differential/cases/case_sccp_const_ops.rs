// Results, not block arguments: every operator here reaches the wasm with at
// least one `i32.const` operand, and several of them are expanded by the wasm
// FRONTEND into op chains whose own extra operands are constants too (the
// rotate's `32 - count`, the unsigned compare's bitcasts, the `rem_s`
// expansion, the division's zero check). Those late-created constant-operand
// ops are the only candidates for SCCP to fold a result the canonicalizer's
// folder did not already reach.

use core::hint::black_box;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = black_box(input1) | 1;
    let s = black_box(input2);

    let mut r = 0u32;
    r ^= x.rotate_left(7);
    r ^= x.rotate_right(13);
    r ^= x / 3;
    r ^= x % 7;
    r ^= (x as i32 / 5) as u32;
    r ^= (x as i32 % 11) as u32;
    r ^= ((x as i32) >> 5) as u32;
    r ^= x >> 9;
    r ^= x << 3;
    r ^= (x & 0x00ff_0000) >> 16;
    r ^= u32::from(x > 0x1000);
    r ^= u32::from((x as i32) < -4096);
    r = r.wrapping_add(s & 0x3f);
    r
}
