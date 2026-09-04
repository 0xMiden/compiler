// Asymmetric-pressure diamond around cross-edge SSA values. The pinned
// call keeps `vx`'s definition before the branch, and the rotate-count
// constants shared by the entry block, both arms, and the join become
// CSE-merged `arith.band` mask ops — SSA values live across BOTH arms
// (trace-verified; user values in locals never cross edges in W). The
// heavy arm's ten-u64 reload cluster (~20 felts) spills those cross-edge
// values inside that arm only; the cheap arm keeps them on the operand
// stack. At the join, W^entry selects them (candidates inherited from the
// cheap arm), so control-flow-edge reconciliation places reloads on the
// heavy edge and compensating spills on the cheap edge — both edges are
// unstructured (Predecessor::Block), so the analysis records edge SPLITS
// and the transform materializes split blocks, redirects the branches, and
// inserts the spills/reloads with Placement::Split — the i1289-gated
// edge-split cluster that symmetric-pressure shapes (spill_branch/
// spill_edge) never reach.
use core::sync::atomic::{AtomicU32, Ordering};

/// Never written with a non-zero value: `fetch_add(0)` is an opaque no-op
/// whose memory side effect pins the call site (LLVM sinks pure calls to
/// their use, which would move `vx`'s definition past the branch).
static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn scramble(a: u64) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    (a ^ p).wrapping_mul(0x2545_f491_4f6c_dd1d) ^ a.rotate_left(13)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n;
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ n.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let v8 = v6.wrapping_add(v4.rotate_left(19)) ^ n.rotate_left(3);
    let v9 = v7.wrapping_sub(v5.rotate_left(21)) ^ m.rotate_left(5);
    // Cross-edge value: exactly one use, after the join.
    let vx = scramble(m ^ n.rotate_left(9));
    let t = if m % 97 < 48 {
        // Heavy arm: wide tree over all ten u64s — the arm-top local reloads
        // hold ~20 felts, so `vx` (furthest next use) is spilled here.
        (v1 ^ v9.rotate_left(1))
            .wrapping_add(v2 ^ v8.rotate_left(3))
            .wrapping_mul(v3 | 1)
            ^ v7.rotate_left(5)
            ^ (v4 ^ v6.rotate_left(7)).wrapping_sub(v5 ^ v0.rotate_left(9))
    } else {
        // Cheap arm: `vx` stays resident on the operand stack.
        m ^ n.rotate_left(3)
    };
    // Join: `vx` is consumed first.
    let r = vx ^ t.rotate_left(21) ^ v0.rotate_left(31) ^ v9.rotate_left(35);
    (r as u32) ^ ((r >> 32) as u32)
}
