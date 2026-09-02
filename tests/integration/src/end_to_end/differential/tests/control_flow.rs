//! Branches, loops, switches, trap edges, and cfg-to-scf shapes.

use super::super::harness::{run_case, run_case_with_inputs};

#[test]
fn branchy() {
    run_case("branchy", include_str!("../cases/case_branchy.rs"));
}

/// Exercises bounded loops with carried values and nested conditional control flow.
#[test]
fn while_carried() {
    run_case("while_carried", include_str!("../cases/case_while_carried.rs"));
}

/// Exercises dense match/switch control flow, including wasm `br_table` translation.
#[test]
fn dense_match() {
    run_case("dense_match", include_str!("../cases/case_dense_match.rs"));
}

/// Exercises nested loops, local breaks, and labelled non-local loop exits.
#[test]
fn nested_breaks() {
    run_case("nested_breaks", include_str!("../cases/case_nested_breaks.rs"));
}

/// Exercises sparse/default-heavy switch control flow.
#[test]
fn sparse_match() {
    run_case("sparse_match", include_str!("../cases/case_sparse_match.rs"));
}

/// Exercises compile-time translation of an unreachable panic edge.
#[test]
fn unreachable_guard() {
    run_case("unreachable_guard", include_str!("../cases/case_unreachable_guard.rs"));
}

/// Bounded loop whose Rust-level duplicated/dead/loop-invariant carried values
/// all travel through wasm locals, so the lifted scf.while forwards no values
/// and the while arg/result canonicalization patterns are invoked but bail
/// early (the locals argument, see KNOWLEDGE.md) — covers those bail paths.
#[test]
fn loop_results() {
    run_case("loop_results", include_str!("../cases/case_loop_results.rs"));
}

/// Loop with three distinct exit edges — exercises cfg-to-scf exit
/// multiplexing (`transform_to_reduce_loop`) and scf.while arg/result
/// canonicalization.
#[test]
fn multi_exit_loop() {
    run_case("multi_exit_loop", include_str!("../cases/case_multi_exit_loop.rs"));
}

/// Dynamically-impossible panic path (cross-modulus contradiction) — the
/// surviving trap exercises `ub::Unreachable` translation and lowering.
#[test]
fn trap_branch() {
    run_case("trap_branch", include_str!("../cases/case_trap_branch.rs"));
}

/// Four-exit loop plus eq-chains that canonicalize into contiguous-at-7 and
/// sparse cf.switch ops — exercises binary-search (interval guard) and
/// linear-search switch lowering.
///
/// Formerly `#[ignore]`d: the frontend re-typed the `br_table` selector with
/// a checked I32->U32 cast, but LLVM rebases contiguous switches by
/// wrapping-subtracting the smallest case, so a selector below the minimum
/// wrapped negative and the VM aborted with 'value does not fit in i32'
/// (issues #1235/#1243, fixed by PR #1245: `pop1_bitcasted` in
/// `translate_br_table`). Re-verified passing 2026-08-27.
#[test]
fn switch_shapes() {
    run_case("switch_shapes", include_str!("../cases/case_switch_shapes.rs"));
}

/// Pinned regression guard for the fixed #1235/#1243 `br_table` selector
/// wrap: input pair (1669775643, 1062584501) makes the rebased selector wrap
/// below the smallest case, which must dispatch to the default arm (random
/// draws hit an `h < 7` pair only ~7/251 of the time, so the pin keeps the
/// wrap path exercised on every run).
#[test]
fn switch_shapes_repro() {
    run_case_with_inputs(
        "switch_shapes_repro",
        include_str!("../cases/case_switch_shapes.rs"),
        &[(1669775643, 1062584501)],
    );
}

/// Loop with multiple `continue` backedges and a mid-body break — exercises
/// cfg-to-scf latch multiplexing and undef discriminator threading.
#[test]
fn continue_paths() {
    run_case("continue_paths", include_str!("../cases/case_continue_paths.rs"));
}

/// br_table dispatch with one impossible-panic arm — switch successor
/// regions with mixed return-like terminators (ret vs unreachable).
#[test]
fn switch_trap_arm() {
    run_case("switch_trap_arm", include_str!("../cases/case_switch_trap_arm.rs"));
}

