//! Operand-scheduler and spill pressure ladders (campaign 10, 2026-09-02):
//! per ladder, the largest shape that compiles and passes differentially is
//! kept as a boundary guard, and every new failure class beyond a boundary
//! is kept as an `#[ignore]`d minimal reproducer with its full signature.

use super::super::harness::{run_case, run_case_with_inputs};

/// Ladder 1/2 (arity and constraint patterns) boundary guard: a single-block
/// chain of EIGHTEEN shared rotate counts on a multi-use u64. The counts are
/// CSE-merged `arith.band` values and `x` is one CSE-merged `load_local`
/// with nineteen uses, so every `x` rotate but the last is an arity-2
/// problem with a Copy-constrained `x`; LLVM schedules the rotates ahead of
/// the xor/add chain, so their results are the freight. Eighteen counts is
/// the largest count that compiles (thirteen for the both-Copy variant that
/// also reuses the counts a third time); twenty counts panics with the known
/// arity-2 `NoSolution` at lowering.rs:109 (`[Move, Copy]` on the first
/// band, 15-felt in-contract stack) — the `rotl_window` class, reproduced
/// with no loop, dispatch, or cross-edge spill at all. Arity-1 ops on the
/// same deep `x` (`unary_window`), `[Copy, Move]` and `[Move, Move]`
/// patterns at thirteen counts all pass.
#[test]
fn chain_window() {
    run_case("chain_window", include_str!("../cases/case_chain_window.rs"));
}

/// Ladder 1 arity-1 rung: `leading_zeros` of a multi-use u64 under thirteen
/// shared counts plus the accumulator. Arity-1 problems bypass the solver
/// (`solve_and_apply` emits the dup/movup directly) and every operand of a
/// <= 16-felt stack is addressable, so the arity-1 ladder has no failure
/// step; this pins it.
#[test]
fn unary_window() {
    run_case("unary_window", include_str!("../cases/case_unary_window.rs"));
}

/// Ladder 3 (width mix) guard: sixteen shared rotate counts alternating
/// between u64 and u32 sources, so the window interleaves one- and two-felt
/// words when the Copy-constrained `x`/`x32` are copied from under them.
#[test]
fn width_mix() {
    run_case("width_mix", include_str!("../cases/case_width_mix.rs"));
}

/// Ladder 4 (spill boundaries, straight line) guard: a right-leaning
/// non-reassociable tree over sixteen u64 leaves (32 felts) in one block —
/// twice the window through the single-block spill path, no cliff (the
/// u32 17/32-leaf and mixed 24-leaf rungs pass too).
#[test]
fn pressure_u64_32() {
    run_case("pressure_u64_32", include_str!("../cases/case_pressure_u64_32.rs"));
}

/// Ladder 4 (spill boundaries, call) guard: ten u64s live across two calls
/// to a pinned `#[inline(never)]` helper whose three u64 arguments are used
/// again afterwards — arity-3-with-copies scheduling at call sites with
/// twenty felts of live state.
#[test]
fn call_live() {
    run_case("call_live", include_str!("../cases/case_call_live.rs"));
}

/// Ladder 5 (zero-trip loops with spilled state) guard: ten shared rotate
/// counts crossing a ZERO-TRIP-CAPABLE `while i < input2 % 97` loop (LLVM
/// keeps the loop guard and a bypass edge; the corpus's `% 97 + 3` bounds
/// become bottom-test loops without one), plus the 28/30 live-through pair
/// and a second zero-trip-capable loop. Ten is the largest count that
/// compiles — eleven hits `zero_trip_frontier`, and the single-loop shape
/// hits `zero_trip_overflow` at twelve.
/// Configuration note (campaign 16): at `--optimize=size-min` ten counts
/// already hit the F2 gap (`NoSolution` on `arith.rotl`, `spill_loop_mix_oz`
/// class), one rung below the O2 boundary; O3 and O1 pass.
#[test]
fn zero_trip_guard() {
    run_case("zero_trip_guard", include_str!("../cases/case_zero_trip_guard.rs"));
}

