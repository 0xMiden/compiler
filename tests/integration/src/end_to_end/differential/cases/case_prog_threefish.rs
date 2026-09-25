// Threefish-256 (the Skein block cipher) reduced to sixteen of its
// seventy-two rounds: four u64 words, the MIX/permute round structure and a
// key injection every four rounds with the tweak schedule.  Threefish-256's
// rotation table has FIFTEEN distinct constants (14, 16, 52, 57, 23, 40, 5,
// 37, 25, 33, 46, 12, 58, 22, 32), which is how the cipher is specified; the
// eight rotation rows are unrolled inside a two-trip round-group loop the way
// reference implementations write them, so fifteen count bands cross the
// round-group loop, the block loop and the output whitening.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Key and tweak from the inputs; the extended words follow the spec.
    let mut k = [0u64; 5];
    k[0] = ((input1 as u64) << 32) | (input2 as u64) | 1;
    k[1] = ((input2 as u64) << 32) | (input1 as u64);
    k[2] = k[0] ^ 0x1bd1_1bda_a9fc_1a22;
    k[3] = k[1].wrapping_mul(0x9e37_79b9_7f4a_7c15);
    k[4] = k[0] ^ k[1] ^ k[2] ^ k[3] ^ 0x1bd1_1bda_a9fc_1a22;

    let mut tw = [0u64; 3];
    tw[0] = (input1 as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    tw[1] = (input2 as u64).wrapping_mul(0x1656_67b1_9e37_79f9);
    tw[2] = tw[0] ^ tw[1];

    let blocks = 1 + (input2 % 3) as usize;
    let mut chain = [0u64; 4];
    let mut blk = 0usize;
    while blk < blocks {
        let mut x0 = chain[0] ^ (input1 as u64).wrapping_add(4 * blk as u64);
        let mut x1 = chain[1] ^ (input2 as u64).wrapping_add(4 * blk as u64 + 1);
        let mut x2 = chain[2] ^ (input1 as u64).wrapping_add(4 * blk as u64 + 2);
        let mut x3 = chain[3] ^ (input2 as u64).wrapping_add(4 * blk as u64 + 3);

        let mut grp = 0usize;
        while grp < 2 {
            // Key injection before the first four rounds of the group.
            x0 = x0.wrapping_add(k[(2 * grp) % 5]);
            x1 = x1
                .wrapping_add(k[(2 * grp + 1) % 5])
                .wrapping_add(tw[(2 * grp) % 3]);
            x2 = x2
                .wrapping_add(k[(2 * grp + 2) % 5])
                .wrapping_add(tw[(2 * grp + 1) % 3]);
            x3 = x3
                .wrapping_add(k[(2 * grp + 3) % 5])
                .wrapping_add(2 * grp as u64);

            // Rounds 0-3 of the group: rotation rows (14,16) (52,57) (23,40)
            // (5,37), each followed by the (0,3,2,1) word permutation.
            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(14) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(16) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(52) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(57) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(23) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(40) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(5) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(37) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            // Key injection before the second four rounds.
            x0 = x0.wrapping_add(k[(2 * grp + 1) % 5]);
            x1 = x1
                .wrapping_add(k[(2 * grp + 2) % 5])
                .wrapping_add(tw[(2 * grp + 1) % 3]);
            x2 = x2
                .wrapping_add(k[(2 * grp + 3) % 5])
                .wrapping_add(tw[(2 * grp + 2) % 3]);
            x3 = x3
                .wrapping_add(k[(2 * grp + 4) % 5])
                .wrapping_add(2 * grp as u64 + 1);

            // Rounds 4-7: rotation rows (25,33) (46,12) (58,22) (32,32).
            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(25) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(33) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(46) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(12) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(58) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(22) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            x0 = x0.wrapping_add(x1);
            x1 = x1.rotate_left(32) ^ x0;
            x2 = x2.wrapping_add(x3);
            x3 = x3.rotate_left(32) ^ x2;
            core::mem::swap(&mut x1, &mut x3);

            grp += 1;
        }

        // Output whitening with the same rotation constants.
        chain[0] = x0 ^ x1.rotate_left(14) ^ x2.rotate_left(52);
        chain[1] = x1 ^ x2.rotate_left(23) ^ x3.rotate_left(5);
        chain[2] = x2 ^ x3.rotate_left(25) ^ x0.rotate_left(46);
        chain[3] = x3 ^ x0.rotate_left(58) ^ x1.rotate_left(32);
        blk += 1;
    }

    let mut out = chain[0]
        ^ chain[1].rotate_left(16)
        ^ chain[2].rotate_left(57)
        ^ chain[3].rotate_left(40);
    out ^= chain[0].rotate_left(37) ^ chain[1].rotate_left(33);
    out ^= chain[2].rotate_left(12) ^ chain[3].rotate_left(22);
    (out as u32) ^ ((out >> 32) as u32)
}
