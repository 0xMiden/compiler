//! Trap edges crossing SPILL FREIGHT: cases that deliberately panic on some
//! inputs while a u64 cluster and CSE-merged rotate count bands are live
//! across the trapping edge, compared with `run_case_traps` so both targets
//! must agree per input on value-or-trap.
//!
//! `traps.rs` (campaign 29) established trap parity on spill-free shapes.
//! This module crosses that oracle with the machinery of campaigns 18/20/21:
//! the spill transform splits critical edges, places reloads on them and
//! erases the ones the stale dominator tree cannot see, and cfg-to-scf lifts
//! `unreachable`-terminated blocks as return-like exits merged through
//! `ReturnLikeOpKey` (operands compared BY TYPE, so every `ub.unreachable` is
//! equivalent to every other). Each case therefore asks two questions at once:
//! is the VALUE on the non-trapping rows still right, and is the trap-or-value
//! DECISION right on the trapping rows.
//!
//! Four facts hold across the whole module and are why these are guards rather
//! than findings (all measured with
//! `MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'` on the case file
//! built exactly as the harness builds it).
//!
//! First, **the guard KIND is invisible to the spill analysis.** The six W1
//! cases below differ only in what traps — an index, a `get().unwrap()`, a
//! zero divisor, a `checked_add().unwrap()`, an `assert!` and an
//! `unreachable!()` — and all six produce exactly 45 spills, 56 reloads, 3
//! split control-flow edges, 20 erased split-edge reloads and 36 reloads
//! lowered to spill-slot loads. Under `-Cpanic=immediate-abort` every one of
//! them is the same wasm `unreachable`.
//!
//! Second, **LLVM merges every trapping edge of a function into ONE wasm
//! `unreachable`.** Every case here — including `both_kinds`, which has an
//! explicit `core::arch::wasm32::unreachable()` AND a Rust panic edge — has
//! exactly one `ub.unreachable` in `entrypoint` before the lift and exactly
//! one after it, at `--optimize=basic`, the default level and
//! `--optimize=size-min` alike. So `combine_exit` never has two of them to
//! combine on a plain-Rust guest, and the `ReturnLikeOpKey` type-comparison is
//! exercised only against the function's single `builtin.ret`.
//!
//! Third, **what the lift does with the trap edge is turn it into an exit
//! column.** After `lift-control-flow` the trapping loop yields one extra u32
//! result which a top-level `cf.switch` dispatches to the block holding the
//! `ub.unreachable`; the no-trap control of the same shape has three fewer
//! blocks, three fewer `scf` ops and no `cf.switch` at all.
//!
//! Fourth, **a trap edge does not move the spill numbers of the interaction
//! shapes.** `dispatch_arm`, `select_arm`, `scan_break` and `second_loop`
//! reproduce their `interact.rs` originals' spill/reload/split/erase counts
//! exactly. It DOES move the canonicalization pattern counts — see the case
//! comments — and at one guard position it moves a compile-time boundary,
//! which is what `guard_above` and `guard_above_masked` pin.

use super::super::harness::{
    run_case_traps, run_case_traps_with_flags, run_case_traps_with_inputs,
};

/// Trap edge in the body of a freight loop, guard kind ARRAY INDEX: a runtime
/// index into a 31-byte `static` taken from five bits of the loop-carried
/// accumulator, at the end of the body, with an eight-u64 cluster and eight
/// count bands live across it.
///
/// Freight: 45 spills, 56 reloads, 3 split edges, 20 erased split reloads, 36
/// reloads lowered to spill-slot loads, no unused phi. Lift: 1 `ub.unreachable`
/// before and 1 after; 11 blocks / 11 `cf` ops before, 6 blocks / 18 `scf` + 2
/// `cf` ops after, the second `cf` being the exit `cf.switch` that dispatches
/// the loop's trap column to the unreachable block. The no-trap control of the
/// same shape (`A31[g & 15]`) has 46 spills, 3 blocks and 15 `scf` ops after
/// the lift and no `cf.switch`. Verdict: agrees on every row.
#[test]
fn body_index() {
    run_case_traps("ts_body_index", include_str!("../cases/case_ts_body_index.rs"));
}

