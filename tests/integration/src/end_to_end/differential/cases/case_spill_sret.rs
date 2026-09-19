// Spill slots across the two call shapes that write memory behind the
// caller's back: a function-pointer dispatch (`hir.exec_indirect` ->
// `dynexec`, two arguments so the argument-blindness panic of
// `calls::indirect_spill` stays out of the way) and two calls that return a
// wide record through a hidden return-area pointer (`[u64; 4]` and
// `(u64, u64, u64)`), whose callees write four/three words into the caller's
// frame while the caller's eight-u64 cluster is spilled. The cluster is
// reloaded after both, together with the returned words.
type Step = fn(u32, u64) -> u64;

#[inline(never)]
fn mix_a(k: u32, s: u64) -> u64 {
    s.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ (k as u64).rotate_left(7)
}

#[inline(never)]
fn mix_b(k: u32, s: u64) -> u64 {
    s.rotate_left(19) ^ (k as u64).wrapping_mul(0x9e37_79b9)
}

static STEPS: [Step; 2] = [mix_a, mix_b];

// Returned by out-pointer: the callee writes four words into the caller frame.
#[inline(never)]
fn quad(a: u64, b: u64) -> [u64; 4] {
    [
        a ^ b.rotate_left(3),
        a.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ b,
        b.rotate_left(29) ^ a.wrapping_add(0x9e37_79b9_7f4a_7c15),
        (a ^ b).wrapping_mul(0x94d0_49bb_1331_11eb),
    ]
}

#[inline(never)]
fn triple(a: u64, b: u64) -> (u64, u64, u64) {
    (
        a.rotate_left(11) ^ b,
        b.rotate_left(23) ^ a,
        a.wrapping_sub(b).wrapping_mul(0xd6e8_feb8_6659_fd93),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let q = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ q;
    let v1 = q.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ q.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let mut acc = m ^ q.rotate_left(7);
    let mut i: u32 = 0;
    while i < (input2 % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (v0 ^ v1.rotate_left(3))
                .wrapping_add(v2 ^ v3.rotate_left(5))
                .wrapping_mul(v4 | 1)
            ^ (v5 ^ v6.rotate_left(7)).wrapping_sub(v7 ^ acc.rotate_left(9));
        i = i.wrapping_add(1);
    }
    // Dispatch with the whole cluster live across it.
    let f = STEPS[((acc >> 5) % 2) as usize];
    let d = f(input1 % 5, acc | 1);
    // Two return-area calls, still with the cluster live.
    let w = quad(acc ^ d, v3 ^ v6);
    let (t0, t1, t2) = triple(w[0] ^ d, w[3] ^ acc);
    let out = acc
        ^ d.rotate_left(3)
        ^ w[0].rotate_left(5)
        ^ w[1].rotate_left(7)
        ^ w[2].rotate_left(9)
        ^ w[3].rotate_left(11)
        ^ t0.rotate_left(13)
        ^ t1.rotate_left(15)
        ^ t2.rotate_left(17)
        ^ v0.rotate_left(19)
        ^ v1.rotate_left(21)
        ^ v2.rotate_left(23)
        ^ v3.rotate_left(25)
        ^ v4.rotate_left(27)
        ^ v5.rotate_left(29)
        ^ v6.rotate_left(31)
        ^ v7.rotate_left(33);
    (out as u32) ^ ((out >> 32) as u32)
}
