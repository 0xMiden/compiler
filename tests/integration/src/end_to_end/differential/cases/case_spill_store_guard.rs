// Passing sibling of `case_spill_store_min.rs` (campaign 28, W3): the same
// kernel with ONE rotation row (two constants) fewer, which compiles and
// matches native at `--optimize=size-min`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut k = [0u64; 5];
    k[0] = ((input1 as u64) << 32) | (input2 as u64) | 1;
    k[1] = ((input2 as u64) << 32) | (input1 as u64);
    k[2] = k[0] ^ 0x1bd1_1bda_a9fc_1a22;
    k[3] = k[1].wrapping_mul(0x9e37_79b9_7f4a_7c15);
    k[4] = k[0] ^ k[1] ^ k[2] ^ k[3];
    let mut tw = [0u64; 3];
    tw[0] = (input1 as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    tw[1] = (input2 as u64).wrapping_mul(0x1656_67b1_9e37_79f9);
    tw[2] = tw[0] ^ tw[1];
    let mut x0 = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut x1 = (input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c16) | 2;
    let mut x2 = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c17) | 3;
    let mut x3 = (input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c18) | 4;
    let mut grp = 0usize;
    while grp < 2 {
        x0 = x0.wrapping_add(k[(2 * grp) % 5]);
        x1 = x1.wrapping_add(k[(2 * grp + 1) % 5]).wrapping_add(tw[(2 * grp) % 3]);
        x2 = x2.wrapping_add(k[(2 * grp + 2) % 5]).wrapping_add(tw[(2 * grp + 1) % 3]);
        x3 = x3.wrapping_add(k[(2 * grp + 3) % 5]).wrapping_add(2 * grp as u64);
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(14) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(16) ^ x2;
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(52) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(57) ^ x2;
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(23) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(40) ^ x2;
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(5) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(37) ^ x2;
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(25) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(33) ^ x2;
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(46) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(12) ^ x2;
        x0 = x0.wrapping_add(x1);
        x1 = x1.rotate_left(58) ^ x0;
        x2 = x2.wrapping_add(x3);
        x3 = x3.rotate_left(22) ^ x2;
        grp += 1;
    }
    let mut out = x0 ^ x1;
    out ^= x2 ^ x3;
    (out as u32) ^ ((out >> 32) as u32)
}