/// Pinned straddling grid for [`body_index`]: four rows that return and three
/// that trap — (3, 987654321), (2, 65535) and (12345, 67890) drive the
/// accumulator's five-bit slice to 31 on some trip.
#[test]
fn body_index_edges() {
    run_case_traps_with_inputs(
        "ts_body_index_edges",
        include_str!("../cases/case_ts_body_index.rs"),
        &[
            (0, 0),
            (7, 3),
            (3, 987654321),
            (2, 65535),
            (15, 15),
            (12345, 67890),
            (u32::MAX, u32::MAX),
        ],
    );
}

/// The same freight and the same guard position as [`body_index`] with the
/// panic coming from `Option::unwrap` on a slice `get` instead of from an
/// index expression. Identical freight numbers (45/56/3/20/36) and an
/// identical native value/trap map to `body_index`'s, which is the sharpest
/// statement of "the guard kind is invisible below the frontend".
#[test]
fn body_get() {
    run_case_traps("ts_body_get", include_str!("../cases/case_ts_body_get.rs"));
}

/// Pinned straddling grid for [`body_get`] — the same rows as
/// [`body_index_edges`], because the two cases compute the same function.
#[test]
fn body_get_edges() {
    run_case_traps_with_inputs(
        "ts_body_get_edges",
        include_str!("../cases/case_ts_body_get.rs"),
        &[
            (0, 0),
            (7, 3),
            (3, 987654321),
            (2, 65535),
            (15, 15),
            (12345, 67890),
            (u32::MAX, u32::MAX),
        ],
    );
}

/// Guard kind DIVISION BY A BAND: the trapping edge is the zero-divisor test
/// in front of a `u32 /` whose divisor is a five-bit mask of the loop-carried
/// accumulator. Freight 45/56/3/20/36, lift 1 -> 1 `ub.unreachable` — the same
/// numbers as [`body_index`] even though the guard also drags a division into
/// the loop body.
#[test]
fn body_div() {
    run_case_traps("ts_body_div", include_str!("../cases/case_ts_body_div.rs"));
}

/// Pinned straddling grid for [`body_div`]: (15, 15), (2, 255) and (0, 65535)
/// reach a zero divisor, the other four rows return.
#[test]
fn body_div_edges() {
    run_case_traps_with_inputs(
        "ts_body_div_edges",
        include_str!("../cases/case_ts_body_div.rs"),
        &[(0, 0), (7, 3), (15, 15), (2, 255), (3, 987654321), (0, 65535), (9, 17)],
    );
}

/// Guard kind `checked_add().unwrap()`: with overflow checks off in the
/// release guest this is the only form of addition that can panic, and
/// `0xffff_ffe1 + d` overflows on exactly one of the thirty-two values of the
/// accumulator's five-bit slice. Freight 45/56/3/20/36.
#[test]
fn body_checked() {
    run_case_traps("ts_body_checked", include_str!("../cases/case_ts_body_checked.rs"));
}

/// Pinned straddling grid for [`body_checked`]: (15, 15), (2, 255) and
/// (0, 12345) overflow, the rest return.
#[test]
fn body_checked_edges() {
    run_case_traps_with_inputs(
        "ts_body_checked_edges",
        include_str!("../cases/case_ts_body_checked.rs"),
        &[(0, 0), (7, 3), (15, 15), (2, 255), (0, 65535), (5, 4095), (0, 12345)],
    );
}

/// Guard kind `assert!`: the trapping edge written by hand rather than taken
/// from `core`, on the same five-bit slice, with the slice folded back into
/// the accumulator afterwards so the guard's operand is live past the guard.
/// Freight 45/56/3/20/36.
#[test]
fn body_assert() {
    run_case_traps("ts_body_assert", include_str!("../cases/case_ts_body_assert.rs"));
}

/// Pinned straddling grid for [`body_assert`]: (15, 15), (2, 255), (0, 65535)
/// and (u32::MAX, u32::MAX) fail the assertion, three rows return.
#[test]
fn body_assert_edges() {
    run_case_traps_with_inputs(
        "ts_body_assert_edges",
        include_str!("../cases/case_ts_body_assert.rs"),
        &[(0, 0), (7, 3), (15, 15), (2, 255), (0, 65535), (5, 4095), (u32::MAX, u32::MAX)],
    );
}

