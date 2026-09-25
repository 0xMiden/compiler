// BLAKE2b compression: the 16-word working vector, the ten-row SIGMA message
// schedule in .rodata and twelve rounds of eight G functions over one or two
// input-derived blocks, with the counter and the last-block flag.  The G
// function carries only FOUR distinct rotation constants (32, 24, 16, 63) --
// the low-constant calibration point of the campaign -- but they are used in
// eight places per round, so the CSE-merged bands cross the round loop, the
// block loop and the finalization.
const IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

const SIGMA: [[u8; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

// The eight G calls of a round, as (a, b, c, d) index quadruples.
const GIDX: [[u8; 4]; 8] = [
    [0, 4, 8, 12],
    [1, 5, 9, 13],
    [2, 6, 10, 14],
    [3, 7, 11, 15],
    [0, 5, 10, 15],
    [1, 6, 11, 12],
    [2, 7, 8, 13],
    [3, 4, 9, 14],
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut msg = [0u64; 32];
    let mut x = (input1 as u64) | 1;
    let mut i = 0usize;
    while i < 32 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        msg[i] = x ^ ((input2 as u64) << 16);
        i += 1;
    }

    let mut h = IV;
    h[0] ^= 0x0101_0000 ^ 64;
    let blocks = 1 + (input2 % 2) as usize;
    let mut blk = 0usize;
    while blk < blocks {
        let mut v = [0u64; 16];
        let mut j = 0usize;
        while j < 8 {
            v[j] = h[j];
            v[j + 8] = IV[j];
            j += 1;
        }
        v[12] ^= (128 * (blk as u64 + 1)) ^ (input1 as u64);
        if blk + 1 == blocks {
            v[14] = !v[14];
        }

        let mut r = 0usize;
        while r < 12 {
            let mut g = 0usize;
            while g < 8 {
                let ia = GIDX[g][0] as usize;
                let ib = GIDX[g][1] as usize;
                let ic = GIDX[g][2] as usize;
                let id = GIDX[g][3] as usize;
                let mx = msg[16 * blk + SIGMA[r][2 * g] as usize];
                let my = msg[16 * blk + SIGMA[r][2 * g + 1] as usize];

                let mut va = v[ia];
                let mut vb = v[ib];
                let mut vc = v[ic];
                let mut vd = v[id];

                va = va.wrapping_add(vb).wrapping_add(mx);
                vd = (vd ^ va).rotate_right(32);
                vc = vc.wrapping_add(vd);
                vb = (vb ^ vc).rotate_right(24);
                va = va.wrapping_add(vb).wrapping_add(my);
                vd = (vd ^ va).rotate_right(16);
                vc = vc.wrapping_add(vd);
                vb = (vb ^ vc).rotate_right(63);

                v[ia] = va;
                v[ib] = vb;
                v[ic] = vc;
                v[id] = vd;
                g += 1;
            }
            r += 1;
        }

        let mut k = 0usize;
        while k < 8 {
            h[k] ^= v[k] ^ v[k + 8];
            k += 1;
        }
        blk += 1;
    }

    // Finalization fold with the same rotation constants.
    let mut out = 0u64;
    let mut j = 0usize;
    while j < 8 {
        let d = h[j];
        out = (out ^ d).rotate_right(32);
        out = out.wrapping_add(d.rotate_right(24) ^ d.rotate_right(16));
        out ^= d.rotate_right(63);
        j += 1;
    }
    (out as u32) ^ ((out >> 32) as u32)
}
