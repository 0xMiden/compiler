//! Pairwise compositions of the campaign-10..13 boundary guards (campaign
//! 14, 2026-09-02): each case composes the SHAPES of two passing guards in
//! one function, kept just under the known panic thresholds, so that
//! interactions between passes (spills x lifting, scheduling x dispatch,
//! values x exits, lanes x wide arithmetic, calls x all of them) run
//! differentially. Every case passes; the ladder rungs above each guard
//! hit only known panic classes (recorded in the doc comments).

use super::super::harness::{run_case, run_case_with_flags, run_case_with_inputs};

/// chain_window x sm16: six rotate counts shared between the code before a
/// sixteen-state machine (64 transition arms, four arm breaks, an early
/// return and a step-budget exit), the machine's arms (rotates of the u64
/// accumulator) and the code after it, so the CSE-merged count bands are
/// live across the jump-threaded nested loops and every dispatch arm. Six
/// is the boundary: eight counts hit the known arity-2 `NoSolution` (F2,
/// `rotl_window` class: 15-felt in-contract stack, Copy count at the bottom).
/// Configuration note (campaign 16): at `--optimize=size-min` six counts
/// already hit the F2 gap (the bands stay un-hoisted, `spill_loop_mix_oz`
/// class), and WITHOUT guest DWARF the case hits the F12 aliasing panic
/// (`nest_continue` class) although it has no labeled continue: Local2Reg
/// promotion, which DWARF blocks, creates the loop-invariant before-block
/// arguments the pattern matches — pinned by [`chain_sm_nodwarf`] below.
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

/// The release-build twin of [`chain_sm`]: the SAME source with the guest's
/// debug-info level pinned to 0 through the harness pseudo-flag, so the
/// finding reproduces without any environment setup.
///
/// Local2Reg promotes a slot only when the local has exactly one store and
/// one load in one block with no branch/region/call op between them, AND
/// `convert_debug_references_for_local` succeeds; under full DWARF rustc's
/// two-op `[WasmLocal(N), StackValue]` declares make that last check fail, so
/// the stores survive. With `debug = 0` there are no declares, the promotions
/// go through, and cfg-to-scf ends up with a payload column that still
/// carries its `ub.poison` initializer at the back edge — the shape
/// [`invariant_args_min`] documents. The pattern then matches and its rewrite
/// aborts.
///
/// Campaign 24 measured the reach: the panic is identical at guest `debug = 0`
/// and `debug = 1` (line tables carry no variable DIEs either), and appears
/// at 16 and at 256 input pairs alike — it is compile-time, so no input is
/// involved. Only `debug = 2` masks it. Un-ignore when the rewriter stops
/// taking a mutable borrow of an operation it is already borrowing (same fix
/// as `invariant_args_min`).
#[test]
#[ignore = "compiler panic WITHOUT full guest DWARF (guest debug 0 and 1, i.e. an ordinary release \
            build): 'AliasingViolationError { kind: Mutable, location: hir/src/ir/operation.rs:877 \
            }' at hir/src/patterns/rewriter.rs:335 while matching \
            'remove-loop-invariant-args-from-before-block' — F12 class; compile-time, no inputs \
            involved"]
fn chain_sm_nodwarf() {
    run_case_with_flags(
        "chain_sm_nodwarf",
        include_str!("../cases/case_chain_sm.rs"),
        &["--guest-debug=0"],
    );
}

/// zero_trip_guard x exit_values: six rotate counts shared between the
/// code before a five-exit loop nest (zero-trip-capable outer `while i <
/// input2 % 97` with a bypass edge, bottom-test inner loop, a `match` in
/// the inner body), the `match` arms and the code after the nest. Six is
/// the boundary: seven hits the arity-2 F2 gap (15 felts) and eight the
/// known F6 over-full stack (20 felts: erased split-edge reloads).
/// Configuration note (campaign 16): at `--optimize=basic` six counts
/// already hit the F2 gap (`NoSolution` on `arith.rotl`); O2, O3 and -Oz
/// pass.
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
/// Configuration note (campaign 16 sweeps, 2026-09-03): at `--optimize=max`
/// LLVM unrolls the innermost body differently and this shape hits the F1
/// spills defect (`unroll_chain` class: 42 "unused phi" warnings, then
/// `NoSolution` scheduling an `scf.condition`); it passes at O2, -Oz and O1.
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
/// Configuration note (campaign 16): at `--optimize=size-min` and
/// `--optimize=basic` LLVM keeps the helper out of line inside the loops and
/// the case hits the F12 aliasing panic (`nest_continue` class); O2 and O3
/// pass.
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

