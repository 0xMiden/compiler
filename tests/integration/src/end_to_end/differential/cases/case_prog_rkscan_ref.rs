// `prog_rkscan_guard` with the Bloom-probe expression moved into an
// `#[inline(never)] fn(&[u64; 4], u64)` — the campaign-21/22 "state BY
// REFERENCE helper" rescue, the one that saves `prog_tlv` and `prog_rle`.
// The four fingerprint words live in the shadow stack because the array's
// address escapes into the helper, so nothing about them can be
// Copy-constrained in the search loop's window.
//
// Computes exactly the same answer as `case_prog_rkscan_guard.rs` on the
// whole 1225-pair native boundary grid (checksum 0x2168a1769f80256d).
const RA: u32 = 7;
const RB: u32 = 17;
const RC: u32 = 29;
const RD: u32 = RA;
const RE: u32 = RB;

const BASE: u64 = 0x0100_0000_01b3;
const MIX: u64 = 0x9e37_79b9_7f4a_7c15;

fn fill(seed: u32) -> [u8; 64] {
    let mut buf = [0u8; 64];
    let mut x = seed | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf[i] = (x >> 3) as u8;
        i += 1;
    }
    buf
}

#[inline(never)]
fn probe_of(f: &[u64; 4], hh: u64) -> u64 {
    (f[0] ^ hh.rotate_left(RA))
        .wrapping_add(f[1] ^ hh.rotate_left(RB))
        .wrapping_mul(f[2] | 1)
        .wrapping_sub(f[3] ^ hh.rotate_left(RC))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let hay = fill(input1);

    let mut fw: [u64; 4] = [MIX, MIX.rotate_left(RA), MIX.rotate_left(RB), MIX.rotate_left(RC)];
    let mut w = 0usize;
    while w + 4 <= 64 {
        let word = u32::from_le_bytes([hay[w], hay[w + 1], hay[w + 2], hay[w + 3]]) as u64;
        fw[0] ^= word.rotate_left(RA);
        fw[1] = fw[1].wrapping_add(word.rotate_left(RB));
        fw[2] ^= word.rotate_left(RC).wrapping_mul(BASE);
        fw[3] = fw[3].wrapping_sub(word.rotate_left(RD));
        w += 4;
    }

    let mut acc = ((input1 as u64) << 32) ^ (input2 as u64) ^ fw[0].rotate_left(RA);
    acc = acc.wrapping_add(acc.rotate_left(RB));
    acc ^= acc.rotate_left(RC);

    let mut collisions = 0u32;
    let mut round = 0u32;
    while round < 3 {
        let nb = if round == 2 && (input2 >> 31) == 0 {
            let np = (input2 as usize) % 60;
            [hay[np], hay[np + 1], hay[np + 2], hay[np + 3]]
        } else {
            let nw = input2.rotate_left(round * 8) ^ (round.wrapping_mul(0x9e37_79b9));
            nw.to_le_bytes()
        };
        let mut nh = 0u64;
        let mut k = 0usize;
        while k < 4 {
            nh = nh.wrapping_mul(BASE).wrapping_add(nb[k] as u64);
            k += 1;
        }
        nh ^= nh.rotate_left(RD);

        let mut h = 0u64;
        let mut i = 0usize;
        while i < 4 {
            h = h.wrapping_mul(BASE).wrapping_add(hay[i] as u64);
            i += 1;
        }
        let mut pow = 1u64;
        let mut p = 0usize;
        while p < 3 {
            pow = pow.wrapping_mul(BASE);
            p += 1;
        }

        let mut pos = 0usize;
        loop {
            let hh = h ^ h.rotate_left(RD);
            let probe = probe_of(&fw, hh);
            if (hh ^ nh) & 0xff == 0 {
                let mut same = true;
                let mut t = 0usize;
                while t < 4 {
                    if hay[pos + t] != nb[t] {
                        same = false;
                    }
                    t += 1;
                }
                if same {
                    return (1u32 << 28)
                        | (((probe ^ acc) as u32 ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
                }
                collisions += 1;
                if collisions > 1 {
                    return (2u32 << 28)
                        | (((probe.rotate_left(RB) ^ acc) as u32) & 0x0fff_ffff);
                }
            }
            if hay[pos] == 0x5a {
                return (3u32 << 28) | (((probe.rotate_left(RC) ^ acc) as u32) & 0x0fff_ffff);
            }
            if pos + 4 >= 64 {
                break;
            }
            h = h
                .wrapping_sub((hay[pos] as u64).wrapping_mul(pow))
                .wrapping_mul(BASE)
                .wrapping_add(hay[pos + 4] as u64);
            acc ^= (probe & 0xffff).rotate_left(RE);
            pos += 1;
        }
        acc = acc.wrapping_mul(BASE) ^ h.rotate_left(RA);
        round += 1;
    }

    let mut out = acc ^ fw[0].rotate_left(RA);
    out = out.wrapping_add(fw[1].rotate_left(RB));
    out ^= fw[2].rotate_left(RC);
    out = out.wrapping_sub(fw[3].rotate_left(RD));
    (4u32 << 28) | (((out as u32) ^ ((out >> 32) as u32)) & 0x0fff_ffff)
}