/// Guard kind `unreachable!()` behind a runtime condition — the barest
/// trapping edge the language offers, and the one every other guard kind in
/// this module decays to under `-Cpanic=immediate-abort`. Freight
/// 45/56/3/20/36 and a native value/trap map identical to [`body_assert`]'s.
#[test]
fn body_unreach() {
    run_case_traps("ts_body_unreach", include_str!("../cases/case_ts_body_unreach.rs"));
}

/// Pinned straddling grid for [`body_unreach`] — the same rows as
/// [`body_assert_edges`], because the two cases compute the same function.
#[test]
fn body_unreach_edges() {
    run_case_traps_with_inputs(
        "ts_body_unreach_edges",
        include_str!("../cases/case_ts_body_unreach.rs"),
        &[(0, 0), (7, 3), (15, 15), (2, 255), (0, 65535), (5, 4095), (u32::MAX, u32::MAX)],
    );
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-09-17) that the TRAPPING EDGE
/// ALONE causes: `invalid operand stack index (11): requires access to more
/// than 16 elements, which is not supported in Miden` at
/// codegen/masm/src/emit/mod.rs:623.
///
/// The case is `case_ts_body_index.rs` with the guard moved one statement up,
/// so it sits immediately above the wide expression that consumes the whole
/// eight-u64 cluster instead of below it. [`guard_above_masked`] is the same
/// file with the index masked into range — the same load, the same mask, the
/// same live cluster, no trapping edge — and it COMPILES and passes. So the
/// boundary is moved by the edge, not by the guard's work or its position.
///
/// Classification by trace: the pre-lift spill run reports `edges to split =
/// 3` and 20 `erase unused reload` lines (the F6 markers), and 49 `additional
/// spills required` (an F17 marker). Neither marker discriminates here — the
/// COMPILING control carries all of them too, with 48 `additional spills
/// required` and the identical 3/20 split-and-erase counts. The only trace
/// difference between the two is the spill/reload balance: 48 spills / 54
/// reloads with the trapping edge against 46 / 56 without it, i.e. the trap
/// edge converts two reloads into two spills. Compile-time — no inputs
/// involved. Un-ignore when this case compiles.
#[test]
#[ignore = "compiler panic: 'invalid operand stack index (11): requires access to more than 16 \
            elements' at codegen/masm/src/emit/mod.rs:623 with the trapping edge placed above the \
            cluster-consuming expression; the same file with the index masked into range \
            (guard_above_masked) compiles (compile-time, no inputs involved)"]
fn guard_above() {
    run_case_traps("ts_guard_above", include_str!("../cases/case_ts_guard_above.rs"));
}

/// The no-trap control for [`guard_above`]: byte-identical except `A31[g & 15]`
/// for `A31[g]`. 46 spills, 56 reloads, 3 split edges, 20 erased split
/// reloads; compiles at every level and never traps on either target.
#[test]
fn guard_above_masked() {
    run_case_traps("ts_guard_above_masked", include_str!("../cases/case_ts_guard_above_masked.rs"));
}

/// Pinned grid for [`guard_above_masked`], all of which must return a value:
/// the rows on which its trapping twin would trap, plus the neighbours.
#[test]
fn guard_above_masked_edges() {
    run_case_traps_with_inputs(
        "ts_guard_above_masked_edges",
        include_str!("../cases/case_ts_guard_above_masked.rs"),
        &[
            (0, 0),
            (7, 3),
            (15, 15),
            (2, 255),
            (0, 65535),
            (12345, 67890),
            (u32::MAX, u32::MAX),
        ],
    );
}

/// Trap edge at the LOOP EXIT: the loop body is guard-free and the two guards
/// — an `assert!` and a table index — fire on the value the loop produced, so
/// the trapping edge is a successor of the loop's exit block rather than of a
/// block inside the region.
///
/// Freight: 47 spills (two more than the in-body cases), 56 reloads, 3 split
/// edges, 20 erased split reloads. Lift: 1 `ub.unreachable` before and after,
/// 14 blocks -> 5, 21 `scf` ops, 2 `cf` ops. Verdict: agrees on every row.
#[test]
fn exit_guard() {
    run_case_traps("ts_exit_guard", include_str!("../cases/case_ts_exit_guard.rs"));
}

