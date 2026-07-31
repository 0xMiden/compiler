// Widest reachable flat call signatures: 16 u32 params and 8 u64 params are
// both exactly 16 stack felts — the call-site scheduling limit (20 felts is a
// verified compile-time spills panic). Several u64 values stay live across
// both call sites, forcing caller-side scheduling/spilling around wide calls.

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn wide16_u32(
    a0: u32,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: u32,
    a6: u32,
    a7: u32,
    a8: u32,
    a9: u32,
    b0: u32,
    b1: u32,
    b2: u32,
    b3: u32,
    b4: u32,
    b5: u32,
) -> u32 {
    a0.wrapping_add(a1.rotate_left(1))
        .wrapping_add(a2 ^ a3)
        .wrapping_add(a4.wrapping_mul(3))
        .wrapping_add(a5 ^ a6)
        .wrapping_add(a7.rotate_left(2))
        .wrapping_add(a8 ^ a9)
        .wrapping_add(b0.wrapping_mul(5))
        .wrapping_add(b1 ^ b2)
        .wrapping_add(b3.rotate_left(3))
        .wrapping_add(b4 ^ b5)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn wide8_u64(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64, a7: u64) -> u64 {
    a0.wrapping_add(a1.rotate_left(1))
        .wrapping_add(a2 ^ a3)
        .wrapping_add(a4.wrapping_mul(3))
        .wrapping_add(a5 ^ a6)
        .wrapping_add(a7.rotate_left(2))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // u64 values live across BOTH wide calls (and used after them).
    let k1 = ((input1 as u64) << 32) | input2 as u64;
    let k2 = k1.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    let w = wide16_u32(
        input1,
        input2,
        input1 ^ 1,
        input2 ^ 2,
        input1.wrapping_add(3),
        input2 ^ 4,
        input1 ^ 5,
        input2.wrapping_add(6),
        input1 ^ 7,
        input2 ^ 8,
        input1.rotate_left(9),
        input2 ^ 10,
        input1 ^ 11,
        input2.rotate_left(12),
        input1 ^ 13,
        input2 ^ 14,
    );
    let wu = wide8_u64(
        k1,
        k2,
        k1 ^ 2,
        k2.wrapping_add(3),
        k1.wrapping_add(4),
        k2 ^ 5,
        k1.rotate_left(6),
        k2 ^ 7,
    );
    // k1/k2 still live here — spilled/reloaded around the calls.
    let mix = k1 ^ k2.rotate_right(9) ^ wu;
    w ^ (mix as u32) ^ ((mix >> 32) as u32)
}
