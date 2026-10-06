// Mutual recursion (`ra` -> `rb` -> `ra`, closed through the function-pointer
// table so the assembler's call-graph cycle check does not fire) where the two
// frames carry DIFFERENT spill freight: `ra` has a three-u64 cluster across
// one loop and no count bands, `rb` the same cluster across TWO loops with the
// recursive dispatch between them plus four shared rotate count bands in each
// body, so at every second level the callee spills strictly more than its
// caller. Each frame consumes the callee's result together with its own
// reloaded cluster, so a slot shared between two activations would show up as
// a depth-dependent wrong answer.
type Step = fn(u32, u64) -> u64;

#[inline(never)]
fn leaf(_n: u32, s: u64) -> u64 {
    s.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ s.rotate_left(17)
}

// Lighter frame: one loop, no bands.
#[inline(never)]
fn ra(n: u32, s: u64) -> u64 {
    if n == 0 {
        return leaf(0, s);
    }
    let m = s | 1;
    let q = s.rotate_left(32) | 2;
    let a0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ q;
    let a1 = q.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let a2 = a0.rotate_left(17) ^ q.wrapping_mul(0x94d0_49bb_1331_11eb);
    let trips = ((s >> 9) % 3) as u32 + 2;
    let mut acc = m ^ q.rotate_left(3);
    let mut i: u32 = 0;
    while i < trips {
        acc = acc.rotate_left(1) ^ (a0 ^ acc).wrapping_add(a1 ^ acc.rotate_left(5)).wrapping_mul(a2 | 1);
        i = i.wrapping_add(1);
    }
    let f = STEPS[1 + ((acc >> 3) % 2) as usize];
    let child = f(n - 1, acc | 1);
    child.rotate_left(9) ^ a0.rotate_left(15) ^ a1.rotate_left(19) ^ a2.rotate_left(23) ^ acc
}

// Heavier frame: the same cluster across two loops with the call between them,
// plus four count bands used before, inside both bodies and after.
#[inline(never)]
fn rb(n: u32, s: u64) -> u64 {
    if n == 0 {
        return leaf(1, !s);
    }
    let m = s | 3;
    let q = s.rotate_left(29) | 4;
    let v0 = m.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ q;
    let v1 = q.wrapping_mul(0xa076_1d64_78bd_642f) ^ m.rotate_left(7);
    let v2 = v0.rotate_left(13) ^ q.wrapping_mul(0xe703_7ed1_a0b4_28db);
    let trips = ((s >> 13) % 3) as u32 + 2;
    let mut acc = (m ^ q) | 1;
    acc ^= m.rotate_left(1) | 1;
    acc = acc.wrapping_add(q.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    acc ^= q.rotate_left(7) | 1;
    let mut i: u32 = 0;
    while i < trips {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7) | 1;
        acc ^= (v0 ^ acc).wrapping_add(v1 ^ acc.rotate_left(9)).wrapping_mul(v2 | 1);
        i = i.wrapping_add(1);
    }
    let f = STEPS[1 + ((acc >> 6) % 2) as usize];
    let child = f(n - 1, acc | 1);
    let mut out = child.rotate_left(11) ^ acc;
    let mut j: u32 = 0;
    while j < trips {
        out ^= out.rotate_left(1) | 1;
        out = out.wrapping_add(out.rotate_left(3));
        out = out.wrapping_sub(out.rotate_left(5));
        out ^= out.rotate_left(7) | 1;
        out ^= (v2 ^ out).wrapping_add(v1 ^ out.rotate_left(9)).wrapping_mul(v0 | 1);
        j = j.wrapping_add(1);
    }
    out ^= m.rotate_left(1) | 1;
    out = out.wrapping_add(q.rotate_left(3));
    out ^ v0.rotate_left(15) ^ v2.rotate_left(21)
}

static STEPS: [Step; 3] = [leaf, ra, rb];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = ((input2 as u64) << 32) | input1 as u64;
    let f = STEPS[1 + (input2 % 2) as usize];
    let r = f(input1 % 5, s | 1);
    (r as u32) ^ ((r >> 32) as u32)
}