/// Pinned straddling grid for [`exit_guard`]: (5, 4095), (0, 31) and
/// (0, u32::MAX) leave through one of the two post-loop guards.
#[test]
fn exit_guard_edges() {
    run_case_traps_with_inputs(
        "ts_exit_guard_edges",
        include_str!("../cases/case_ts_exit_guard.rs"),
        &[(0, 0), (7, 3), (5, 4095), (0, 31), (15, 15), (0, u32::MAX), (2, 255)],
    );
}

/// Trap edge in the SECOND of two sequential loops sharing one cluster — the
/// campaign-20 shape whose freight-only version reaches the `frontier.rs:123`
/// unwrap through a three-predecessor join.
///
/// Freight: 57 spills, 84 reloads, six split edges, 32 erased split reloads —
/// EXACTLY `interact::cascade_spill`'s numbers, which this case is
/// `cascade_spill` plus one guard at the end of the second loop's body. The
/// trapping edge therefore costs the spill analysis nothing at all here. Lift:
/// 1 `ub.unreachable` before and after, 20 blocks -> 9, 33 `scf` + 3 `cf`.
#[test]
fn second_loop() {
    run_case_traps("ts_second_loop", include_str!("../cases/case_ts_second_loop.rs"));
}

/// Pinned straddling grid for [`second_loop`]: (0, 128), (5, 4095),
/// (12345, 67890) and (u32::MAX, u32::MAX) trap in the second loop; the other
/// three return. `input2`'s bits also decide which trips of which loop take
/// the `continue` edge, so the value rows cover both loops' column removal.
#[test]
fn second_loop_edges() {
    run_case_traps_with_inputs(
        "ts_second_loop_edges",
        include_str!("../cases/case_ts_second_loop.rs"),
        &[
            (0, 0),
            (7, 3),
            (0, 128),
            (5, 4095),
            (15, 15),
            (12345, 67890),
            (u32::MAX, u32::MAX),
        ],
    );
}

/// Trap edge inside the empty-`continue` ARM of the column-removal cascade —
/// `interact::cascade_spill` with an `assert!` before the first loop's
/// `continue`.
///
/// This is the one interaction shape whose PATTERN COUNTS the trapping edge
/// moves, and it moves them a lot (`pattern-rewrite-driver` trace, default
/// level, trap twin vs original): `while-unused-result` 1 vs 2,
/// `index-switch-remove-unused-results` 1 vs 2 — the guarded loop stops
/// producing the cascade entirely, the unguarded one still fires it once —
/// while `simplify-passthrough-cond-br` goes 10 vs 0, `split-critical-edges`
/// 11 vs 2, `while-remove-unused-args` 3 vs 2, `simplify-cond-br-like-switch`
/// 1 vs 0 and `convert-trivial-if-to-select` 3 vs 4. Freight: 57 spills, 86
/// reloads, seven split edges, 34 erased split reloads (the original's 57 /
/// 84 / 6 / 32). It compiles and agrees with native everywhere at the default
/// level, at `--optimize=size-min`, at `--optimize=basic` and without guest
/// DWARF.
///
/// CONFIGURATION-DEPENDENT COMPILE-TIME COMPILER PANIC (safe Rust,
/// 2026-09-17), caused by the trapping edge alone: at `--optimize=max` this
/// case panics with "called `Option::unwrap()` on a `None` value" at
/// hir/src/ir/dominance/frontier.rs:123, the F6 site, while
/// `interact::cascade_spill` — the same file WITHOUT the `assert!` in the
/// `continue` arm — compiles at `--optimize=max` with 57 spills / 84 reloads /
/// 6 split edges / 32 erased split reloads. The trap twin dies inside the
/// FIRST spill transform (one `edges to split = 6` line, then the unwrap; no
/// `erase unused reload` and no `convert reload to load` line is ever
/// reached). Pinned in-repo by [`cascade_cont_max`].
#[test]
fn cascade_cont() {
    run_case_traps("ts_cascade_cont", include_str!("../cases/case_ts_cascade_cont.rs"));
}