/// Mid-loop exit with a rotation-resistant body — produces an scf.while
/// with a non-empty `after` region.
#[test]
fn midloop_exit() {
    run_case("midloop_exit", include_str!("../cases/case_midloop_exit.rs"));
}

/// Tail-merged return paths (exit block with args) plus an impossible trap
/// exit — cf.cond_br lowering with successor block arguments.
#[test]
fn ret_args() {
    run_case("ret_args", include_str!("../cases/case_ret_args.rs"));
}

/// Labeled break/continue through two loop levels, all-state-in-locals exits
/// (zero-result index_switch), loop-produced bool, and distinct-constant
/// match returns — nested scf.while + chained discriminator index_switches.
#[test]
fn cf_shapes() {
    run_case("cf_shapes", include_str!("../cases/case_cf_shapes.rs"));
}

/// Statically-infinite loop behind an impossible guard plus two planted wasm
/// `unreachable` sites — cfg-to-scf `create_unreachable_terminator`, mixed
/// return-like exit kinds, and `ub.unreachable`-terminated region lowering.
#[test]
fn unreachable_exits() {
    run_case("unreachable_exits", include_str!("../cases/case_unreachable_exits.rs"));
}

/// br_table in a loop with break/continue/return/trap arms — nested user +
/// discriminator index_switches and mixed in-/out-of-loop switch successors.
#[test]
fn switch_loop_mix() {
    run_case("switch_loop_mix", include_str!("../cases/case_switch_loop_mix.rs"));
}

/// Bare `loop {}` behind an impossible cross-modulus guard — the loop header
/// is a block containing only its own back-edge br, exercising the
/// collapse-into-self-loop bail of the passthrough-branch canonicalizations.
#[test]
fn spin_guard() {
    run_case("spin_guard", include_str!("../cases/case_spin_guard.rs"));
}

/// Campaign 11 ladder 1 (state machines): eight states driven by one input
/// bit per step, four exits (three arm breaks + header step budget), two
/// `continue` arms that skip the shared tail. Exit tag = top nibble.
#[test]
fn sm_bits() {
    run_case("sm_bits", include_str!("../cases/case_sm_bits.rs"));
}

/// Pinned exits of `sm_bits` (native-verified exit tags): step budget T
/// (0, 0), (1, 0), (0, 2); arm break A (3735928559, 194), (123456789,
/// 987654321); arm break B (4294967295, 4294967295), (7, 97), (65535,
/// 65536); arm break C (100, 200), (77777, 3).
#[test]
fn sm_bits_edges() {
    run_case_with_inputs(
        "sm_bits_edges",
        include_str!("../cases/case_sm_bits.rs"),
        &[
            (0, 0),
            (1, 0),
            (0, 2),
            (3735928559, 194),
            (123456789, 987654321),
            (4294967295, 4294967295),
            (7, 97),
            (65535, 65536),
            (100, 200),
            (77777, 3),
        ],
    );
}

/// Campaign 11 ladder 1: a u64-state machine matched against 64-bit
/// constants (wat-verified: LLVM lowers it as a `br_table` on a wrapped
/// half plus eight `i64.eq`/`i64.ne` compares) and a (u32, u32) pair-state
/// machine with tuple patterns and guards.
#[test]
fn sm_wide() {
    run_case("sm_wide", include_str!("../cases/case_sm_wide.rs"));
}

/// Pinned exits of `sm_wide` (top two bits = u64 machine exit 1..3, next
/// two = pair machine exit 1..3, native-verified): (1, 0) = 1/1, (0, 0) =
/// 1/2, (0, 1) = 1/3, (4294967295, 4294967295) = 2/1, (123456789,
/// 987654321) = 2/3, (100, 200) = 3/1, (2863311530, 5) = 3/3.
#[test]
fn sm_wide_edges() {
    run_case_with_inputs(
        "sm_wide_edges",
        include_str!("../cases/case_sm_wide.rs"),
        &[
            (1, 0),
            (0, 0),
            (0, 1),
            (4294967295, 4294967295),
            (123456789, 987654321),
            (100, 200),
            (2863311530, 5),
            (1431655765, 16),
        ],
    );
}

