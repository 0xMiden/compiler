// The campaign-20 spill freight recipe inside a frame that recurses through a
// function-pointer table (the assembler's linker rejects a direct call-graph
// cycle), with the recursive dispatch placed between the two loops that share
// the cluster. Three u64 cluster values are consumed by one wide right-leaning
// non-reassociable chain per loop body, and three masked rotate count bands
// are used before the first loop, on the accumulator inside both bodies and
// after the second loop, so the values are live across the dispatch and a
// spill slot is read back after it returns. This is the largest rung of the
// (cluster, bands) ladder that compiles; see the test's doc comment.
type Step = fn(u32, u64) -> u64;

#[inline(never)]
fn leaf(_n: u32, s: u64) -> u64 {
    s.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ s.rotate_left(29)
}

#[inline(never)]
fn rec(n: u32, s: u64) -> u64 {
    if n == 0 {
        return leaf(0, s);
    }
    let m = s | 1;
    let q = s.rotate_left(32) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ q.rotate_left(2);
    let v1 = q.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(3);
    let v2 = m.wrapping_mul(0x94d0_49bb_1331_11eb) ^ q.rotate_left(4);
    let trips = ((s >> 7) % 3) as u32 + 2;
    let mut acc = (m ^ q) | 1;
    acc ^= m.rotate_left(1) | 1;
    acc = acc.wrapping_add(q.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    let mut i: u32 = 0;
    while i < trips {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= (v0 ^ acc.rotate_left(2))
            .wrapping_sub(v1.wrapping_add(acc))
            .wrapping_mul(v2 ^ acc.rotate_left(6));
        i = i.wrapping_add(1);
    }
    let f = STEPS[((acc >> 5) % 2) as usize];
    let child = f(n - 1, acc | 1);
    let mut out = child.rotate_left(11) ^ acc;
    let mut j: u32 = 0;
    while j < trips {
        out ^= out.rotate_left(1) | 1;
        out = out.wrapping_add(out.rotate_left(3));
        out = out.wrapping_sub(out.rotate_left(5));
        out ^= (v0 ^ out.rotate_left(2))
            .wrapping_sub(v1.wrapping_add(out))
            .wrapping_mul(v2 ^ out.rotate_left(6));
        j = j.wrapping_add(1);
    }
    out ^= m.rotate_left(1) | 1;
    out = out.wrapping_add(q.rotate_left(3));
    out = out.wrapping_sub(m.rotate_left(5));
    out ^ v0.rotate_left(15) ^ v2.rotate_left(29)
}

static STEPS: [Step; 2] = [leaf, rec];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = ((input1 as u64) << 32) | input2 as u64;
    let r = rec(input1 % 6, s | 1);
    (r as u32) ^ ((r >> 32) as u32)
}