/// [`cascade_cont`] pinned at `--optimize=max` through `run_case_traps_with_flags`
/// (the flags+traps entry point added for this finding, 2026-09-17): the
/// compile-time panic described above, kept in-repo. Un-ignore when the case
/// compiles at that level.
#[test]
#[ignore = "compiler panic at --optimize=max: 'called `Option::unwrap()` on a `None` value' at \
            hir/src/ir/dominance/frontier.rs:123 (the F6 site) inside the first spill transform, \
            caused by the trapping edge alone — interact::cascade_spill, the same file without the \
            assert! in the continue arm, compiles at --optimize=max (director re-ran both \
            2026-09-17)"]
fn cascade_cont_max() {
    run_case_traps_with_flags(
        "ts_cascade_cont_max",
        include_str!("../cases/case_ts_cascade_cont.rs"),
        &["--optimize=max"],
    );
}

/// Pinned straddling grid for [`cascade_cont`]: (0, 8), (0, 255), (2, 9) and
/// (7, 17) take the `continue` arm on a trip where the assertion fails; the
/// other three rows return.
#[test]
fn cascade_cont_edges() {
    run_case_traps_with_inputs(
        "ts_cascade_cont_edges",
        include_str!("../cases/case_ts_cascade_cont.rs"),
        &[(0, 0), (7, 3), (0, 8), (0, 255), (2, 9), (15, 15), (7, 17)],
    );
}

/// Trap edge UNDER A HELPER: the bounds check that panics belongs to an
/// `#[inline(never)]` callee, so `entrypoint` itself has ZERO `ub.unreachable`
/// before and after the lift and the callee has the one — the only case in
/// this module where the freight region and the trapping edge are in different
/// functions. The call boundary forces a spill of everything live: 46 spills,
/// 56 reloads, 3 split edges, 20 erased split reloads. Same native value/trap
/// map as [`body_index`].
#[test]
fn helper_arg() {
    run_case_traps("ts_helper_arg", include_str!("../cases/case_ts_helper_arg.rs"));
}

/// Pinned straddling grid for [`helper_arg`] — the same rows as
/// [`body_index_edges`], with the trap now taken one frame down.
#[test]
fn helper_arg_edges() {
    run_case_traps_with_inputs(
        "ts_helper_arg_edges",
        include_str!("../cases/case_ts_helper_arg.rs"),
        &[
            (0, 0),
            (7, 3),
            (3, 987654321),
            (2, 65535),
            (15, 15),
            (12345, 67890),
            (u32::MAX, u32::MAX),
        ],
    );
}

/// The mirror image of [`helper_arg`]: a total `#[inline(never)]` helper whose
/// RETURN VALUE is the index that decides whether the caller traps. The
/// `ub.unreachable` is back in `entrypoint` (1 before and after the lift) and
/// the callee has none. Freight 45/56/3/20/36.
#[test]
fn helper_ret() {
    run_case_traps("ts_helper_ret", include_str!("../cases/case_ts_helper_ret.rs"));
}

/// Pinned straddling grid for [`helper_ret`]: (15, 15), (5, 4095), (2, 15) and
/// (u32::MAX, u32::MAX) make the helper return 31.
#[test]
fn helper_ret_edges() {
    run_case_traps_with_inputs(
        "ts_helper_ret_edges",
        include_str!("../cases/case_ts_helper_ret.rs"),
        &[(0, 0), (7, 3), (15, 15), (5, 4095), (2, 255), (2, 15), (u32::MAX, u32::MAX)],
    );
}

/// Trap edge in a `match` ARM of the dispatch shape — `interact::dispatch_spill`
/// with an `assert!` at the head of arm 15.
///
/// Freight: 20 spills, 20 reloads, one split edge, four erased split reloads —
/// exactly the original's numbers. The pattern the shape is about still fires:
/// `simplify-switch-fallback-overlap` 1, as without the trap. What the trapping
/// arm adds is `convert-trivial-if-to-select` 2 vs 1,
/// `simplify-cond-br-like-switch` 1 vs 0 and `while-remove-unused-args` 1 vs 0.
#[test]
fn dispatch_arm() {
    run_case_traps("ts_dispatch_arm", include_str!("../cases/case_ts_dispatch_arm.rs"));
}

