// MINIMAL DEFAULT-LEVEL REPRODUCER of the F6 lowering site without a
// zero-trip-capable loop (campaign 28, W2): FIVE u64 values defined before a
// bottom-tested `(input1 % 7) + 2` loop, consumed inside it in ONE wide
// expression and used again after it.  Same erased-split-reload mechanism as
// `case_window_erased_min.rs`, but the over-window stack is met by the
// operand scheduler instead of the emitter.  See tests/pressure.rs.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n.rotate_left(2);
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(3);
    let v2 = m.wrapping_mul(0x94d0_49bb_1331_11eb) ^ n.rotate_left(4);
    let v3 = n.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ m.rotate_left(5);
    let v4 = m.wrapping_mul(0xa076_1d64_78bd_642f) ^ n.rotate_left(6);
    let mut acc = (m ^ n) | 1;
    let mut i0: u32 = 0;
    while i0 < (input1 % 7) + 2 {
        acc ^= (v0 ^ acc.rotate_left(2)).wrapping_add(v1.wrapping_add(acc)).wrapping_sub(v2 ^ acc.rotate_left(4)).wrapping_mul(v3.wrapping_add(acc)).wrapping_add(v4 ^ acc.rotate_left(6));
        i0 = i0.wrapping_add(1);
    }
    let mut out = acc;
    out ^= v0.rotate_left(3);
    out = out.wrapping_sub(v1);
    out ^= v2.rotate_left(5);
    out = out.wrapping_sub(v3);
    out ^= v4.rotate_left(7);
    (out as u32) ^ ((out >> 32) as u32)
}