/// Campaign 11 ladder 1: two nested state machines, the inner machine's
/// exit tag selecting the outer transition.
#[test]
fn sm_nested() {
    run_case("sm_nested", include_str!("../cases/case_sm_nested.rs"));
}

/// Pinned outer exits of `sm_nested` (native-verified): budget (0, 0),
/// (4294967295, 4294967295); arm exit A (2863311530, 5); arm exit B
/// (2147483648, 12345), (0, 1), (8, 8).
#[test]
fn sm_nested_edges() {
    run_case_with_inputs(
        "sm_nested_edges",
        include_str!("../cases/case_sm_nested.rs"),
        &[
            (0, 0),
            (4294967295, 4294967295),
            (2863311530, 5),
            (2147483648, 12345),
            (0, 1),
            (8, 8),
        ],
    );
}

/// Campaign 11 ladder 2 (multi-exit loops): five exits carrying different
/// values — labeled break from a match arm, early return, inner break,
/// post-inner value break, zero-trip-capable outer header exit.
#[test]
fn exit_values() {
    run_case("exit_values", include_str!("../cases/case_exit_values.rs"));
}

/// Pinned exits of `exit_values` (native-verified): outer header exit at
/// zero trips (0, 0), (7, 97), (3735928559, 194) and at one trip (0, 1),
/// (4042322160, 98); labeled break from the match arm (2863311530, 5),
/// (999, 4); early return (2147483648, 12345); post-inner value break
/// (4294967295, 4294967295), (65535, 65536), (8, 8).
#[test]
fn exit_values_edges() {
    run_case_with_inputs(
        "exit_values_edges",
        include_str!("../cases/case_exit_values.rs"),
        &[
            (0, 0),
            (7, 97),
            (3735928559, 194),
            (0, 1),
            (4042322160, 98),
            (2863311530, 5),
            (999, 4),
            (2147483648, 12345),
            (4294967295, 4294967295),
            (65535, 65536),
            (8, 8),
        ],
    );
}

/// Campaign 11 ladder 3 (loop-carried sets): five- and nine-variable loops
/// with rotations, swaps, subset updates, a back-edge-only read, and a
/// loop-carried selector.
#[test]
fn carried_sets() {
    run_case("carried_sets", include_str!("../cases/case_carried_sets.rs"));
}

/// Campaign 11 ladder 4 (nesting depth) boundary guard: eight nested loops,
/// three of them zero-trip-capable, with escapes (continue / labeled breaks
/// / return) from four different depths. Eight is the deepest nest that
/// compiles at the default opt-level: cfg-to-scf threads every escaping
/// value and exit discriminator outward as REGION-OP RESULT columns, so the
/// widest lifted `scf.if` here already carries fifteen u32 results; one
/// more level overflows the 16-felt budget (`deep_nest_overflow` below).
/// The same source fails under `--optimize=size-min` (`nest8_oz` in
/// `opt_levels.rs`), where LLVM keeps more of the nest as loops.
#[test]
fn nest8() {
    run_case("nest8", include_str!("../cases/case_nest8.rs"));
}

/// Pinned exits of `nest8` (native-verified): all levels zero-trip (0, 0);
/// one-trip outer levels (0, 1), (0, 2); the `continue 'l2` path (0, 65),
/// (0, 68); labeled break to level 1 (256, 64); labeled break to level 5
/// (19456, 65), (19456, 77); early return from level 8 (17920, 128),
/// (17920, 131).
#[test]
fn nest8_edges() {
    run_case_with_inputs(
        "nest8_edges",
        include_str!("../cases/case_nest8.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 65),
            (0, 68),
            (256, 64),
            (19456, 65),
            (19456, 77),
            (17920, 128),
            (17920, 131),
        ],
    );
}

/// Campaign 11 ladder 4: diamond inside a loop inside a diamond inside a
/// zero-trip-capable loop, with arm-local loops and three inner exits.
#[test]
fn diamond_nest() {
    run_case("diamond_nest", include_str!("../cases/case_diamond_nest.rs"));
}

