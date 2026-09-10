// A simulation driven by three real PRNGs: SplitMix64 seeds the state
// (shifts 30, 27, 31), xoshiro256** produces the stream (rotations 7, 45 and
// the shift 17) and a PCG-style output permutation post-processes it (shifts
// 18, 27, 59 plus a RUNTIME rotate by the top five bits).  EIGHT distinct
// shift/rotate constants -- 27 is shared between SplitMix and PCG -- cross
// the seeding loop, the simulation loop and the statistics fold; the
// random walk keeps five u64 statistics live across the loop.
fn splitmix(z: &mut u64) -> u64 {
    *z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut x = *z;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut seed = ((input1 as u64) << 32) | (input2 as u64) | 1;
    let mut s = [0u64; 4];
    let mut i = 0usize;
    while i < 4 {
        s[i] = splitmix(&mut seed);
        i += 1;
    }

    let steps = 16 + (input2 % 48) as usize;
    let mut pos = 0u64;
    let mut hi = 0u64;
    let mut lo = u64::MAX;
    let mut acc = 0u64;
    let mut crossings = 0u64;

    let mut step = 0usize;
    while step < steps {
        // xoshiro256**
        let r = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);

        // PCG-style output permutation of the raw word.
        let xorshifted = ((r >> 18) ^ r) >> 27;
        let rot = (r >> 59) as u32;
        let perm = (xorshifted as u32).rotate_right(rot);

        // Random walk over the permuted stream.
        let delta = (perm & 0xff) as u64;
        if perm & 0x100 != 0 {
            pos = pos.wrapping_sub(delta);
        } else {
            pos = pos.wrapping_add(delta);
        }
        if pos > hi {
            hi = pos;
        }
        if pos < lo {
            lo = pos;
        }
        if (pos ^ acc) & 0x8000_0000_0000_0000 != 0 {
            crossings = crossings.wrapping_add(1);
        }
        acc = acc
            .wrapping_add(pos ^ (r >> 27))
            .rotate_left(7)
            .wrapping_add(perm as u64);
        step += 1;
    }

    // Fold the statistics with the same constants.
    let mut out = acc ^ (hi >> 30) ^ (lo >> 31);
    out = out.rotate_left(45) ^ (crossings << 17);
    out = out.wrapping_add((pos >> 18) ^ (pos >> 27) ^ (pos >> 59));
    out ^= s[0].rotate_left(7) ^ s[1].rotate_left(45) ^ (s[2] >> 30) ^ (s[3] >> 31);
    (out as u32) ^ ((out >> 32) as u32)
}
