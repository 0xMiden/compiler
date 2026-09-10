// SHA-512 compression over one or two input-derived blocks: the real 80-entry
// round-constant table in .rodata, an 80-word message schedule and the eight
// working variables carried through the round loop.  The four mixing
// functions carry TWELVE distinct rotate/shift constants -- Sigma0 (28, 34,
// 39), Sigma1 (14, 18, 41), sigma0 (1, 8, >> 7) and sigma1 (19, 61, >> 6) --
// and they cross the schedule loop, the round loop and the block loop, since
// the compression is written inline in the block loop the way a single-purpose
// no_std kernel is.
const K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

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

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Message words derived from the inputs (two blocks of sixteen).
    let mut msg = [0u64; 32];
    let mut x = (input2 as u64) | 1;
    let mut i = 0usize;
    while i < 32 {
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 8;
        msg[i] = x ^ ((input1 as u64) << 32);
        i += 1;
    }
    // Message-length byte in the last word of the final block.
    msg[31] = (input1 % 1024) as u64;

    let mut h = IV;
    let blocks = 1 + (input2 % 2) as usize;
    let mut blk = 0usize;
    while blk < blocks {
        let mut w = [0u64; 80];
        let mut t = 0usize;
        while t < 16 {
            w[t] = msg[16 * blk + t];
            t += 1;
        }
        while t < 80 {
            let w15 = w[t - 15];
            let w2 = w[t - 2];
            let s0 = w15.rotate_right(1) ^ w15.rotate_right(8) ^ (w15 >> 7);
            let s1 = w2.rotate_right(19) ^ w2.rotate_right(61) ^ (w2 >> 6);
            w[t] = w[t - 16]
                .wrapping_add(s0)
                .wrapping_add(w[t - 7])
                .wrapping_add(s1);
            t += 1;
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        let mut r = 0usize;
        while r < 80 {
            let big1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(big1)
                .wrapping_add(ch)
                .wrapping_add(K[r])
                .wrapping_add(w[r]);
            let big0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = big0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
            r += 1;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
        blk += 1;
    }

    // Fold the digest with the same mixing constants the compression uses.
    let mut out = 0u64;
    let mut j = 0usize;
    while j < 8 {
        let v = h[j];
        out ^= v.rotate_right(14) ^ v.rotate_right(28) ^ (v >> 7);
        out = out.wrapping_add(v.rotate_right(41) ^ (v >> 6));
        j += 1;
    }
    (out as u32) ^ ((out >> 32) as u32)
}
