// Table validation pipeline (campaign 21, cliff shape: a chain of early-break
// scans — the most freight-tolerant shape, kept as the control).  A 48-entry
// u64 table derived from the inputs is checked by five sequential scans —
// find-first out-of-range entry, any duplicate low word, all entries
// monotone, first entry over the quota, and a checksum scan — each of which
// breaks as soon as it has its answer, and all five share the same four
// rotate constants and the six u64 running statistics that are folded into
// the result at the end.
const V0: u32 = 6;
const V1: u32 = 18;
const V2: u32 = 30;
const V3: u32 = 42;

const MIXC: u64 = 0x2545_f491_4f6c_dd1d;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut table = [0u64; 48];
    let mut x = input1 | 1;
    let mut i = 0usize;
    while i < 48 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        table[i] = ((x as u64) << 24).wrapping_mul(MIXC) ^ ((input2 as u64) << (i as u32 % 17));
        i += 1;
    }

    let limit = ((input2 as u64) << 32) | 0x00ff_ffff;
    let quota = (input1 as u64).wrapping_mul(MIXC) | 1;

    let mut first_bad = 48u64;
    let mut dup = 0u64;
    let mut monotone = 1u64;
    let mut over = 48u64;
    let mut check = MIXC.rotate_left(V0) ^ (input1 as u64);
    let mut folded = MIXC.rotate_left(V1) ^ (input2 as u64);

    // Scan 1: the first entry outside the allowed range.
    let mut a = 0usize;
    while a < 48 {
        let v = table[a];
        check ^= v.rotate_left(V0);
        if v > limit {
            first_bad = a as u64;
            break;
        }
        a += 1;
    }

    // Scan 2: any duplicate low word among the first 24 entries.
    let mut b = 1usize;
    while b < 24 {
        folded = folded.wrapping_add(table[b].rotate_left(V1));
        if (table[b] ^ table[b - 1]) & 0xffff == 0 {
            dup = b as u64;
            break;
        }
        b += 1;
    }

    // Scan 3: monotonicity of the rotated keys.
    let mut c = 1usize;
    while c < 48 {
        let prev = table[c - 1].rotate_left(V2);
        let cur = table[c].rotate_left(V2);
        check = check.wrapping_mul(MIXC) ^ cur;
        if cur < prev {
            monotone = 0;
            break;
        }
        c += 1;
    }

    // Scan 4: the first entry over the quota.
    let mut d = 0usize;
    while d < 48 {
        folded ^= table[d].rotate_left(V3);
        if table[d] > quota {
            over = d as u64;
            break;
        }
        d += 1;
    }

    // Scan 5: checksum of the accepted prefix.
    let mut e = 0usize;
    let stop = if first_bad < 48 { first_bad as usize } else { 48 };
    while e < stop {
        check = (check ^ table[e].rotate_left(V0)).wrapping_mul(MIXC);
        folded = folded.wrapping_sub(table[e].rotate_left(V1));
        if check & 0x3ff == 0x2a {
            break;
        }
        e += 1;
    }

    let mut out = check.rotate_left(V0) ^ folded.rotate_left(V1);
    out = out.wrapping_add(first_bad.rotate_left(V2));
    out ^= dup.rotate_left(V3);
    out = out.wrapping_mul(monotone | 1);
    out ^= over.rotate_left(V0) ^ (e as u64).rotate_left(V1);
    (out as u32) ^ ((out >> 32) as u32)
}