/// Formerly `#[ignore]`d as the first F12 reproducer in the corpus (campaign
/// 14 attempt 2, 2026-09-03). It compiles and matches native AT THE DEFAULT
/// LEVEL and at `--optimize=max` since the guest toolchain bump to
/// nightly-2026-09-01, but the COMPILER BUG IS NOT FIXED — it only moved down
/// the level ladder: at `--optimize=size-min` and `--optimize=basic` the same
/// source still panics at hir/src/patterns/rewriter.rs:335 (measured
/// 2026-09-17), as it does at every level with nightly-2026-04-30 guests. What
/// LLVM changed is whether it leaves cfg-to-scf a loop-invariant before-block
/// argument, not the pattern. The class's default-level reproducer is
/// `invariant_args_min`. Not pinned as a `_oz` twin: the level-dependent F12
/// panic of this exact source is already carried by `nest_continue_inline`.
/// What it used to do:
///
/// two nested `for` loops, one `#[inline(never)]` call in the inner loop and a
/// `continue 'outer` from the inner loop. Building it panicked in the
/// post-lift Canonicalizer with
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
/// Suggested fix (still open, see `invariant_args_min`): bind the target
/// region to a local (dropping the op borrow) before calling
/// `inline_region_before`.
#[test]
fn nest_continue() {
    run_case("nest_continue", include_str!("../cases/case_nest_continue.rs"));
}

/// Inlined twin of `nest_continue`: the same two `for` loops and `continue
/// 'outer` with the helper `#[inline(always)]` — LLVM restructures the nest,
/// the lifted `scf.while` has no loop-invariant iter arg, and the case
/// compiles and passes.
/// Configuration note (campaign 16, measured on nightly-2026-04-30 guests):
/// below O2 (`--optimize=size-min`, `--optimize=basic`) the helper was no
/// longer inlined, so this twin became the `nest_continue` shape and hit the
/// F12 panic as well. Both cases compile with nightly-2026-09-01 guests; the
/// class itself is still open (`invariant_args_min`).
#[test]
fn nest_continue_inline() {
    run_case("nest_continue_inline", include_str!("../cases/case_nest_continue_inline.rs"));
}

/// MINIMAL COMPILE-TIME COMPILER PANIC REPRODUCER for the F12 aliasing class
/// (safe Rust, campaign 22, 2026-09-09), reduced from `programs::prog_varint`
/// to twenty-six lines: an inner `loop` whose FIRST statement is an early
/// `return`, nested in an outer `while`, a two-step xorshift byte source, and
/// two rotate constants (7 before and inside the loop; 13 in the post-loop
/// fold, shared with the xorshift's `<< 13` count).
/// No labeled `continue`, no call in the loop, no u64 state, four running
/// values fewer than `prog_varint`. Building it panics with
/// `AliasingViolationError { kind: Mutable, location:
/// hir/src/ir/operation.rs:877 }` at hir/src/patterns/rewriter.rs:335, the
/// driver's last line being `trying to match
/// 'remove-loop-invariant-args-from-before-block' dialect=scf op=while`.
/// MECHANISM (post-lift IR dumps of this case and of its passing sibling,
/// `-Z print-ir-after-pass=lift-control-flow`): cfg-to-scf materialises ONE
/// `ub.poison<u32>` per function and uses it as the initializer of EVERY
/// `scf.while` payload column, so the pattern's invariance test — "the i-th
/// yield operand equals the i-th init", or "the condition operand at the
/// yielded after-block argument's index equals the i-th init" — is satisfied
/// by ANY column that still carries that one poison value at the loop's back
/// edge. Here the in-body `scf.if` yields poison in column 2 in every arm, the
/// canonicalizer collapses it to the poison value itself, the `scf.condition`
/// forwards it, and the pattern matches. The passing sibling's in-body `if`
/// has no all-arms-poison column, so the pattern never matches.
/// Per level: default PANIC, `--optimize=max` PANIC, `--optimize=basic`
/// PANIC, `--optimize=size-min` PASSES; identical with and without guest
/// DWARF (`FUZZA_GUEST_DEBUG=0`). Bounded by `invariant_args_guard` below —
/// the same nest computing the same answer with the `return` moved BELOW the
/// `break` — and by the same nest with a labeled `break`, with a labeled
/// `continue`, and by the single-loop version, all of which compile.
/// Compile-time — no inputs involved. Un-ignore when the rewriter stops taking
/// a mutable borrow of an operation it is already borrowing.
#[test]
#[ignore = "compiler panic at the DEFAULT configuration: 'AliasingViolationError { kind: Mutable, \
            location: hir/src/ir/operation.rs:877 }' at hir/src/patterns/rewriter.rs:335 while \
            matching 'remove-loop-invariant-args-from-before-block' — F12 class; compile-time, no \
            inputs involved"]
fn invariant_args_min() {
    run_case("invariant_args_min", include_str!("../cases/case_invariant_args_min.rs"));
}

