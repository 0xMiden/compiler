// A helper that -Oz keeps as a real call (no inline attributes: LLVM's own
// per-callee size arithmetic) invoked from three sites, two of them INSIDE a
// loop across which five masked rotate count bands are live. The call
// arguments are reused after each call (Copy constraints at the call) and the
// result is consumed at depth, so the call marshalling happens with the
// un-hoisted count bands sitting under the argument window.
// masked count bands crossing it. Five constant rotate counts are used before
// the loop and inside it on the loop-carried accumulator, so the CSE-merged
// bands are live across the call; the helper's arguments are reused after the
// call and its result is consumed at depth.
fn step(a: u32, b: u32, c: u32, d: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9e37_79b9) ^ b.rotate_left(5);
    x = x.wrapping_add(c).rotate_left(7) ^ d;
    x ^= x >> 15;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    x
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc ^= m.rotate_left(5);
    acc = acc.wrapping_sub(n.rotate_left(7));
    acc ^= m.rotate_left(9);
    let p = input1 | 3;
    let q = input2 | 5;
    let iters = (input2 % 97) + 3;
    let mut i: u32 = 0;
    let mut side: u32 = 0;
    while i < iters {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        side = step(side ^ p, q, acc as u32, i);
        acc ^= acc.rotate_left(5);
        acc = acc.wrapping_sub(acc.rotate_left(7));
        acc ^= acc.rotate_left(9);
        side = side.wrapping_add(step(p, q, side, i ^ 1));
        i = i.wrapping_add(1);
    }
    let r = acc ^ (side as u64) ^ step(p, q, side, 0) as u64;
    (r as u32) ^ ((r >> 32) as u32) ^ p ^ q
}