/// Pinned exits of `diamond_nest` (native-verified): outer zero-trip (0, 0)
/// and one-trip (0, 1) header exits, normal exit (999, 4); labeled break
/// from the inner then-arm (2863311530, 5), (7, 97); inner else-arm break
/// then normal exit (2147483648, 12345), (8, 8); early return from the
/// outer else-arm (4294967295, 4294967295), (4042322160, 98), (0, 63).
#[test]
fn diamond_nest_edges() {
    run_case_with_inputs(
        "diamond_nest_edges",
        include_str!("../cases/case_diamond_nest.rs"),
        &[
            (0, 0),
            (0, 1),
            (999, 4),
            (2863311530, 5),
            (7, 97),
            (2147483648, 12345),
            (8, 8),
            (4294967295, 4294967295),
            (4042322160, 98),
            (0, 63),
        ],
    );
}

/// Campaign 11 ladder 6 (switch shapes): dense, holey-with-hot-default,
/// sparse u16, u8 `|`/range/guard patterns, nested match, two matches on one
/// selector, and a br_table with a loop-carried selector.
#[test]
fn switch_forms() {
    run_case("switch_forms", include_str!("../cases/case_switch_forms.rs"));
}

/// Pinned selectors of `switch_forms`: `input1` values whose hashed dense
/// selector is 0, 1, 2, 3, 8, 9, 10, 11, 15, 20, 21 and 31 (every holey
/// case, the last dense case, and defaults on both sides of the holes); u16
/// selectors 0, 7, 100, 1000, 0x7fff, 0x8000, 0xffff and a truncated
/// 0x1_0007; u8 selectors 5, 9, 10, 19 (both guard parities), 20, 40, 60,
/// 80, 100, 199, 200, 250, 255.
#[test]
fn switch_forms_edges() {
    run_case_with_inputs(
        "switch_forms_edges",
        include_str!("../cases/case_switch_forms.rs"),
        &[
            (0, 0),
            (13, 0),
            (5, 0),
            (18, 0),
            (15, 0),
            (28, 0),
            (7, 0),
            (20, 0),
            (4, 0),
            (14, 0),
            (27, 0),
            (21, 0),
            (0, 7),
            (0, 100),
            (0, 1000),
            (0, 0x7fff),
            (0, 0x8000),
            (0, 0xffff),
            (0, 0x1_0007),
            (5 << 3, 2),
            (9 << 3, 2),
            (10 << 3, 2),
            (10 << 3, 3),
            (19 << 3, 2),
            (19 << 3, 3),
            (20 << 3, 2),
            (40 << 3, 3),
            (60 << 3, 2),
            (80 << 3, 3),
            (100 << 3, 2),
            (199 << 3, 3),
            (200 << 3, 2),
            (250 << 3, 3),
            (255 << 3, 2),
        ],
    );
}

/// Campaign 11 ladder 7 (short-circuit lattices): `&&`/`||` chains and
/// mixed lattices over side-effecting probes whose evaluation set is
/// recorded in an atomic counter.
#[test]
fn shortcircuit() {
    run_case("shortcircuit", include_str!("../cases/case_shortcircuit.rs"));
}

/// Campaign 11 ladder 8 (jump-threading sources): condition tested twice,
/// guarded arms sharing one target, goto-like loop with duplicated tails,
/// two-constant-exit loop.
#[test]
fn threading() {
    run_case("threading", include_str!("../cases/case_threading.rs"));
}

/// Pinned trips of `threading`'s two-constant-exit loop (bound `input1 %
/// 89`, native-verified): zero-trip (0, 0), (0, 12345), (89, 3); one-trip
/// (1, 0), (1, 5), (90, 7); n-trip header exit (178, 4294967295); n-trip
/// body exit (1000, 0), (1000, 1).
#[test]
fn threading_edges() {
    run_case_with_inputs(
        "threading_edges",
        include_str!("../cases/case_threading.rs"),
        &[
            (0, 0),
            (0, 12345),
            (89, 3),
            (1, 0),
            (1, 5),
            (90, 7),
            (178, 4294967295),
            (1000, 0),
            (1000, 1),
        ],
    );
}

