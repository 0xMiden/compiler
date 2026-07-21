// Each branch arm first CALLS a non-inlinable helper (its result cannot be
// sunk or rescheduled next to the terminator like pure ops can), then builds
// a wide expression tree over ~10 u64 locals whose pressure spills the call
// result, and finally yields that call result as the arm's value. The
// spilled-then-unreloaded value flows out of the arm as a successor
// argument / scf.yield operand, exercising control-flow-edge reload
// reconciliation (edge splits) and the region-terminator reload path of the
// MIN algorithm.
#[inline(never)]
fn scramble(a: u64) -> u64 {
    a.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ a.rotate_left(13)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
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
    let mut z = m ^ n.rotate_left(7);
    let t = if m % 97 < 48 {
        // Call result lives across the whole tree below, with no other use in
        // this arm: highest next-use distance, first spill candidate.
        let r = scramble(z ^ v0);
        z = (v1 ^ v9.rotate_left(1))
            .wrapping_add(v2 ^ v8.rotate_left(3))
            .wrapping_mul(v3 | 1)
            ^ v7.rotate_left(5)
            ^ (v4 ^ v6.rotate_left(7)).wrapping_sub(v5 ^ z.rotate_left(9));
        r
    } else {
        let r = scramble(z.wrapping_add(v9));
        z = (v8 ^ v0.rotate_left(2))
            .wrapping_sub(v7 ^ v1.rotate_left(4))
            .wrapping_mul(v6 | 1)
            ^ v2.rotate_left(6)
            ^ (v5 ^ v3.rotate_left(8)).wrapping_add(v4 ^ z.rotate_left(10));
        r
    };
    let r = t ^ z.rotate_left(21) ^ v0.rotate_left(31) ^ v9.rotate_left(35);
    (r as u32) ^ ((r >> 32) as u32)
}
