// Feistel mixer with conditional rounds (campaign 21, cliff shape:
// asymmetric diamond in a hot loop).  A four-branch Feistel network encrypts
// a sequence of 32 counter blocks; the round function is applied only on the
// iterations the schedule selects, so the then-arm reads all six u64 key
// words and mixes them with four rotate constants while the else-arm only
// advances the counter — the pressure difference the spill analysis has to
// reconcile on the join.  The six key words are derived before the loop and
// folded into the result after it, and the rotate constants 9, 17, 27 and 39
// are shared by the key schedule, both arms and the final fold.
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
    let k2 = (seed.wrapping_add(k1)).rotate_left(F2) ^ DELTA;
    let k3 = (k0 ^ k2).rotate_left(F3).wrapping_mul(DELTA | 3);
    let k4 = (k1.wrapping_sub(k3)).rotate_left(F0) ^ k2;
    let k5 = (k2 ^ k3.rotate_left(F1)).wrapping_add(k4);

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
            let t2 = round_fn(t1.wrapping_add(right), k2);
            let t3 = round_fn(t2 ^ k3.rotate_left(F0), k3);
            let t4 = round_fn(t3.wrapping_sub(k4.rotate_left(F1)), k4);
            let t5 = round_fn(t4 ^ k5.rotate_left(F2), k5);
            tally = tally.wrapping_add(t5.rotate_left(F3));
            t5 ^ (t0.wrapping_add(t2) ^ t4.rotate_left(F0))
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
    out = out.wrapping_sub(k2.rotate_left(F2) ^ k3.rotate_left(F3));
    out ^= k4.rotate_left(F0).wrapping_mul(k5 | 1);
    (out as u32) ^ ((out >> 32) as u32)
}
