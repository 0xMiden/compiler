// Loop-header W^entry pressure through shared masked rotate counts. The
// translator wraps every rotate count in `arith.band(count, mask)`; the
// folder dedups the constant operands function-wide and CSE merges the
// identical bands across blocks, so a count constant reused in two blocks
// becomes ONE u32 SSA value (one felt) crossing the edge between them.
// Loop 1 reuses SIXTEEN counts between the pre-loop code and rotates of the
// loop-carried `acc` (LICM cannot hoist those), so 18 felts are alive at
// the loop header with in-loop next-use distances — the loop-header W^entry
// computation takes its w_used >= K over-capacity arm (candidate sort +
// take_while fill), and the excluded values force spill/reload
// reconciliation (edge splits) on the preheader edge AND the loop backedge.
// Counts 28/30 are used in the entry block and again only after loop 2;
// their bands cross both loop headers (trace-verified), though liveness
// still reports them with in-loop distances, so they land in the candidate
// set rather than the live-through set (see the scratch/knowledge notes on
// the empty-live-through observation).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    // Pre-loop partners of the sixteen in-loop counts (odd 1..31), plus the
    // first uses of the live-through counts 28/30.
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc ^= m.rotate_left(5);
    acc = acc.wrapping_sub(n.rotate_left(7));
    acc ^= m.rotate_left(9);
    acc = acc.wrapping_add(n.rotate_left(11));
    acc ^= m.rotate_left(13);
    acc = acc.wrapping_sub(n.rotate_left(15));
    acc ^= m.rotate_left(17);
    acc = acc.wrapping_add(n.rotate_left(19));
    acc ^= m.rotate_left(21);
    acc = acc.wrapping_sub(n.rotate_left(23));
    acc ^= m.rotate_left(25);
    acc = acc.wrapping_add(n.rotate_left(27));
    acc ^= m.rotate_left(29);
    acc = acc.wrapping_sub(n.rotate_left(31));
    acc ^= m.rotate_left(28);
    acc = acc.wrapping_add(n.rotate_left(30));
    let iters = (input2 % 97) + 3;
    let mut i: u32 = 0;
    while i < iters {
        // Sixteen rotates of the loop-carried value with the shared counts.
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc ^= acc.rotate_left(5);
        acc = acc.wrapping_sub(acc.rotate_left(7));
        acc ^= acc.rotate_left(9);
        acc = acc.wrapping_add(acc.rotate_left(11));
        acc ^= acc.rotate_left(13);
        acc = acc.wrapping_sub(acc.rotate_left(15));
        acc ^= acc.rotate_left(17);
        acc = acc.wrapping_add(acc.rotate_left(19));
        acc ^= acc.rotate_left(21);
        acc = acc.wrapping_sub(acc.rotate_left(23));
        acc ^= acc.rotate_left(25);
        acc = acc.wrapping_add(acc.rotate_left(27));
        acc ^= acc.rotate_left(29);
        acc = acc.wrapping_sub(acc.rotate_left(31));
        i = i.wrapping_add(1);
    }
    // Loop 2: light body with fresh counts; 28/30 stay live through it.
    let mut acc2 = acc | 1;
    let iters2 = (input1 % 89) + 2;
    let mut j: u32 = 0;
    while j < iters2 {
        acc2 ^= acc2.rotate_left(4);
        acc2 = acc2.wrapping_add(acc2.rotate_left(6));
        j = j.wrapping_add(1);
    }
    // Post-loop-2 partners of the live-through counts 28/30.
    let r = acc2 ^ acc2.rotate_left(28) ^ acc.rotate_left(30);
    (r as u32) ^ ((r >> 32) as u32)
}
