// Diamond inside a kept loop whose arms differ in pressure: the then-arm
// re-rotates the accumulator with all six CSE-merged count bands, the else-arm
// is a single add. That asymmetry is what makes the spill analysis reconcile
// the two edges (a value in W^entry of the join that one predecessor does not
// carry gets a reload split onto that edge), so the diamond the post-lift
// canonicalizer turns into `cf.select`s carries band traffic on one side only.
// The direction is chosen by `input1`'s bits, one bit per trip, so a grid can
// pin all-then, all-else and alternating paths.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    acc ^= n.rotate_left(7);
    acc = acc.wrapping_add(m.rotate_left(9));
    acc = acc.wrapping_sub(n.rotate_left(11));
    let iters = (input2 % 11) + 2;
    let mut i: u32 = 0;
    while i < iters {
        let t = if (input1 >> (i & 31)) & 1 == 1 {
            acc ^= acc.rotate_left(1) | 1;
            acc = acc.wrapping_add(acc.rotate_left(3));
            acc = acc.wrapping_sub(acc.rotate_left(5));
            acc ^= acc.rotate_left(7) | 1;
            acc = acc.wrapping_add(acc.rotate_left(9));
            acc = acc.wrapping_sub(acc.rotate_left(11));
            acc ^ 1
        } else {
            acc.wrapping_add(3)
        };
        acc = acc.wrapping_mul(0x0100_0193) ^ t;
        i = i.wrapping_add(1);
    }
    let mut out = acc ^ 0x5a5a_5a5a_5a5a_5a5a;
    out ^= acc.rotate_left(1);
    out = out.wrapping_add(out.rotate_left(3));
    out ^= acc.rotate_left(5);
    out = out.wrapping_add(out.rotate_left(7));
    out ^= acc.rotate_left(9);
    out = out.wrapping_add(out.rotate_left(11));
    (out as u32) ^ ((out >> 32) as u32)
}
