// SHA-256 compression (campaign 17, program 1): the real 64-word message
// schedule and 64 rounds with the standard K constants (a .rodata table),
// run twice — first over the padded single-block message `input1 ||
// input2` (big-endian words, 0x80 pad, 64-bit length), then over a block
// rebuilt from the first digest's bytes at an input-derived rotation and
// xored with input bytes — with the eight working variables live through
// the round loop and the schedule in a stack array. Both digests are folded
// with a leading-zero "difficulty" flag, so a single wrong bit in any round
// changes the result.
static K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

// One SHA-256 compression of `block` into `state`.
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    let mut i = 0usize;
    while i < 16 {
        w[i] = u32::from_be_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ]);
        i += 1;
    }
    while i < 64 {
        let x = w[i - 15];
        let y = w[i - 2];
        let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
        let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        i += 1;
    }
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];
    let mut r = 0usize;
    while r < 64 {
        let bs1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h.wrapping_add(bs1).wrapping_add(ch).wrapping_add(K[r]).wrapping_add(w[r]);
        let bs0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = bs0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
        r += 1;
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Block 1: the padded eight-byte message `input1 || input2`.
    let mut block = [0u8; 64];
    block[0..4].copy_from_slice(&input1.to_be_bytes());
    block[4..8].copy_from_slice(&input2.to_be_bytes());
    block[8] = 0x80;
    block[63] = 64;
    let mut st = H0;
    compress(&mut st, &block);
    let d1 = st;
    // Block 2: the first digest's bytes at an input-derived rotation, each
    // xored with an input byte selected by a stride.
    let rot = input2 % 61;
    let mut i = 0u32;
    while i < 64 {
        let word = d1[((i / 4) & 7) as usize];
        let b = (word >> (8 * (i & 3))) as u8;
        let x = (input1 >> ((i * 5) & 31)) as u8;
        block[((i + rot) % 64) as usize] = b ^ x;
        i += 1;
    }
    compress(&mut st, &block);
    // Fold both digests and a difficulty flag (leading zero nibble) into
    // the result.
    let mut acc = 0u32;
    let mut k = 0u32;
    while k < 8 {
        acc = acc.rotate_left(5) ^ st[k as usize] ^ d1[k as usize].rotate_left(k);
        k += 1;
    }
    let flag = (d1[0].leading_zeros() >= 4) as u32 | (((st[0] & 0xf) == 0) as u32) << 1;
    acc ^ (flag << 30)
}
