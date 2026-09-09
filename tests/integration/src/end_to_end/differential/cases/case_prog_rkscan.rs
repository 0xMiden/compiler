// Rabin-Karp substring scanner (campaign 21, cliff shape: return-heavy inner
// loop nested in an outer loop).  Three needles derived from the inputs are
// searched for in a 64-byte xorshift-filled buffer with a rolling hash; the
// inner scan loop has four early exits (exact match, collision budget
// exhausted, sentinel byte, scan budget) and the six u64 fingerprint words of
// the haystack, computed before the search, are combined in ONE Bloom-style
// probe inside the loop and folded into the result after it.  The five rotate
// constants seed the hash, update it inside the loop and finalize it.
const RA: u32 = 7;
const RB: u32 = 17;
const RC: u32 = 29;
const RD: u32 = 41;
const RE: u32 = 53;

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

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let hay = fill(input1);

    // Fingerprint words of the haystack: a six-word Bloom filter over the
    // 4-byte windows, built once before any search.
    let mut f0 = MIX;
    let mut f1 = MIX.rotate_left(RA);
    let mut f2 = MIX.rotate_left(RB);
    let mut f3 = MIX.rotate_left(RC);
    let mut f4 = MIX.rotate_left(RD);
    let mut f5 = MIX.rotate_left(RE);
    let mut w = 0usize;
    while w + 4 <= 64 {
        let word = u32::from_le_bytes([hay[w], hay[w + 1], hay[w + 2], hay[w + 3]]) as u64;
        f0 ^= word.rotate_left(RA);
        f1 = f1.wrapping_add(word.rotate_left(RB));
        f2 ^= word.rotate_left(RC).wrapping_mul(BASE);
        f3 = f3.wrapping_sub(word.rotate_left(RD));
        f4 ^= word.rotate_left(RE);
        f5 = f5.wrapping_add(word ^ f0.rotate_left(RA));
        w += 4;
    }

    let mut acc = ((input1 as u64) << 32) ^ (input2 as u64) ^ f0.rotate_left(RA);
    acc = acc.wrapping_add(acc.rotate_left(RB));
    acc ^= acc.rotate_left(RC);

    let mut collisions = 0u32;
    let mut round = 0u32;
    while round < 3 {
        // Rounds 0 and 1 look for a caller-supplied pattern; round 2 looks for
        // a window taken from the buffer itself (a de-duplication scan) unless
        // the caller asked for external patterns only.
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

        // Rolling hash of the first window.
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
            // Bloom probe: every fingerprint word is live at this point.
            let probe = (f0 ^ hh.rotate_left(RA))
                .wrapping_add(f1 ^ hh.rotate_left(RB))
                .wrapping_mul(f2 | 1)
                .wrapping_sub(f3 ^ hh.rotate_left(RC))
                ^ f4.wrapping_add(hh.rotate_left(RD))
                ^ f5.rotate_left(RE);
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
            // Roll the window one byte forward.
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

    let mut out = acc ^ f0.rotate_left(RA);
    out = out.wrapping_add(f1.rotate_left(RB));
    out ^= f2.rotate_left(RC);
    out = out.wrapping_sub(f3.rotate_left(RD));
    out ^= f4.rotate_left(RE) ^ f5;
    (4u32 << 28) | (((out as u32) ^ ((out >> 32) as u32)) & 0x0fff_ffff)
}
