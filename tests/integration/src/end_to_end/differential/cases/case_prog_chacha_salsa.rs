// ChaCha20 keystream encryption followed by a Salsa20-core checksum over the
// ciphertext: SEVEN distinct rotation constants on a 32-bit state -- the
// ChaCha quarter-round's 16, 12, 8, 7 and the Salsa column/row round's 7, 9,
// 13, 18 (the 7 is shared) -- crossing the keystream-block loop, the two
// twenty-round cores and the folding loop.  The 32-bit twin of the u64
// programs in this module: same constant count, half the operand width.
const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Plaintext: 32 words derived from the inputs.
    let mut buf = [0u32; 32];
    let mut x = input1 | 1;
    let mut i = 0usize;
    while i < 32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf[i] = x ^ input2;
        i += 1;
    }

    // ChaCha20: two 16-word keystream blocks, counter in word 12.
    let mut blk = 0u32;
    while blk < 2 {
        let mut s = [0u32; 16];
        s[0] = SIGMA[0];
        s[1] = SIGMA[1];
        s[2] = SIGMA[2];
        s[3] = SIGMA[3];
        let mut k = 0usize;
        while k < 8 {
            s[4 + k] = input1.wrapping_mul(0x9e37_79b9).wrapping_add(k as u32) ^ input2;
            k += 1;
        }
        s[12] = blk;
        s[13] = input2;
        s[14] = input1 ^ input2;
        s[15] = 0x5555_5555;

        let mut v = s;
        let mut r = 0usize;
        while r < 10 {
            // column round
            let mut c = 0usize;
            while c < 4 {
                let (ia, ib, ic, id) = (c, 4 + c, 8 + c, 12 + c);
                let mut a = v[ia];
                let mut b = v[ib];
                let mut cc = v[ic];
                let mut d = v[id];
                a = a.wrapping_add(b);
                d = (d ^ a).rotate_left(16);
                cc = cc.wrapping_add(d);
                b = (b ^ cc).rotate_left(12);
                a = a.wrapping_add(b);
                d = (d ^ a).rotate_left(8);
                cc = cc.wrapping_add(d);
                b = (b ^ cc).rotate_left(7);
                v[ia] = a;
                v[ib] = b;
                v[ic] = cc;
                v[id] = d;
                c += 1;
            }
            // diagonal round
            let mut g = 0usize;
            while g < 4 {
                let ia = g;
                let ib = 4 + ((g + 1) % 4);
                let ic = 8 + ((g + 2) % 4);
                let id = 12 + ((g + 3) % 4);
                let mut a = v[ia];
                let mut b = v[ib];
                let mut cc = v[ic];
                let mut d = v[id];
                a = a.wrapping_add(b);
                d = (d ^ a).rotate_left(16);
                cc = cc.wrapping_add(d);
                b = (b ^ cc).rotate_left(12);
                a = a.wrapping_add(b);
                d = (d ^ a).rotate_left(8);
                cc = cc.wrapping_add(d);
                b = (b ^ cc).rotate_left(7);
                v[ia] = a;
                v[ib] = b;
                v[ic] = cc;
                v[id] = d;
                g += 1;
            }
            r += 1;
        }

        let mut w = 0usize;
        while w < 16 {
            buf[16 * blk as usize + w] ^= v[w].wrapping_add(s[w]);
            w += 1;
        }
        blk += 1;
    }

    // Salsa20 core as the checksum's mixing function over the ciphertext.
    let mut z = [0u32; 16];
    let mut j = 0usize;
    while j < 16 {
        z[j] = buf[j] ^ buf[16 + j];
        j += 1;
    }
    let mut r = 0usize;
    while r < 10 {
        // column round
        let mut c = 0usize;
        while c < 4 {
            let i0 = 5 * c % 16;
            let i1 = (i0 + 4) % 16;
            let i2 = (i0 + 8) % 16;
            let i3 = (i0 + 12) % 16;
            z[i1] ^= z[i0].wrapping_add(z[i3]).rotate_left(7);
            z[i2] ^= z[i1].wrapping_add(z[i0]).rotate_left(9);
            z[i3] ^= z[i2].wrapping_add(z[i1]).rotate_left(13);
            z[i0] ^= z[i3].wrapping_add(z[i2]).rotate_left(18);
            c += 1;
        }
        // row round
        let mut q = 0usize;
        while q < 4 {
            let i0 = 4 * q;
            let i1 = i0 + 1;
            let i2 = i0 + 2;
            let i3 = i0 + 3;
            z[i1] ^= z[i0].wrapping_add(z[i3]).rotate_left(7);
            z[i2] ^= z[i1].wrapping_add(z[i0]).rotate_left(9);
            z[i3] ^= z[i2].wrapping_add(z[i1]).rotate_left(13);
            z[i0] ^= z[i3].wrapping_add(z[i2]).rotate_left(18);
            q += 1;
        }
        r += 1;
    }

    let mut out = 0u32;
    let mut k = 0usize;
    while k < 16 {
        out = out.rotate_left(16) ^ z[k].rotate_left(12);
        out = out.wrapping_add(z[k].rotate_left(8) ^ z[k].rotate_left(7));
        k += 1;
    }
    out
}
