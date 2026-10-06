// Trap edge in the SECOND of two sequential loops that share one cluster —
// the campaign-20 shape whose freight-only version reaches the
// `frontier.rs:123` unwrap through a three-predecessor join.
//
// The body is `interact::cascade_spill`: two 8-trip loops with an empty
// `continue` arm, eight rotate count bands used before the first loop, on each
// loop's accumulator and after the second, and an eight-u64 cluster consumed
// in one wide expression in BOTH bodies and again afterwards. Only the second
// loop carries a guard, at the end of its body, so the trapping edge sits in
// the join's neighbourhood that the spill transform's split edges feed.
static A31: [u8; 31] = [
    0x03, 0x11, 0x37, 0x59, 0x83, 0xb1, 0xd3, 0x01, 0x2b, 0x57, 0x85, 0xb3, 0xe1, 0x0f, 0x3d, 0x6b,
    0x99, 0xc7, 0xf5, 0x23, 0x51, 0x7f, 0xad, 0xdb, 0x09, 0x37, 0x65, 0x93, 0xc1, 0xef, 0x1d,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n.rotate_left(2);
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(3);
    let v2 = m.wrapping_mul(0x94d0_49bb_1331_11eb) ^ n.rotate_left(4);
    let v3 = n.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ m.rotate_left(5);
    let v4 = m.wrapping_mul(0xa076_1d64_78bd_642f) ^ n.rotate_left(6);
    let v5 = n.wrapping_mul(0xe703_7ed1_a0b4_28db) ^ m.rotate_left(7);
    let v6 = m.wrapping_mul(0x8ebc_6af0_9c88_c6e3) ^ n.rotate_left(8);
    let v7 = n.wrapping_mul(0x5895_58cb_3521_e49d) ^ m.rotate_left(9);
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    acc ^= n.rotate_left(7);
    acc = acc.wrapping_add(m.rotate_left(9));
    acc = acc.wrapping_sub(n.rotate_left(11));
    acc ^= m.rotate_left(13);
    acc = acc.wrapping_add(n.rotate_left(15));
    let mut s0 = acc;
    let mut t0: u32 = 0;
    while t0 < 8 {
        t0 += 1;
        if (input2 >> t0) & 1 == 0 {
            continue;
        }
        s0 ^= s0.rotate_left(1) | 1;
        s0 = s0.wrapping_add(s0.rotate_left(3));
        s0 = s0.wrapping_sub(s0.rotate_left(5));
        s0 ^= s0.rotate_left(7) | 1;
        s0 = s0.wrapping_add(s0.rotate_left(9));
        s0 = s0.wrapping_sub(s0.rotate_left(11));
        s0 ^= s0.rotate_left(13) | 1;
        s0 = s0.wrapping_add(s0.rotate_left(15));
        s0 ^= (v0 ^ s0.rotate_left(2))
            .wrapping_add(v1.wrapping_add(s0))
            .wrapping_sub(v2 ^ s0.rotate_left(4))
            .wrapping_mul(v3.wrapping_add(s0))
            .wrapping_add(v4 ^ s0.rotate_left(6))
            .wrapping_sub(v5.wrapping_add(s0))
            .wrapping_mul(v6 ^ s0.rotate_left(8))
            .wrapping_add(v7.wrapping_add(s0));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s0;
    let mut s1 = acc;
    let mut t1: u32 = 0;
    while t1 < 8 {
        t1 += 1;
        if (input2 >> (t1 + 1)) & 1 == 0 {
            continue;
        }
        s1 ^= s1.rotate_left(1) | 1;
        s1 = s1.wrapping_add(s1.rotate_left(3));
        s1 = s1.wrapping_sub(s1.rotate_left(5));
        s1 ^= s1.rotate_left(7) | 1;
        s1 = s1.wrapping_add(s1.rotate_left(9));
        s1 = s1.wrapping_sub(s1.rotate_left(11));
        s1 ^= s1.rotate_left(13) | 1;
        s1 = s1.wrapping_add(s1.rotate_left(15));
        s1 ^= (v0 ^ s1.rotate_left(2))
            .wrapping_add(v1.wrapping_add(s1))
            .wrapping_sub(v2 ^ s1.rotate_left(4))
            .wrapping_mul(v3.wrapping_add(s1))
            .wrapping_add(v4 ^ s1.rotate_left(6))
            .wrapping_sub(v5.wrapping_add(s1))
            .wrapping_mul(v6 ^ s1.rotate_left(8))
            .wrapping_add(v7.wrapping_add(s1));
        let g = ((s1 >> 57) & 31) as usize;
        s1 ^= A31[g] as u64;
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s1;
    let mut out = acc ^ 0x5a5a_5a5a_5a5a_5a5a;
    out ^= acc.rotate_left(1);
    out = out.wrapping_add(out.rotate_left(3));
    out ^= acc.rotate_left(5);
    out = out.wrapping_add(out.rotate_left(7));
    out ^= acc.rotate_left(9);
    out = out.wrapping_add(out.rotate_left(11));
    out ^= acc.rotate_left(13);
    out = out.wrapping_add(out.rotate_left(15));
    out ^= v0.rotate_left(3);
    out = out.wrapping_sub(v1);
    out ^= v2.rotate_left(5);
    out = out.wrapping_sub(v3);
    out ^= v4.rotate_left(7);
    out = out.wrapping_sub(v5);
    out ^= v6.rotate_left(9);
    out = out.wrapping_sub(v7);
    (out as u32) ^ ((out >> 32) as u32)
}
