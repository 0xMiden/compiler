// Spill slots must be PER ACTIVATION. `rec` carries the campaign-20 freight
// recipe (an eight-u64 cluster defined before a loop, consumed inside it in
// one wide expression, reloaded after it, plus shared masked rotate counts)
// and makes its recursive call BETWEEN the two loops that share the cluster,
// so every cluster value is live across the call and is spilled at the call
// site. The recursion goes through a function-pointer table because the
// assembler's linker rejects a direct call-graph cycle ("found a cycle in the
// call graph", miden-assembly linker/errors.rs:41), and each dispatch takes
// only two arguments so the `hir.exec_indirect` argument blindness of
// `calls::indirect_spill` is not in play. If the callee's spill slots were
// the same memory as the caller's, the values reloaded after the call would
// be the callee's and the answer would change with the recursion depth.
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
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ q;
    let v1 = q.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ q.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let trips = ((s >> 7) % 3) as u32 + 2;
    let mut acc = m ^ q.rotate_left(7);
    let mut i: u32 = 0;
    while i < trips {
        acc = acc.rotate_left(1)
            ^ (v0 ^ v1.rotate_left(3))
                .wrapping_add(v2 ^ v3.rotate_left(5))
                .wrapping_mul(v4 | 1)
            ^ (v5 ^ v6.rotate_left(7)).wrapping_sub(v7 ^ acc.rotate_left(9));
        i = i.wrapping_add(1);
    }
    // One recursive activation per frame, dispatched through the table with
    // the whole cluster live across it.
    let f = STEPS[((acc >> 5) % 3) as usize];
    let child = f(n - 1, acc | 1);
    // Second loop over the SAME cluster after the call returned: these
    // reloads must read this activation's slots, not the callee's.
    let mut out = child.rotate_left(11) ^ acc;
    let mut j: u32 = 0;
    while j < trips {
        out = out.rotate_left(1)
            ^ (v7 ^ v6.rotate_left(3))
                .wrapping_add(v5 ^ v4.rotate_left(5))
                .wrapping_mul(v3 | 1)
            ^ (v2 ^ v1.rotate_left(7)).wrapping_sub(v0 ^ out.rotate_left(9));
        j = j.wrapping_add(1);
    }
    out ^ v0.rotate_left(15) ^ v3.rotate_left(21) ^ v7.rotate_left(29)
}

#[inline(never)]
fn rec2(n: u32, s: u64) -> u64 {
    if n == 0 {
        return leaf(1, !s);
    }
    let f = STEPS[((s >> 11) % 3) as usize];
    f(n - 1, s ^ 0x5851_f42d_4c95_7f2d).rotate_left(7) ^ s
}

static STEPS: [Step; 3] = [leaf, rec, rec2];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = ((input1 as u64) << 32) | input2 as u64;
    let r = rec(input1 % 6, s | 1);
    (r as u32) ^ ((r >> 32) as u32)
}
