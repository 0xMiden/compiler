//! Pairwise compositions of the campaign-10..13 boundary guards (campaign
//! 14, 2026-09-02): each case composes the SHAPES of two passing guards in
//! one function, kept just under the known panic thresholds, so that
//! interactions between passes (spills x lifting, scheduling x dispatch,
//! values x exits, lanes x wide arithmetic, calls x all of them) run
//! differentially. Every case passes; the ladder rungs above each guard
//! hit only known panic classes (recorded in the doc comments).

use super::super::harness::{run_case, run_case_with_inputs};

/// chain_window x sm16: six rotate counts shared between the code before a
/// sixteen-state machine (64 transition arms, four arm breaks, an early
/// return and a step-budget exit), the machine's arms (rotates of the u64
/// accumulator) and the code after it, so the CSE-merged count bands are
/// live across the jump-threaded nested loops and every dispatch arm. Six
/// is the boundary: eight counts hit the known arity-2 `NoSolution` (F2,
/// `rotl_window` class: 15-felt in-contract stack, Copy count at the bottom).
#[test]
fn chain_sm() {
    run_case("chain_sm", include_str!("../cases/case_chain_sm.rs"));
}

/// Pinned inputs for every exit tag of `chain_sm` (budget, four arm breaks,
/// the early return).
#[test]
fn chain_sm_edges() {
    run_case_with_inputs(
        "chain_sm_edges",
        include_str!("../cases/case_chain_sm.rs"),
        &[
            (0, 0),
            (0, 1),
            (37, 1),
            (74, 0),
            (111, 25),
            (222, 0),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// zero_trip_guard x exit_values: six rotate counts shared between the
/// code before a five-exit loop nest (zero-trip-capable outer `while i <
/// input2 % 97` with a bypass edge, bottom-test inner loop, a `match` in
/// the inner body), the `match` arms and the code after the nest. Six is
/// the boundary: seven hits the arity-2 F2 gap (15 felts) and eight the
/// known F6 over-full stack (20 felts: erased split-edge reloads).
#[test]
fn bands_exits() {
    run_case("bands_exits", include_str!("../cases/case_bands_exits.rs"));
}

/// Pinned inputs for every exit of `bands_exits`: the zero-trip header
/// exit, the labeled break with a value, the early return, the post-inner
/// break, plus one-trip shapes.
#[test]
fn bands_exits_edges() {
    run_case_with_inputs(
        "bands_exits_edges",
        include_str!("../cases/case_bands_exits.rs"),
        &[
            (0, 0),
            (37, 3),
            (0, 1),
            (111, 3),
            (1, 1),
            (0, 97),
            (5, 98),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// call_live x carried_sets: nine loop-carried u32 values with a full
/// rotation per trip plus three carried u64s, two pinned calls per trip
/// whose u64 arguments are reused after each call, a partial per-arm update
/// driven by a carried selector, and a `<< 32` / `>> 32` band shared by the
/// code before, inside and after the loop.
#[test]
fn calls_carried() {
    run_case("calls_carried", include_str!("../cases/case_calls_carried.rs"));
}

/// select_chain x switch_forms: six selects on one reused condition over
/// two multi-use u64s feed `br_table` selectors inside a loop (a dense
/// 16-arm match on a loop-carried selector seeded from a select, a holey
/// match with a hot default on a select-derived selector), with every
/// select result and both sources consumed after the loop.
#[test]
fn selects_switch() {
    run_case("selects_switch", include_str!("../cases/case_selects_switch.rs"));
}

/// pressure_u64_32 x nest: a six-level loop nest (levels 1 and 4
/// zero-trip-capable) whose innermost body is a 12-leaf right-leaning u64
/// tree (24 felts in one block) over the loop-carried value, with escapes
/// at three depths (early return, labeled break of level 1, `continue` of
/// level 3). Six levels stay under the F7 depth boundary (9 at O2, 7 at -Oz).
#[test]
fn tree_nest() {
    run_case("tree_nest", include_str!("../cases/case_tree_nest.rs"));
}

/// Pinned inputs for every exit tag of `tree_nest` (all-zero-trip, the
/// `continue`, the labeled break, the early return) and full trips.
#[test]
fn tree_nest_edges() {
    run_case_with_inputs(
        "tree_nest_edges",
        include_str!("../cases/case_tree_nest.rs"),
        &[(0, 0), (0, 64), (0, 68), (0, 188), (0xffff, 0xffff), (0xffff_ffff, 0xffff_ffff)],
    );
}

/// C12 value ladders x multi-exit loops: inside a five-exit loop nest every
/// exit is decided by a value-ladder result computed on the trip — i32
/// `checked_div` with divisors reaching 0 / -1 / MIN, `checked_shl` at
/// runtime counts across the width, the high limb of a `mul_wide_u`
/// product, an i16 sext chain — while a 4-limb u128 product folds into the
/// carried u64 on every trip.
#[test]
fn ladder_exits() {
    run_case("ladder_exits", include_str!("../cases/case_ladder_exits.rs"));
}

/// Pinned inputs for every exit of `ladder_exits`: the zero-trip header,
/// the `checked_div` None (divisor 0 on the third inner trip), the
/// `checked_shl` None, the post-inner break, plus the MIN / -1 row.
#[test]
fn ladder_exits_edges() {
    run_case_with_inputs(
        "ladder_exits_edges",
        include_str!("../cases/case_ladder_exits.rs"),
        &[(0, 0), (5, 0x8000_0000), (37, 1), (111, 9), (0x8000_0001, 0xffff_ffff), (1, 1)],
    );
}

/// C13 packed lanes x C12 wide arithmetic: a 35-byte packed record in a
/// runtime-indexed array (every field at all four byte offsets) whose
/// unaligned loads feed a 4-limb u128 product, dynamic-count 128-bit shifts,
/// an i128 `checked_div` with lane-driven 0 / -1 divisors, sext/trunc chains
/// and a carrying width tree, with the wide results stored back through the
/// neighbour's unaligned fields.
#[test]
fn lanes_wide() {
    run_case("lanes_wide", include_str!("../cases/case_lanes_wide.rs"));
}

/// C10 spills x C13 memory copies: ten pinned u64 values (twenty felts) live
/// across three runtime-length misaligned `memory.copy`s (static -> stack,
/// stack -> stack, disjoint in-buffer `copy_within`) and a runtime-length
/// `fill`, then consumed by a 12-leaf u64 tree over the copied words.
#[test]
fn spills_copies() {
    run_case("spills_copies", include_str!("../cases/case_spills_copies.rs"));
}

/// calls x everything: a zero-trip-capable four-state machine whose arms
/// call a 16-felt sret helper and store the u128 into a packed frame array
/// through a `&mut` helper, dispatch a fn pointer, run a runtime-length
/// `copy_within` through a `&mut` helper, or mix and `continue`, with four
/// u64 values live across the loop and a final fold helper; every arm body
/// is a helper so the composing function has no constant shift count (a
/// single crossing band here hits the known F6 defect, `frontier.rs:123`).
#[test]
fn calls_all() {
    run_case("calls_all", include_str!("../cases/case_calls_all.rs"));
}

/// Pinned inputs for every exit of `calls_all`: zero-trip (`input2 % 17 ==
/// 0`), the copy-arm break, the call-decided return, and each starting
/// state with a full trip count.
#[test]
fn calls_all_edges() {
    run_case_with_inputs(
        "calls_all_edges",
        include_str!("../cases/case_calls_all.rs"),
        &[(0, 0), (0, 74), (0, 33), (1, 16), (2, 16), (3, 16), (0xffff_ffff, 0xffff_ffff)],
    );
}

/// ret_area x exit_values: wide by-value helper results decide AND carry the
/// exits of a zero-trip-capable loop nest — a `(u64, u64)`-shaped record,
/// an `Option<u128>` and a `Result<u64, u32>` through return areas; the u128
/// payloads leave the nest through a labeled `break 'outer <u128>` from the
/// inner loop, a value-carrying break after it and the outer header exit,
/// so the lifted exit dispatch threads four-felt result columns, while an
/// `Err` arm may `return` early.
#[test]
fn sret_exits() {
    run_case("sret_exits", include_str!("../cases/case_sret_exits.rs"));
}

/// Pinned inputs for every exit of `sret_exits`: the header exit (zero
/// trips), the `None` labeled break, the `Err` return, the post-inner break,
/// plus one-trip shapes.
#[test]
fn sret_exits_edges() {
    run_case_with_inputs(
        "sret_exits_edges",
        include_str!("../cases/case_sret_exits.rs"),
        &[
            (0, 0),
            (0, 4),
            (0, 15),
            (235_806_222, 74),
            (1, 1),
            (0, 61),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// big_frame x mut_arrays x dispatch: a 2 KiB `[u32; 512]` frame escapes as
/// a runtime-bounded `&mut [u32]` sub-slice (fat pointer) through fn-pointer
/// dispatch into helpers that fill it, copy a runtime-length prefix onto its
/// disjoint suffix (an overlapping `copy_within` is the known `mem_overlap`
/// trap) and xor-fold it, with a u64 carried across the dispatches in a
/// loop and the result read back at runtime indexes.
#[test]
fn frame_dispatch() {
    run_case("frame_dispatch", include_str!("../cases/case_frame_dispatch.rs"));
}

/// shortcircuit x calls: `&&` / `||` lattices whose operands are
/// side-effecting helper calls (direct and fn-pointer dispatched) writing an
/// evaluation log through a `&mut [u32; 8]`, mixed with plain compares,
/// negations and a `match` on two lattice results inside a loop with a
/// `continue`; the log order and the booleans are folded into the result.
#[test]
fn shortcircuit_calls() {
    run_case("shortcircuit_calls", include_str!("../cases/case_shortcircuit_calls.rs"));
}

/// chain_window x calls: six masked shift counts shared by the code before
/// a pinned direct 7-u64 call, passed to a 6-u32 + u64 helper, used again
/// around a fn-pointer dispatch and after it — CSE-merged count bands live
/// across direct and indirect call boundaries. The dispatch takes plain
/// locals only: computing two of its arguments in place is the
/// `indirect_spill_args` panic (tests/calls.rs).
#[test]
fn bands_calls() {
    run_case("bands_calls", include_str!("../cases/case_bands_calls.rs"));
}

/// ext_chains x calls: i8 / i16 / u8 / u16 / bool parameters and results
/// crossing `#[inline(never)]` call boundaries (caller-side sign / zero
/// extension, truncation on return), chained through a loop whose carried
/// values are narrow, with MIN / -1 / MAX parameters reached from the
/// inputs, `as` widenings of every result and a narrow-typed fn-pointer
/// dispatch.
#[test]
fn narrow_sigs() {
    run_case("narrow_sigs", include_str!("../cases/case_narrow_sigs.rs"));
}

/// carried_sets x wide x calls: three u128 values (twelve felts) carried by
/// a bottom-test loop across a pinned call per trip that takes two of them
/// by value and returns a u128 through a return area, a full rotation of
/// the carried set every trip and a call result deciding the `break`.
#[test]
fn carried_wide() {
    run_case("carried_wide", include_str!("../cases/case_carried_wide.rs"));
}

/// Pinned inputs for `carried_wide`: one trip, the call-decided break and
/// the full trip count.
#[test]
fn carried_wide_edges() {
    run_case_with_inputs(
        "carried_wide_edges",
        include_str!("../cases/case_carried_wide.rs"),
        &[(0, 0), (0, 7), (8, 3), (0xffff_ffff, 0xffff_ffff)],
    );
}

/// nest x calls: a five-level loop nest (levels 1, 3 and 5 zero-trip-
/// capable) with a pinned helper call at EVERY level whose result decides
/// that level's exit (`break`, labeled `break 'l1` from level 2, a
/// same-level `continue` at level 3, early `return` from level 4, `break`
/// at level 5), so the lifted exit dispatch threads call results through
/// five levels of region result columns with the carried u64 crossing every
/// call. A `continue` of an OUTER level from an inner loop that contains a
/// call is the `nest_continue` panic below.
#[test]
fn nest_calls() {
    run_case("nest_calls", include_str!("../cases/case_nest_calls.rs"));
}

/// Pinned inputs for every exit tag of `nest_calls` (all-zero-trip, the
/// level-1 break, the labeled break, the level-3 continue, the level-4
/// return, the level-5 break) and full trips.
#[test]
fn nest_calls_edges() {
    run_case_with_inputs(
        "nest_calls_edges",
        include_str!("../cases/case_nest_calls.rs"),
        &[
            (0, 0),
            (0, 21),
            (0, 4),
            (16, 4),
            (16, 8),
            (269_492_240, 2),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, campaign 14 attempt 2,
/// 2026-09-03, NEW class): two nested `for` loops, one `#[inline(never)]`
/// call in the inner loop and a `continue 'outer` from the inner loop.
/// Building it panics in the post-lift Canonicalizer with
/// `AliasingViolationError { kind: Mutable, location: hir/src/ir/operation.rs:877 }`
/// at hir/src/patterns/rewriter.rs:335 (`Rewriter::move_block_before` →
/// `Block::insert_before` → `Region::borrow_mut`). Mechanism: the lifted
/// outer `scf.while` carries a loop-invariant before-block argument, so
/// `RemoveLoopInvariantArgsFromBeforeBlock`
/// (dialects/scf/src/canonicalization/remove_loop_invariant_args_from_before_block.rs)
/// matches for the first time in this corpus and, at the end of its
/// rewrite, calls `rewriter.inline_region_before(after_region,
/// new_while.borrow().after().as_region_ref())`: the temporaries of that
/// argument expression — the `EntityRef` of the new while op and the
/// `EntityRef<Region>` created by `Operation::region` (operation.rs:877) —
/// stay borrowed for the whole call, and `inline_region_before` then needs
/// the same region mutably (with the greedy driver's listener through
/// `move_block_before` → `Block::insert_before`; without one through
/// `ip.borrow_mut()` directly). The pattern therefore cannot complete for
/// ANY shape it matches. Panic-only (nothing is emitted). Bounded by
/// `nest_continue_inline` (the helper inlined: LLVM restructures the nest
/// and no invariant iter arg is produced — passes), `nest_calls` (calls at
/// every level of a five-level nest with breaks, a return and a same-level
/// `continue` — passes) and the corpus' labeled continues without a call in
/// the inner loop (`cf_shapes`, `nest8`, `wide_exits`, `tree_nest`).
/// Compile-time — no inputs involved. Suggested fix: bind the target region
/// to a local (dropping the op borrow) before calling
/// `inline_region_before`. Un-ignore when this case compiles.
#[test]
#[ignore = "compiler panic: AliasingViolationError { kind: Mutable, location: \
            hir/src/ir/operation.rs:877 } at hir/src/patterns/rewriter.rs:335 — \
            RemoveLoopInvariantArgsFromBeforeBlock inlines the after region while its own borrow \
            of the new while's region is still alive (compile-time, no inputs involved)"]
fn nest_continue() {
    run_case("nest_continue", include_str!("../cases/case_nest_continue.rs"));
}

/// Inlined twin of `nest_continue`: the same two `for` loops and `continue
/// 'outer` with the helper `#[inline(always)]` — LLVM restructures the nest,
/// the lifted `scf.while` has no loop-invariant iter arg, and the case
/// compiles and passes.
#[test]
fn nest_continue_inline() {
    run_case("nest_continue_inline", include_str!("../cases/case_nest_continue_inline.rs"));
}

/// packed_fields x ret_area: helpers return `repr(C, packed)` records (a
/// u128 at byte offset 1, a u64 at offset 3) BY VALUE straight into
/// runtime-indexed elements of stack arrays of such records, so the
/// return-area pointer is a computed unaligned address; the elements are
/// read back at other runtime indexes and through a slice helper.
#[test]
fn sret_indexed() {
    run_case("sret_indexed", include_str!("../cases/case_sret_indexed.rs"));
}

/// switch_forms x calls: a dense 24-arm `match` (one `br_table`) on a
/// loop-carried selector inside a loop, whose arms call helpers of every
/// arity shape — zero-arg, zero-result, a 16-felt eight-u64 signature, a
/// `(u64, u64)` return area, a fn-pointer dispatch — or `continue` /
/// `break` / `return`, with two u64 and the selector carried across the
/// loop and a holey `match` with a hot default re-selecting the next arm
/// from a call result.
#[test]
fn switch_calls() {
    run_case("switch_calls", include_str!("../cases/case_switch_calls.rs"));
}

/// Pinned inputs for `switch_calls`: the `break` arm, the `continue` arm,
/// the `return` arm, zero and one trips, every starting selector class.
#[test]
fn switch_calls_edges() {
    run_case_with_inputs(
        "switch_calls_edges",
        include_str!("../cases/case_switch_calls.rs"),
        &[(0, 1), (1, 1), (3, 12), (0, 0), (2, 0), (23, 12), (0xffff_ffff, 0xffff_ffff)],
    );
}