/// Passing sibling of [`invariant_args_min`]: the same nest, the same answer
/// on every input, with the inner loop's early `return` moved BELOW the
/// `break`. One statement moved is the whole diff, and it is enough for the
/// lifted `scf.while` to have no all-arms-poison payload column, so
/// `RemoveLoopInvariantArgsFromBeforeBlock` does not match. Passes at all four
/// optimization levels and with guest DWARF off.
#[test]
fn invariant_args_guard() {
    run_case("invariant_args_guard", include_str!("../cases/case_invariant_args_guard.rs"));
}

/// Per-exit pinned grid for [`invariant_args_guard`]: the in-loop `return`
/// (tag 1) and the normal exit (tag 5), plus zero / all-ones / equal pairs.
#[test]
fn invariant_args_guard_edges() {
    run_case_with_inputs(
        "invariant_args_guard_edges",
        include_str!("../cases/case_invariant_args_guard.rs"),
        &[
            (0, 0),
            (1, 0),
            (255, 3),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (1, 1),
            (0x9e37_79b9, 0x9e37_79b9),
        ],
    );
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

/// Formerly `#[ignore]`d as the minimal return-free F12 reproducer. It
/// compiles and matches native at ALL FOUR optimization levels and without
/// guest DWARF since the toolchain bump to nightly-2026-09-01 — the seven-trip
/// mixing loop no longer survives into cfg-to-scf as a nested loop with a
/// merged exit — but the COMPILER BUG IS NOT FIXED: nightly-2026-04-30 guests
/// still reproduce the panic verbatim (re-arbitrated 2026-09-17) and
/// `invariant_args_min` still panics with the current toolchain. Kept as a
/// guard of the new shape. What it used to do (reduced from
/// `programs_oz::prog_blake2b`): an outer block loop over a
/// two-word chaining state and an inner SEVEN-trip mixing loop over an
/// array-indexed working vector, with `.rodata` index tables and a message
/// array. The program contains no `return`, no `break` and no `continue` —
/// the campaign-22 source rule ("a `return` that leaves the function from
/// inside a two-level nest") is therefore not the only producer. Building it
/// panics with `AliasingViolationError { kind: Mutable, location:
/// hir/src/ir/operation.rs:877 }` at hir/src/patterns/rewriter.rs:335, the
/// pattern driver's last line being `trying to match
/// 'remove-loop-invariant-args-from-before-block' dialect=scf op=while`.
/// MECHANISM (post-lift IR of this case and of its sibling, `-Z
/// print-ir-after-pass=lift-control-flow` with
/// `MIDENC_TRACE='pass:lift-control-flow=trace'`): cfg-to-scf materialises ONE
/// `ub.poison<u32>` per function and initialises every `scf.while` payload
/// column with it. Here the INNER loop's exit dispatch is the poison source:
/// inside the inner while's `before` region an `scf.if` on the mixing loop's
/// exit test yields TWELVE results whose first SIX columns are that one poison
/// value in BOTH arms (the remaining six carry a value pair swapped between
/// the arms and four values common to both), the canonicalizer collapses each
/// all-arms-poison
/// column to the poison value itself, `scf.condition` forwards it, and the
/// pattern's "yield operand equals the init operand" test is satisfied. The
/// source construct behind those columns is the merged exit of the inner
/// counted loop — the payload the outer loop's `scf.index_switch` dispatch
/// selects on — not any user-visible early exit.
/// Per level on nightly-2026-04-30 guests: default PANIC, `--optimize=basic`
/// PANIC, `--optimize=max` and `--optimize=size-min` compile.
/// Bounded by [`invariant_args_noreturn_guard`] below: with SIX mixing steps
/// LLVM unrolls the inner loop into the block loop, cfg-to-scf sees a single
/// `scf.while` whose `scf.condition` forwards only real values, and the
/// pattern never matches.
#[test]
fn invariant_args_noreturn() {
    run_case(
        "invariant_args_noreturn",
        include_str!("../cases/case_invariant_args_noreturn.rs"),
    );
}

/// Passing sibling of [`invariant_args_noreturn`]: the same program with SIX
/// mixing steps instead of seven. LLVM unrolls the inner loop, so the lifted
/// IR has ONE `scf.while` (four payload columns) instead of a nest, its
/// `scf.condition` forwards four real values, and no column carries poison.
/// This is the one-ingredient boundary of the return-free producer: what
/// decides F12 is whether cfg-to-scf still sees a nested loop with a merged
/// exit, not any source-level idiom.
#[test]
fn invariant_args_noreturn_guard() {
    run_case(
        "invariant_args_noreturn_guard",
        include_str!("../cases/case_invariant_args_noreturn_guard.rs"),
    );
}
