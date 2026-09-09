// Bounding sibling of `prog_feistel` (campaign 21): the same conditional
// Feistel network over 32 counter blocks — the round function applied only on
// the iterations the schedule selects, so the then-arm reads the key material
// and the else-arm none — with TWO round keys instead of six.
const F0: u32 = 9;
const F1: u32 = 17;
const F2: u32 = 27;
const F3: u32 = 39;

const DELTA: u64 = 0x9e37_79b9_7f4a_7c15;

fn round_fn(x: u64, k: u64) -> u64 {
    let y = x.wrapping_add(k).rotate_left(F0);
    (y ^ y.rotate_left(F1)).wrapping_mul(DELTA) ^ y.rotate_left(F2)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Key schedule: six round keys derived once.
    let seed = ((input1 as u64) << 32) | (input2 as u64) | 1;
    let k0 = seed.rotate_left(F0).wrapping_mul(DELTA);
    let k1 = (seed ^ k0).rotate_left(F1).wrapping_add(DELTA);

    let mut left = seed.rotate_left(F2);
    let mut right = seed.rotate_left(F3) ^ DELTA;
    let mut tally = 0u64;
    let mut skipped = 0u64;

    let mut i: u32 = 0;
    while i < 32 {
        let block = ((i as u64) << 32) ^ left;
        // The schedule decides whether this block gets the full round.
        let active = ((block >> 5) ^ (i as u64)) % 97 < 48;
        let mixed = if active {
            // Then-arm: the whole key material is live here.
            let t0 = round_fn(right, k0);
            let t1 = round_fn(t0 ^ left, k1);
            tally = tally.wrapping_add(t1.rotate_left(F3));
            t1 ^ t0.rotate_left(F0)
        } else {
            // Else-arm: no key material at all.
            skipped = skipped.wrapping_add(1);
            right.rotate_left(F1)
        };
        let next = left ^ mixed;
        left = right;
        right = next;
        i += 1;
    }

    let mut out = left.rotate_left(F0) ^ right.rotate_left(F1);
    out = out.wrapping_add(tally.rotate_left(F2));
    out ^= skipped.rotate_left(F3);
    out ^= k0.rotate_left(F0).wrapping_add(k1.rotate_left(F1));
    (out as u32) ^ ((out >> 32) as u32)
}