/// Pinned zero-trip / one-trip inputs for `zero_trip_guard`: both loops
/// skipped, first loop skipped only, second loop skipped only, single trips.
/// The spilled counts must read back correctly on the bypass paths.
#[test]
fn zero_trip_guard_repro() {
    run_case_with_inputs(
        "zero_trip_guard_repro",
        include_str!("../cases/case_zero_trip_guard.rs"),
        &[(0, 0), (1, 0), (0, 97), (89, 194), (178, 1), (5, 98)],
    );
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-09-02): building this case
/// panics with `called Option::unwrap() on a None value` at
/// hir/src/ir/dominance/frontier.rs:123 (`DominanceFrontier::new`, called
/// from `midenc_hir_transform::spill::rewrite_cfg_spills`). Shape: the
/// `zero_trip_guard` case with ELEVEN shared rotate counts — counts shared
/// between the pre-loop code and rotates of the loop-carried accumulator
/// inside a `while i < input2 % 97` loop (zero-trip-capable, so LLVM keeps a
/// bypass edge around it), the 28/30 live-through pair, and a second
/// zero-trip-capable loop. Root cause (triage 2026-09-02 with
/// `MIDENC_TRACE='pass:spills=trace'`): the pass splits six control-flow
/// edges to place reloads, then rebuilds SSA form with the `DominanceInfo`
/// the spill ANALYSIS computed and cached BEFORE those splits
/// (hir-analysis spills.rs `dominfo.dominance(entry_region)`; hir-transform
/// spill.rs `rewrite_cfg_spills` calls `dominfo.dominance(region)` on the
/// same cached analysis). The split blocks are absent from that tree, so
/// when `DominanceFrontier::new` walks the predecessors of a join and meets
/// a split block, `domtree.get(Some(p))` is `None`. The walk only runs for
/// blocks with THREE or more predecessors (`enumerate().any(|(i, _)| i >
/// 1)`), which is why the two-predecessor diamond `spill_split` never
/// reaches the unwrap even though its split-edge reloads are affected the
/// same way (see `zero_trip_overflow`). Not the F1 spills defect (no "unused
/// phi" warning in the trace) and not the arity-2 solver gap (no solver
/// involved). Bounded by: ten counts with the same two loops compile and
/// pass on pinned zero-trip inputs (`zero_trip_guard`); the same shape with
/// `% 97 + 3` bounds (no bypass edge; `spill_loop_mix`, sixteen counts)
/// passes. Compile-time — no inputs involved. Un-ignore when this case
/// compiles (the transform recomputes dominance after splitting edges).
///
/// Producer-set correction (campaign 20): a zero-trip-capable loop is NOT
/// necessary for this unwrap, only a join with three or more predecessors
/// reached through one of the transform's own split edges. Two shapes with
/// bottom-tested `(input % k) + 2` bounds reach it: a sixteen-arm `match`
/// inside a kept loop with nine or ten shared count bands crossing it (the
/// arms are the many-predecessor join), and two sequential loops where the
/// bands are used before the first and again only inside the SECOND. Both are
/// also opt-level-dependent in the other direction from the documented rungs —
/// they panic at the DEFAULT guest opt-level and compile at
/// `--optimize=size-min`. The passing compositions of those shapes are
/// `interact::dispatch_spill` (ten bands) and `interact::sink_spill`. Both
/// shapes are now kept as runnable minimal reproducers of their own,
/// `frontier_dispatch` and `frontier_seq` below (campaign 21).
#[test]
#[ignore = "compiler panic: 'called Option::unwrap() on a None value' at \
            hir/src/ir/dominance/frontier.rs:123 (DominanceFrontier::new from \
            spill::rewrite_cfg_spills) — the spill transform rebuilds SSA form from a dominator \
            tree cached before its own edge splits (compile-time, no inputs involved)"]
