// Ten u64 values (20 felts) held live ACROSS a loop: every iteration rotates
// each of them by a loop-variant amount (so LICM cannot collapse them into
// fewer live values), and three of them are consumed again after the loop.
// W^entry at the loop header exceeds the 16-felt operand stack, forcing the
// spill analysis down its loop-header paths (compute_w_entry_loop /
// spill_trailing_until_fits / max_loop_pressure) and requiring spills and
// reloads along the critical backedge and exit edges (edge splits).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    // Ten u64s, all loop-live below.
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n;
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ n.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let v8 = v6.wrapping_add(v4.rotate_left(19)) ^ n.rotate_left(3);
    let v9 = v7.wrapping_sub(v5.rotate_left(21)) ^ m.rotate_left(5);
    let iters = (input2 % 97) + 3;
    let mut acc = (m ^ n) | 1;
    let mut i: u32 = 0;
    while i < iters {
        // Rotate counts depend on `i`, so each `vK` itself must stay live in
        // the loop body rather than a hoisted loop-invariant derivative.
        let r = i & 63;
        acc = acc.wrapping_mul(v0.rotate_left(r) | 1);
        acc ^= v1.rotate_left(r.wrapping_add(1));
        acc = acc.wrapping_add(v2.rotate_left(r.wrapping_add(2)));
        acc ^= v3.rotate_left(r.wrapping_add(3));
        acc = acc.wrapping_sub(v4.rotate_left(r.wrapping_add(4)));
        acc ^= v5.rotate_left(r.wrapping_add(5));
        acc = acc.wrapping_add(v6.rotate_left(r.wrapping_add(6)));
        acc ^= v7.rotate_left(r.wrapping_add(7));
        acc = acc.wrapping_sub(v8.rotate_left(r.wrapping_add(8)));
        acc ^= v9.rotate_left(r.wrapping_add(9));
        i = i.wrapping_add(1);
    }
    // Loop-exit-edge uses: three of the spilled values reloaded after the loop.
    let r = acc ^ v0.rotate_left(25) ^ v4.rotate_left(33) ^ v9.rotate_left(41);
    (r as u32) ^ ((r >> 32) as u32)
}
