//! Cases whose value is the cf/scf CANONICALIZATION work they force the
//! compiler to do: removing a result column of a region op and remapping the
//! survivors, collapsing a passthrough branch, merging switch arms into the
//! fallback, and turning a two-successor switch into a conditional branch.
//! Each of those rewrites is index arithmetic over columns, cases or successor
//! arguments, so a wrong remap is a SILENT MISCOMPILE rather than a panic —
//! which is why every case here has an `_edges` twin that pins one input per
//! arm / per exit and asserts the values against the native build on every
//! run, not only when the fuzzer happens to draw them.
//!
//! Pattern-firing evidence for each case was taken with
//! `MIDENC_TRACE='pattern-rewrite-driver=trace'`, which prints
//! `trying to match '<pattern>'` before every attempt and
//! `pattern matched successfully` after every rewrite. All four shapes fire
//! their target pattern at the DEFAULT guest opt-level, with the counts quoted
//! below, and fire it the same number of times at `--optimize=size-min` (only
//! the incidental `split-critical-edges` / `convert-trivial-if-to-select`
//! counts move by one there). None of them needs a pinned configuration, so
//! they stay on plain `run_case`.

use super::super::harness::{run_case, run_case_with_inputs};

/// Region-op RESULT COLUMN removal with index remapping, eight times over.
///
/// Each of the eight sequential 8-trip loops here has an empty `continue` arm,
/// which is exactly the shape that leaves a cfg-to-scf exit-dispatch column
/// with no consumer: `WhileUnusedResult` drops the loop result, its yield
/// operand dies, and `IndexSwitchRemoveUnusedResults` then rebuilds the
/// `scf.index_switch` without that column, remapping every survivor's index.
/// Fired counts at the default level: `while-unused-result` 8,
/// `index-switch-remove-unused-results` 8, `while-remove-unused-args` 8,
/// `split-critical-edges` 8, `convert-trivial-if-to-select` 16 — one cascade
/// per loop, so the count is a linear function of the number of such loops
/// (1/2/4/8 loops → 1/2/4/8 cascades). The four cascade counts are identical
/// at `--optimize=size-min`.
///
/// Producer fact (campaign 19): the EMPTINESS of the `continue` arm is what
/// makes the cascade happen. The same loop with a `continue` arm that updates
/// any carried variable produces no unused column and fires neither pattern,
/// and neither does a loop that leaves through an early `break` instead. The
/// previously known producer (`opt_levels::loop_keep_oz`) fired each pattern
/// once, at `-Oz` only; this case fires them eight times at the default level.
#[test]
fn col_cascade() {
    run_case("col_cascade", include_str!("../cases/case_col_cascade.rs"));
}

/// Pinned grid for [`col_cascade`]: `input2`'s bits decide which of the eight
/// loops take their `continue` edge on which trip, so these pairs cover
/// all-continue (`0`), never-continue (`u32::MAX`), two alternating bit
/// patterns and two mixed ones — every loop's carried value is asserted under
/// a different continuation mix.
#[test]
fn col_cascade_edges() {
    run_case_with_inputs(
        "col_cascade_edges",
        include_str!("../cases/case_col_cascade.rs"),
        &[
            (0, 0),
            (0, u32::MAX),
            (1, 0xaaaa_aaaa),
            (u32::MAX, 0x5555_5555),
            (0x1234_5678, 0xdead_beef),
            (1, 0),
        ],
    );
}

/// Passthrough-branch collapse (`SimplifyPassthroughCondBr`) at the DEFAULT
/// opt-level: an inner loop whose only exits are four in-loop `return`s plus a
/// `break`, nested in a kept outer loop. Fired counts: ten
/// `simplify-passthrough-cond-br` rewrites interleaved with thirteen
/// `split-critical-edges` — the two alternate to a fixpoint, which is the
/// mechanism (splitting a critical edge leaves a passthrough block whose
/// target has a unique predecessor, satisfying the guard in
/// `collapse_branch`).
///
/// Producer facts (campaign 19, ladder over the number of return sites R and
/// the position P of the `break` among them): the pattern fires if and only if
/// at least one in-loop `return` comes AFTER the `break` — with the `break`
/// last it never fires, for every R. The count is ten on every firing rung
/// regardless of R (2..5) and P, so it is a property of the two-level frame
/// rather than of the number of return sites, and R = 6 stops it entirely
/// (LLVM restructures the body). Unlike `opt_levels::deadfall_oz`, which
/// documents this pattern as a `-Oz` shape, the rewrite happens at the default
/// opt-level too: the ten passthrough collapses happen at both levels (only
/// the interleaved `split-critical-edges` count moves, 13 vs 12).
#[test]
fn passthru_frame() {
    run_case("passthru_frame", include_str!("../cases/case_passthru_frame.rs"));
}