fn zero_trip_frontier() {
    run_case("zero_trip_frontier", include_str!("../cases/case_zero_trip_frontier.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-09-02): building this case
/// panics with `NoSolution` at codegen/masm/src/lower/lowering.rs:109 while
/// scheduling `arith.rotl` with constraints `[Copy, Move]` over an
/// EIGHTEEN-felt operand stack (two u64 rotate results above fourteen u32
/// count values) — the emitter stack exceeds the K = 16 cap the spill
/// analysis is supposed to enforce, so the Copy-constrained count at index
/// 14 is simply outside the MASM window. Shape: twelve masked rotate counts
/// shared between the pre-loop code, rotates of the loop-carried
/// accumulator inside a `while i < input2 % 97` loop (zero-trip-capable, so
/// LLVM keeps a bypass edge), and the post-loop code. Root cause (same
/// defect as `zero_trip_frontier`, trace-verified): the pre-lift pass DID
/// spill the counts and placed their reloads in three split-edge blocks,
/// but its SSA reconstruction walks a dominator tree computed before the
/// splits, never visits the split blocks, and erases all nine split-edge
/// reloads as unused ("erase unused reload" in the pass trace); the in-loop
/// and post-loop uses keep referring to the original values, which stay
/// live on the operand stack past their spills, and the post-lift pass
/// spills them a second time to no effect. The passing corpus cases with
/// edge splits (`spill_split`, `spill_loop_mix`, `spill_switch`) lose their
/// split-edge reloads the same way and pass only because the unrelieved
/// pressure still fits the window. Not F1 (no "unused phi" warning) and not
/// the `rotl_window` class (that one is in-contract, <= 16 felts). Bounded
/// by: ten counts compile and pass with zero-trip inputs pinned (single
/// loop, and `zero_trip_guard` for the two-loop shape); the `% 97 + 3` bound
/// (no bypass edge) passes at every count tried; six counts with the
/// `spill_switch` 6-way match body reach a 22-felt stack (`[Move, Move]` on
/// `arith.bxor`), five pass. Compile-time — no inputs involved. Un-ignore
/// when this case compiles.
#[test]
#[ignore = "compiler panic: 'with error: NoSolution' at codegen/masm/src/lower/lowering.rs:109 on \
            an 18-felt operand stack — the spill transform erases its split-edge reloads (stale \
            dominator tree), so spilled counts stay live past a zero-trip-capable loop \
            (compile-time, no inputs involved)"]
fn zero_trip_overflow() {
    run_case("zero_trip_overflow", include_str!("../cases/case_zero_trip_overflow.rs"));
}

/// Ladder 6 (reload placement / region results) guard: twelve counts shared
/// between the pre-loop code and the loop body (dead after the loop) plus
/// two counts used only before and after it, across a bottom-test `scf.while`
/// that carries a dispatch discriminator and a payload column (an in-loop
/// `return`). The loop-header budget leaves room for the while's result
/// columns; the twelve dead-inside counts are dropped at the post-op drop
/// site and the two live-through counts are reloaded after the loop. Every
/// (in-loop, live-through) split from (2, 12) to (12, 2) passes with the
/// `+ 3` bound; the same splits with a zero-trip-capable bound all fail in
/// the `zero_trip_overflow` class.
/// Configuration note (campaign 16): at `--optimize=size-min` the un-hoisted
/// bands push the copy-constrained count past the window and the case hits
/// the F2 gap (`spill_loop_mix_oz` class); O2, O3 and O1 pass.
#[test]
fn while_results() {
    run_case("while_results", include_str!("../cases/case_while_results.rs"));
}

/// Ladder 7 (select chains) guard: one reused condition feeding eight
/// selects over two multi-use u64 values that both stay live past every
/// select, with u64 freight computed before the selects and consumed after
/// them — arity-3 `cf.select` problems with a Copy-constrained condition and
/// arms at increasing depth (nested two-condition selects and selects that
/// choose rotate counts pass as well).
#[test]
fn select_chain() {
    run_case("select_chain", include_str!("../cases/case_select_chain.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, campaign 21, 2026-09-09): the
/// minimal `frontier.rs:123` reproducer with NO zero-trip-capable loop — a
/// sixteen-arm `match` inside a bottom-tested `(input1 % 13) + 2` loop, three
/// of whose arms carry a dynamically impossible `panic!()` guard (the
/// `SimplifyCondBrLikeSwitch` producer from `canon::trap_dispatch`), with NINE
/// masked rotate count bands used before the loop, on the loop-carried
/// accumulator inside it, and after it. Building it panics with `called
/// Option::unwrap() on a None value` at hir/src/ir/dominance/frontier.rs:123
/// (`DominanceFrontier::new` from `spill::rewrite_cfg_spills`), the same
/// defect `zero_trip_frontier` documents: the transform rebuilds SSA form from
/// the dominator tree the spill ANALYSIS cached before the transform's own
/// edge splits, and the many-predecessor join of the dispatch is reached
/// through one of those split blocks. It is a DEFAULT-level-only failure —
/// `--optimize=size-min` compiles the same source.
/// Minimality (campaign 21, one probe per axis, all at the default level):
/// removing the three impossible arms compiles, keeping only ONE of them
/// compiles, eight arms with one impossible arm compiles, four arms compiles,
/// eight bands instead of nine compiles, dropping the post-loop uses of the
/// bands compiles, and dropping the in-loop uses compiles — so every
/// ingredient is necessary at this size. Compile-time — no inputs involved.
/// Un-ignore when the transform recomputes dominance after splitting edges.
#[test]
#[ignore = "compiler panic: 'called `Option::unwrap()` on a `None` value' at \
            hir/src/ir/dominance/frontier.rs:123 (DominanceFrontier::new from \
            spill::rewrite_cfg_spills) — a 16-arm dispatch with three impossible arms and nine \
            crossing count bands in a bottom-tested loop, no zero-trip loop involved; \
            DEFAULT-level only (compiles at --optimize=size-min), compile-time, no inputs"]
fn frontier_dispatch() {
    run_case("frontier_dispatch", include_str!("../cases/case_frontier_dispatch.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, campaign 21, 2026-09-09): the
/// second minimal `frontier.rs:123` reproducer without a zero-trip-capable
/// loop, and the smallest freight in the corpus that reaches the unwrap — two
/// sequential bottom-tested loops (`(input1 % 7) + 2` and `(input2 % 7) + 2`)
/// where FOUR masked rotate count bands are used before the first loop and
/// again only inside the second one. Same panic and same mechanism as
/// `frontier_dispatch`, and likewise DEFAULT-level only (`--optimize=size-min`
/// compiles it). Band ladder at the default level (campaign 21): two and three
/// bands compile, four through eight panic; campaign 20 had only measured
/// eight. Bounded by the same shape with the FIRST loop removed, which
/// compiles at every band count tried — one loop is not enough, the bands must
/// cross a loop before reaching the region that uses them. Compile-time — no
/// inputs involved.
#[test]
#[ignore = "compiler panic: 'called `Option::unwrap()` on a `None` value' at \
            hir/src/ir/dominance/frontier.rs:123 (DominanceFrontier::new from \
            spill::rewrite_cfg_spills) — two sequential bottom-tested loops with four count bands \
            used before the first and only inside the second; DEFAULT-level only (compiles at \
            --optimize=size-min), compile-time, no inputs"]
fn frontier_seq() {
    run_case("frontier_seq", include_str!("../cases/case_frontier_seq.rs"));
}

/// MINIMAL DEFAULT-LEVEL REPRODUCER of the F6 EMITTER site (campaign 28): 24
/// lines, no dispatch, no zero-trip loop, no nested loop. FOUR u64 values are
/// defined before a bottom-tested `(input1 % 7) + 2` loop, consumed inside it
/// in ONE wide expression (so all four are live at a single program point) and
/// used again after it, with ONE masked rotate count band used before, inside
/// and after the same loop. Building it panics with `invalid operand stack
/// index (10): requires access to more than 16 elements, which is not
/// supported in Miden` at codegen/masm/src/emit/mod.rs:623.
/// CLASSIFIED F6 by trace, not by site
/// (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`): `edges to split
/// = 1` and SIX `erase unused reload` lines — the spill transform rebuilds SSA
/// form from the dominator tree the spill ANALYSIS cached before the
/// transform's own edge split, never visits the split block, and erases the
/// reloads it placed there, so the spilled values stay live on the operand
/// stack past their spills. Same defect as `zero_trip_frontier` /
/// `frontier_seq`, met at the emitter instead of the dominance frontier.
/// Panics at ALL FOUR optimization levels (emit/mod.rs:623 at the default
/// level and at max, lowering.rs:109 at size-min and basic), so no `-O` escape
/// exists for it. Bounded by `window_erased_guard` below, which removes the
/// band (the same four values, same loop, same post-loop uses), and by the
/// three-value/one-band rung, which also compiles; note the band axis is
/// NON-MONOTONE here — two bands compile again at the default level.
/// Compile-time — no inputs involved. Un-ignore when the transform recomputes
/// dominance after splitting edges.
#[test]
#[ignore = "compiler panic: 'invalid operand stack index (10): requires access to more than 16 \
            elements' at codegen/masm/src/emit/mod.rs:623 — F6 (edges to split = 1, six erased \
            split reloads): four u64 values live across a bottom-tested loop plus one crossing \
            count band; all four optimization levels, compile-time, no inputs"]
fn window_erased_min() {
    run_case("window_erased_min", include_str!("../cases/case_window_erased_min.rs"));
}

/// Passing sibling of [`window_erased_min`] and [`overflow_cluster_min`]
/// (campaign 28): the same shape with the count band removed — four u64 values
/// defined before the loop, joined in one wide expression inside it and used
/// again after it. The spill analysis requests NO spill here (`edges to split`
/// never appears in the trace), so this rung is the last one below the cliff;
/// it also passes at `--optimize=size-min` and `--optimize=max` and panics at
/// `--optimize=basic` (lowering.rs:109), which is the campaign-21 "opt level is
/// not a safety ladder" rule at minimal scale.
#[test]
fn window_erased_guard() {
    run_case("window_erased_guard", include_str!("../cases/case_window_erased_guard.rs"));
}

/// Pinned trip-count grid for [`window_erased_guard`]: the minimum (2) and
/// maximum (8) trips of the `(input1 % 7) + 2` loop, the all-zero and all-ones
/// rows, and an equal pair — the values the cluster and the wide expression
/// must still compute correctly when the analysis is one rung below spilling.
#[test]
fn window_erased_guard_edges() {
    run_case_with_inputs(
        "window_erased_guard_edges",
        include_str!("../cases/case_window_erased_guard.rs"),
        &[(0, 0), (6, 1), (7, 0xffff_ffff), (0xffff_ffff, 0xffff_ffff), (13, 13)],
    );
}

/// MINIMAL DEFAULT-LEVEL REPRODUCER of the F6 LOWERING site WITHOUT a
/// zero-trip-capable loop (campaign 28): 23 lines — [`window_erased_guard`]
/// with one more u64 value in the cluster (five defined before the loop,
/// joined in one wide expression inside it, used again after it) and no count
/// band at all. Building it panics with `failed to schedule operands: [%24,
/// %82] for inst 'arith.rotl' with error: NoSolution` at
/// codegen/masm/src/lower/lowering.rs:109, constraints `[Copy, Copy]`, over a
/// SEVENTEEN-felt operand stack (six u64 operands and five u32 counts) — the
/// Copy-constrained count sits at index 10, outside the MASM window, so this
/// is the over-window mechanism and not the in-window arity-2 solver gap
/// (`rotl_window`/F2). CLASSIFIED F6 by trace: `edges to split = 1` and SIX
/// `erase unused reload` lines, the same stale-dominator-tree defect as
/// `zero_trip_overflow` — which needed twelve counts and a zero-trip-capable
/// loop for the same site. Panics at all four optimization levels. Bounded by
/// [`window_erased_guard`] (one cluster value fewer, no spills at all).
/// Compile-time — no inputs involved. Un-ignore when the transform recomputes
/// dominance after splitting edges.
#[test]
#[ignore = "compiler panic: 'failed to schedule operands: [%24, %82] for inst arith.rotl with \
            error: NoSolution' at codegen/masm/src/lower/lowering.rs:109 over a 17-felt stack — F6 \
            (edges to split = 1, six erased split reloads): five u64 values live across a \
            bottom-tested loop, no zero-trip loop and no count band; all four optimization levels, \
            compile-time, no inputs"]
fn overflow_cluster_min() {
    run_case("overflow_cluster_min", include_str!("../cases/case_overflow_cluster_min.rs"));
}