/// Pinned straddling grid for [`dispatch_arm`]: (7, 7), (9, 8), (3, 129) and
/// (u32::MAX, u32::MAX) enter arm 15 with the assertion false; the other three
/// rows walk the dispatch without trapping.
#[test]
fn dispatch_arm_edges() {
    run_case_traps_with_inputs(
        "ts_dispatch_arm_edges",
        include_str!("../cases/case_ts_dispatch_arm.rs"),
        &[(0, 0), (7, 3), (7, 7), (9, 8), (15, 15), (3, 129), (u32::MAX, u32::MAX)],
    );
}

/// Trap edge in one ARM of the if-to-select diamond — `interact::select_spill`
/// with an `assert!` at the head of the heavy then-arm.
///
/// `ConvertTrivialIfToSelect` STILL FIRES with an `unreachable` arm in the
/// diamond, and fires MORE: 2 rewrites against the original's 1, plus a
/// `simplify-cond-br-like-switch` and a `while-remove-unused-args` that the
/// original does not fire at all. Freight: 10 spills, 10 reloads, one split
/// edge, two erased split reloads — the original's numbers exactly.
#[test]
fn select_arm() {
    run_case_traps("ts_select_arm", include_str!("../cases/case_ts_select_arm.rs"));
}

/// Pinned straddling grid for [`select_arm`]: (2, 255), (2, 127) and
/// (3, 987654321) take the then-arm with the assertion false; the other four
/// pin all-else, all-then and two mixed direction sequences that return.
#[test]
fn select_arm_edges() {
    run_case_traps_with_inputs(
        "ts_select_arm_edges",
        include_str!("../cases/case_ts_select_arm.rs"),
        &[(0, 0), (7, 3), (2, 255), (2, 127), (15, 15), (3, 987654321), (0, 65535)],
    );
}

/// Trap edge in the BREAK PREDICATE of a scan-loop chain —
/// `interact::scan_spill` with the second loop's break test reading a 14-byte
/// `static` at an accumulator-derived index.
///
/// Freight: 18 spills, 18 reloads, one split edge, four erased split reloads —
/// the original's numbers exactly. The pattern the shape is about loses one
/// firing: `while-remove-unused-args` 3 against 4, i.e. the guarded loop stops
/// producing it while the other three still do; `split-critical-edges` 6 vs 8
/// and `convert-trivial-if-to-select` 3 vs 4 follow.
#[test]
fn scan_break() {
    run_case_traps("ts_scan_break", include_str!("../cases/case_ts_scan_break.rs"));
}

/// Pinned straddling grid for [`scan_break`]: (0, 2), (2, 255) and (0, 129)
/// index past the table while evaluating the second loop's break test; the
/// other four rows run the chain to the end.
#[test]
fn scan_break_edges() {
    run_case_traps_with_inputs(
        "ts_scan_break_edges",
        include_str!("../cases/case_ts_scan_break.rs"),
        &[(0, 0), (7, 3), (0, 2), (2, 255), (15, 15), (0, 129), (9, 17)],
    );
}

/// BOTH trap kinds in one freight loop: an explicit `core::arch::wasm32::
/// unreachable()` (host: `abort()`) and a Rust panic edge, on two different
/// slices of the loop-carried accumulator, with the cluster crossing both.
///
/// The point of the case is what the guest does NOT show: the wasm has one
/// `unreachable` in `entrypoint`, not two, so LLVM has already merged the two
/// source-level trap kinds into a single block and `combine_exit` has nothing
/// to combine. Verified at `--optimize=basic`, the default level and
/// `--optimize=size-min`: 1 `ub.unreachable` before the lift and 1 after, at
/// all three. Freight 45/56/3/20/36, the same as the single-guard cases.
#[test]
fn both_kinds() {
    run_case_traps("ts_both_kinds", include_str!("../cases/case_ts_both_kinds.rs"));
}

/// Pinned straddling grid for [`both_kinds`]: (0, 8) and (5, 4095) leave
/// through one of the two edges, (31, 63) and (u32::MAX, u32::MAX) through the
/// other kind, and three rows return.
#[test]
fn both_kinds_edges() {
    run_case_traps_with_inputs(
        "ts_both_kinds_edges",
        include_str!("../cases/case_ts_both_kinds.rs"),
        &[(0, 0), (7, 3), (0, 8), (5, 4095), (15, 15), (31, 63), (u32::MAX, u32::MAX)],
    );
}