/// Pinned exit grid for [`passthru_frame`]: one native-verified input pair per
/// return site (tags 1-4 are the four in-loop returns, tag 5 is the outer
/// loop's normal exit), so every collapsed passthrough is value-checked on
/// every run.
#[test]
fn passthru_frame_edges() {
    run_case_with_inputs(
        "passthru_frame_edges",
        include_str!("../cases/case_passthru_frame.rs"),
        &[(0, 40), (0, 4), (0, 0), (0, 16), (1024, 0)],
    );
}

/// Switch ARM MERGING: a sixteen-arm `match` on an input-derived selector
/// where five scattered arms share the default arm's body. LLVM points those
/// `br_table` entries at the fallback block, and
/// `simplify-switch-fallback-overlap` fires ONCE, rebuilding the `cf.switch`
/// without all five overlapping cases at the same time — the fire count never
/// measures how many arms were merged, which is why the value check is per
/// arm.
///
/// Ladder fact (campaign 19): neither the arm count (8/16/32), nor the number
/// of duplicated arms (3/5/9), nor their placement (contiguous, scattered,
/// tail) moves the count off one rewrite per switch; `control_flow::sm16`'s
/// fourteen fires are fourteen distinct switches, not fourteen merged arms.
#[test]
fn arms_merge() {
    run_case("arms_merge", include_str!("../cases/case_arms_merge.rs"));
}

/// Per-arm pinned grid for [`arms_merge`]: the selector is `input1 & 15` when
/// `input2` is zero and is returned in the top nibble, so these sixteen pairs
/// assert one input per arm — the five merged arms, the ten kept arms and the
/// true default alike. A case key rebuilt at the wrong index, or a
/// non-overlapping case dropped along with the overlapping ones, changes
/// exactly one of these values.
#[test]
fn arms_merge_edges() {
    run_case_with_inputs(
        "arms_merge_edges",
        include_str!("../cases/case_arms_merge.rs"),
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (8, 0),
            (9, 0),
            (10, 0),
            (11, 0),
            (12, 0),
            (13, 0),
            (14, 0),
            (15, 0),
        ],
    );
}

/// Two-successor switch rewriting (`SimplifyCondBrLikeSwitch`): a sixteen-arm
/// `match` with three dynamically-impossible `panic!()` arms. The trapping
/// arms and the fallback-overlap merge together leave a `cf.switch` with a
/// single case plus its fallback, which the pattern replaces with an equality
/// test and a `cf.cond_br` — a rewrite that picks the case key and both
/// destinations by index, so a swap there is a silent miscompile. Fired
/// counts: `simplify-cond-br-like-switch` 1,
/// `simplify-switch-fallback-overlap` 1, `split-critical-edges` 6 — the same
/// at the default level and at `--optimize=size-min`.
///
/// Closure correction (campaign 19): this pattern was recorded as having no
/// producer at all, on the argument that LLVM emits compares rather than a
/// two-target `br_table` and that overlap merging never gets a switch that low.
/// It does — a corpus-wide trace found six producers among the committed
/// control-flow cases (`unreachable_exits`, `switch_loop_mix`,
/// `switch_trap_arm`, `trap_branch`, `spin_guard`, `ret_args`), all of them
/// trap or impossible-guard shapes.
#[test]
fn trap_dispatch() {
    run_case("trap_dispatch", include_str!("../cases/case_trap_dispatch.rs"));
}

/// Per-arm pinned grid for [`trap_dispatch`]: sixteen pairs, one per arm of
/// the match (including the three arms whose impossible `panic!()` guard must
/// stay un-taken), with the selector echoed in the top nibble.
#[test]
fn trap_dispatch_edges() {
    run_case_with_inputs(
        "trap_dispatch_edges",
        include_str!("../cases/case_trap_dispatch.rs"),
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (8, 0),
            (9, 0),
            (10, 0),
            (11, 0),
            (12, 0),
            (13, 0),
            (14, 0),
            (15, 0),
        ],
    );
}
