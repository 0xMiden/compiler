// Spill pressure under a dense `match` inside a `% 97`-bounded loop. Ten
// masked rotate-count bands (CSE-merged cross-block SSA values, see
// case_spill_loop_mix) are used before the loop and after it, so they must
// cross the loop AND the 6-way dispatch inside it every iteration — spilled
// values crossing scf.index_switch arm edges, per-arm edge reconciliation
// on the in-loop switch (multi-region successor traversals in the spill
// analysis' liveness/DCA walks), with structurally-varied arm bodies so the
// dispatch survives as a br_table.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    // First uses of the six shared counts.
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc ^= m.rotate_left(5);
    acc = acc.wrapping_sub(n.rotate_left(7));
    acc ^= m.rotate_left(9);
    acc = acc.wrapping_add(n.rotate_left(11));
    let iters = (input2 % 97) + 3;
    let mut i: u32 = 0;
    while i < iters {
        // Dense 6-way dispatch with structurally-distinct arms.
        acc = match (acc as u32) & 7 {
            0 => (acc ^ 0x9e37_79b9).wrapping_mul(129),
            1 => acc.rotate_left(2).wrapping_add(0x85eb_ca6b),
            2 => acc.wrapping_sub(0xc2b2_ae35) ^ (acc >> 5),
            3 => acc.rotate_left(6) ^ acc.wrapping_mul(65),
            4 => acc.wrapping_add(acc.rotate_left(10)) | 1,
            _ => acc ^ (acc << 3) ^ 0x27d4_eb2f,
        };
        i = i.wrapping_add(1);
    }
    // Post-loop partners of the six crossing counts.
    let mut r = acc;
    r ^= acc.rotate_left(1);
    r = r.wrapping_add(acc.rotate_left(3));
    r ^= acc.rotate_left(5);
    r = r.wrapping_sub(acc.rotate_left(7));
    r ^= acc.rotate_left(9);
    r = r.wrapping_add(acc.rotate_left(11));
    (r as u32) ^ ((r >> 32) as u32)
}
