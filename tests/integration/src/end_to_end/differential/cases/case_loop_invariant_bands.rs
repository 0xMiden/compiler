// Trap edge in the BODY of a freight loop with FOLD-PROOF rotate counts;
// guard kind: `assert!`.
//
// The twin of `case_ts_body_assert.rs` with the constant rotate amounts
// replaced by twelve loop-invariant ones derived from the inputs at runtime
// (`(input1 >> k) & 63` and `(input2 >> k) & 63`), so no constant folding can
// ever remove them. They are used before the loop, inside it (on the
// loop-carried accumulator and in the wide expression over the eight-u64
// cluster) and after it; the trap edge is an `assert!` on five bits of the
// accumulator. After the lift the loop forwards nine loop-carried values plus
// nine loop-defined live-outs, 18 felts, more than the operand stack window,
// and the `narrow-region-ops` pass must move the excess to procedure locals
// (#1459). In the constant-amount twin the masks were only kept apart by
// debug info.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let r0 = (input1 >> 1) & 63;
    let r1 = (input1 >> 3) & 63;
    let r2 = (input1 >> 5) & 63;
    let r3 = (input1 >> 7) & 63;
    let r4 = (input1 >> 9) & 63;
    let r5 = (input1 >> 11) & 63;
    let r6 = (input2 >> 1) & 63;
    let r7 = (input2 >> 3) & 63;
    let r8 = (input2 >> 5) & 63;
    let r9 = (input2 >> 7) & 63;
    let r10 = (input2 >> 9) & 63;
    let r11 = (input2 >> 11) & 63;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n.rotate_left(r0);
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(r1);
    let v2 = m.wrapping_mul(0x94d0_49bb_1331_11eb) ^ n.rotate_left(r2);
    let v3 = n.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ m.rotate_left(r3);
    let v4 = m.wrapping_mul(0xa076_1d64_78bd_642f) ^ n.rotate_left(r4);
    let v5 = n.wrapping_mul(0xe703_7ed1_a0b4_28db) ^ m.rotate_left(r5);
    let v6 = m.wrapping_mul(0x8ebc_6af0_9c88_c6e3) ^ n.rotate_left(r6);
    let v7 = n.wrapping_mul(0x5895_58cb_3521_e49d) ^ m.rotate_left(r7);
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(r8);
    acc = acc.wrapping_add(n.rotate_left(r9));
    acc = acc.wrapping_sub(m.rotate_left(r10));
    acc ^= n.rotate_left(r11);
    let mut s = acc;
    let mut t: u32 = 0;
    while t < 8 {
        t += 1;
        if (input2 >> t) & 1 == 0 {
            continue;
        }
        s ^= s.rotate_left(r0) | 1;
        s = s.wrapping_add(s.rotate_left(r1));
        s = s.wrapping_sub(s.rotate_left(r2));
        s ^= s.rotate_left(r3) | 1;
        s = s.wrapping_add(s.rotate_left(r4));
        s = s.wrapping_sub(s.rotate_left(r5));
        s ^= s.rotate_left(r6) | 1;
        s = s.wrapping_add(s.rotate_left(r7));
        s ^= (v0 ^ s.rotate_left(r8))
            .wrapping_add(v1.wrapping_add(s))
            .wrapping_sub(v2 ^ s.rotate_left(r9))
            .wrapping_mul(v3.wrapping_add(s))
            .wrapping_add(v4 ^ s.rotate_left(r10))
            .wrapping_sub(v5.wrapping_add(s))
            .wrapping_mul(v6 ^ s.rotate_left(r11))
            .wrapping_add(v7.wrapping_add(s));
        let d = (s >> 57) & 31;
        assert!(d != 31);
        s ^= d ^ 0x37;
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s;
    let mut out = acc ^ 0x5a5a_5a5a_5a5a_5a5a;
    out ^= acc.rotate_left(r0);
    out = out.wrapping_add(out.rotate_left(r1));
    out ^= acc.rotate_left(r2);
    out = out.wrapping_add(out.rotate_left(r3));
    out ^= acc.rotate_left(r4);
    out = out.wrapping_add(out.rotate_left(r5));
    out ^= acc.rotate_left(r6);
    out = out.wrapping_add(out.rotate_left(r7));
    out ^= v0.rotate_left(r8);
    out = out.wrapping_sub(v1);
    out ^= v2.rotate_left(r9);
    out = out.wrapping_sub(v3);
    out ^= v4.rotate_left(r10);
    out = out.wrapping_sub(v5);
    out ^= v6.rotate_left(r11);
    out = out.wrapping_sub(v7);
    (out as u32) ^ ((out >> 32) as u32)
}