/// Campaign 11 round 2: `Ordering`-dispatched state machine (unsigned vs
/// signed compare pair), three-way `match a.cmp(&b)`, min/max/clamp selects.
#[test]
fn cmp_dispatch() {
    run_case("cmp_dispatch", include_str!("../cases/case_cmp_dispatch.rs"));
}

/// Pinned compare boundaries of `cmp_dispatch`: equal pairs (0, 0), (5, 5),
/// (0x8000_0000, 0x8000_0000), (0xffff_ffff, 0xffff_ffff); pairs whose
/// signed and unsigned orderings disagree (0x7fff_ffff, 0x8000_0000),
/// (0x8000_0000, 0x7fff_ffff), (1, 0xffff_ffff), (0xffff_ffff, 1); and
/// agreeing pairs (1, 2), (2, 1).
#[test]
fn cmp_dispatch_edges() {
    run_case_with_inputs(
        "cmp_dispatch_edges",
        include_str!("../cases/case_cmp_dispatch.rs"),
        &[
            (0, 0),
            (5, 5),
            (0x8000_0000, 0x8000_0000),
            (0xffff_ffff, 0xffff_ffff),
            (0x7fff_ffff, 0x8000_0000),
            (0x8000_0000, 0x7fff_ffff),
            (1, 0xffff_ffff),
            (0xffff_ffff, 1),
            (1, 2),
            (2, 1),
        ],
    );
}

/// Campaign 11 round 2: do-while with body-computed condition + continue
/// skipping the bottom test, loop-carried bool condition, nested do-while.
#[test]
fn do_while() {
    run_case("do_while", include_str!("../cases/case_do_while.rs"));
}

/// Pinned exits of `do_while`'s first loop (native-verified): bottom-test
/// exit (0, 0), (0, 1), (0, 2); exit after the `continue` arm (0, 3),
/// (0, 56), (0, 109); mid-body exit (1280, 1), (1280, 2), (1280, 3).
#[test]
fn do_while_edges() {
    run_case_with_inputs(
        "do_while_edges",
        include_str!("../cases/case_do_while.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 56),
            (0, 109),
            (1280, 1),
            (1280, 2),
            (1280, 3),
        ],
    );
}

/// Campaign 11 round 2: triangle loop nest (inner bound = outer counter,
/// zero-trip at i == 0) with return / labeled continue / break escapes.
#[test]
fn triangle() {
    run_case("triangle", include_str!("../cases/case_triangle.rs"));
}

/// Pinned exits of `triangle` (native-verified): outer zero-trip (0, 0),
/// outer one-trip with inner zero-trip (0, 1), inner one-trip with third
/// level zero-trip (0, 2); `continue 'outer` path (0, 13), (0, 14); inner
/// break path (768, 17), (768, 36); early return (256, 15), (256, 17).
#[test]
fn triangle_edges() {
    run_case_with_inputs(
        "triangle_edges",
        include_str!("../cases/case_triangle.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 13),
            (0, 14),
            (768, 17),
            (768, 36),
            (256, 15),
            (256, 17),
        ],
    );
}

/// Campaign 11 round 2: inner loop produces the outer br_table selector;
/// outer arms set the inner bound.
#[test]
fn nested_selector() {
    run_case("nested_selector", include_str!("../cases/case_nested_selector.rs"));
}

/// Pinned exits of `nested_selector` (native-verified): outer budget (0, 0),
/// (0, 1); arm-2 break (0, 3), (0, 6); arm-6 last-trip break (0, 10),
/// (0, 12); arm-4 early return (0, 9), (0, 21).
#[test]
fn nested_selector_edges() {
    run_case_with_inputs(
        "nested_selector_edges",
        include_str!("../cases/case_nested_selector.rs"),
        &[(0, 0), (0, 1), (0, 3), (0, 6), (0, 10), (0, 12), (0, 9), (0, 21)],
    );
}

/// Campaign 11 ladder 1 boundary rung: sixteen-state machine, two bits per
/// step, 64 transition arms, six exits.
#[test]
fn sm16() {
    run_case("sm16", include_str!("../cases/case_sm16.rs"));
}

/// Pinned exits of `sm16` (native-verified): budget (0, 0), (0, 1); arm
/// breaks 2..5 (6656, 49), (0, 2), (3584, 113), (12288, 241); early return
/// (1792, 0), (1792, 2).
#[test]
fn sm16_edges() {
    run_case_with_inputs(
        "sm16_edges",
        include_str!("../cases/case_sm16.rs"),
        &[
            (0, 0),
            (0, 1),
            (6656, 49),
            (0, 2),
            (3584, 113),
            (12288, 241),
            (1792, 0),
            (1792, 2),
        ],
    );
}

/// Campaign 11 ladder 4 / exit multiplicity bound for `deep_nest_overflow`:
/// FIVE nested zero-trip-capable loops whose innermost body can leave to
/// every enclosing level (labeled breaks to levels 1-4, a plain break, a
/// `continue` of level 2, an early return) plus one more labeled break per
/// level tail — ten escape sites at depth five compile and pass, so the
/// overflow below is driven by nesting DEPTH, not by the number of exits.
#[test]
fn wide_exits() {
    run_case("wide_exits", include_str!("../cases/case_wide_exits.rs"));
}

/// Pinned exits of `wide_exits` (native-verified): all-zero-trip (0, 0);
/// normal completion (0, 17); tail break to level 2 (455, 272); `continue
/// 'l2` (689, 272); innermost breaks to levels 1..4 (741, 17), (1820, 323),
/// (1378, 119), (1534, 272); plain inner break (3692, 68); early return
/// (273, 68); level-3 tail break (65, 527); level-2 tail break (0, 187).
#[test]
fn wide_exits_edges() {
    run_case_with_inputs(
        "wide_exits_edges",
        include_str!("../cases/case_wide_exits.rs"),
        &[
            (0, 0),
            (0, 17),
            (455, 272),
            (689, 272),
            (741, 17),
            (1820, 323),
            (1378, 119),
            (1534, 272),
            (3692, 68),
            (273, 68),
            (65, 527),
            (0, 187),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-09-02, campaign 11): building
/// this case panics with `implicit operand stack overflow along incoming
/// control flow edges of after(scf.if in ^block76)` at
/// hir-analysis/src/analyses/spills.rs:1533 (`assert!(taken <= K)` in
/// `compute_w_exit_region_branch_op`, post-lift TransformSpills). Shape: NINE
/// nested `while i < (input >> k) % 3` loops (all zero-trip-capable) with
/// labeled breaks from the innermost body to three outer levels and an early
/// return. Root cause (spills trace + HIR of the passing eight-level twin):
/// cfg-to-scf's exit dispatch threads every escaping value and every level's
/// exit discriminator outward as RESULT columns of the enclosing
/// `scf.while`/`scf.if` ops (~two columns per level; the widest `scf.if` of
/// `nest8` has fifteen u32 results). At depth nine the results exceed K =
/// 16 felts: the analysis trims them with `spill_trailing_until_fits`, but
/// then inserts every value live along ALL incoming edges (`count ==
/// predecessor_count`, here two u32s) into `take` without re-checking the
/// budget, and the assert fires. Not F1 (no unrolling, no "unused phi"),
/// not F2 (no scheduler involved), not F6 (no dominator staleness; the
/// pressure is real region-result width, not stale reloads). Bounded by:
/// `nest8` (eight levels, same generator family, passes with pinned
/// per-level exits) and `wide_exits` (ten escape sites at depth five pass
/// — exit count is not the driver); at `--optimize=size-min` the same
/// assert fires from depth SEVEN (six passes; `nest8_oz` in opt_levels.rs
/// pins it on the kept eight-level source). Compile-time, no inputs
/// involved. Un-ignore when this case compiles (the region-exit budget
/// must spill more results, or cfg-to-scf must not widen results per
/// level).
#[test]
#[ignore = "compiler panic: 'implicit operand stack overflow along incoming control flow edges of \
            after(scf.if in ^block76)' at hir-analysis/src/analyses/spills.rs:1533 — cfg-to-scf \
            exit-dispatch result columns of a nine-deep loop nest exceed the 16-felt budget \
            (compile-time, no inputs involved)"]
fn deep_nest_overflow() {
    run_case("deep_nest_overflow", include_str!("../cases/case_deep_nest_overflow.rs"));
}

/// Campaign 11 ladder 5 (many-block functions) boundary guard: a 200-arm
/// `match`, an eight-level decision tree (256 leaves) and a 128-step chain
/// of bit-guarded `if`s — about 800 basic blocks, all threading their
/// values through wasm locals; compiles and passes at the default opt-level.
#[test]
fn blocks_max() {
    run_case("blocks_max", include_str!("../cases/case_blocks_max.rs"));
}

/// Campaign 11 ladder 6 boundary guard: the widest `br_table` that builds —
/// a 255-arm dense `match` (255 targets plus the default). One more arm is
/// `switch256` below.
#[test]
fn switch255() {
    run_case("switch255", include_str!("../cases/case_switch255.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-09-02, campaign 11): building
/// this case panics with `too many operand groups: TryFromIntError(
/// PosOverflow)` at hir/src/ir/operation/builder.rs:224 (the successor
/// operand-group index of an op is a `u8`, so a `cf.switch` cannot hold
/// more than 256 successors). Shape: a 256-arm dense `match` on `input1 %
/// 256` — LLVM emits one `br_table` with 256 targets plus a default, and
/// the wasm frontend's `translate_br_table` builds a `cf.switch` with one
/// successor group per target. Bounded by: `switch255` (255 arms) compiles
/// and passes; the 253- and 254-arm rungs pass too. Compile-time, no inputs
/// involved. Un-ignore when this case compiles (wider group index, or the
/// frontend splitting oversized switches).
#[test]
#[ignore = "compiler panic: 'too many operand groups: TryFromIntError(PosOverflow)' at \
            hir/src/ir/operation/builder.rs:224 — a br_table with 256 targets overflows the u8 \
            successor operand-group index (compile-time, no inputs involved)"]
fn switch256() {
    run_case("switch256", include_str!("../cases/case_switch256.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-08-27): building this case
/// panics in the MASM operand scheduler with `NoSolution` at
/// codegen/masm/src/lower/lowering.rs:109. Trigger: LLVM runtime-unrolls the
/// `% 97`-bounded loop 8x into a single block computing the interleaved
/// non-reassociable chain `((((acc*33)^i)*33)^(i+1))*33 ...`, whose live
/// state spills. Root cause (triage 2026-08-27): NOT a hard scheduling
/// problem — TransformSpills' `insert_required_phis`
/// (hir-transform/src/spill.rs) seeds EVERY predecessor edge of a
/// dominance-frontier join with the spilled value itself; on the loop-BYPASS
/// edge (this zero-trip-capable `while`) the definition does not dominate,
/// the seed is never rewritten (the pass warns "unused phi"; dead-phi
/// removal is an open TODO), no verifier checks SSA dominance, and
/// cfg-to-scf threads the dead phi args into a sibling-region `scf.yield` —
/// so the scheduler receives an UNSATISFIABLE problem (an operand that is
/// not on the operand stack). With yield arity 2 the TwoArgs-only tactic
/// list returns NotApplicable and NoSolution panics; the arity-3 twin of
/// the same defect is `unroll_rotmix` (spills.rs), and the independent
/// in-contract arity-2 solver gap is `rotl_window` (spills.rs). Bounded by:
/// the xor-only and mul-only bodies of the identical loop compile and pass,
/// and `case_chain300`'s ~400-op straight-line chain passes. Un-ignore when
/// this case compiles (after a spills fix it may still hit the rotl_window
/// gap — then re-triage).
#[test]
#[ignore = "compiler panic: 'with error: NoSolution' at codegen/masm/src/lower/lowering.rs:109 \
            while scheduling the 8x-unrolled mul-xor loop chain (compile-time, no inputs involved)"]
fn unroll_chain() {
    run_case("unroll_chain", include_str!("../cases/case_unroll_chain.rs"));
}
