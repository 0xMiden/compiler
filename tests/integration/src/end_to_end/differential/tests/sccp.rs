//! Cases aimed at sparse conditional constant propagation
//! (`hir-transform/src/sccp.rs`): the merge kinds its lattice can see, the
//! results it can fold, and the dead edges its `DeadCodeAnalysis` could prove.
//!
//! The reach fact that shapes the whole module, measured on this branch on
//! 2026-09-17: **SCCP never sees a block argument in a plain-Rust guest.** It
//! runs third in the function pipeline (Canonicalizer, CSE, SCCP,
//! SinkOperandDefs, Local2Reg, …), so wasm locals are still `hir.store_local` /
//! `hir.load_local` traffic it has no memory model for, and the only values it
//! could join at a merge are block arguments. Two independent measurements say
//! there are none:
//!
//! * Across all 1304 harness-built guest wasms, the ONLY block-result construct
//!   LLVM's wasm backend emits is `loop (result i32)` (108 occurrences in 106
//!   files) — zero `block (result …)`, zero `if (result …)`. Every `if`, every
//!   `match` and every loop-carried value merges through a wasm LOCAL.
//! * The block arguments the frontend does build — `translate_loop`'s exit
//!   block and the synthetic function-exit block — are gone before SCCP runs:
//!   [`loop_result`] enters the first canonicalizer with four of them and
//!   leaves with none, two as empty predecessor-less blocks and the rest merged
//!   away by `simplify-br-to-block-with-single-predecessor`.
//!
//! So SCCP's lattice join is never exercised at a merge here, and the pass is a
//! no-op on every case below: the op histogram of the `entrypoint` body is
//! identical before and after it, and the only thing it does is re-unique each
//! function's existing `arith.constant`s one-for-one. Every case therefore
//! carries its evidence in the doc comment and earns its keep as a VALUE check
//! of the merge it builds — the merges are real, they are just carried by
//! locals, and a wrong one is a silent wrong answer.
//!
//! Evidence commands (one pass per run):
//! `-Z print-ir-after-pass=sparse-conditional-constant-propagation` with
//! `MIDENC_TRACE='pass:sparse-conditional-constant-propagation=trace,rewriter=trace'`,
//! and `wasm-tools print` on the harness-built guest for the merge kind.

use super::super::harness::{run_case, run_case_with_inputs};

/// Constant on one edge, computed value on the other — the "constant meets
/// overdefined" join, and the smallest merge LLVM cannot turn into a `select`
/// (the else arm ends in an `#[inline(never)]` call, and the then arm holds a
/// second call behind `black_box` so both blocks survive).
///
/// Wasm: the merge is `local 1` — `i32.const 5; local.set 1` on one edge,
/// `call $…opaque; local.set 1` on the other, `local.get 1` after the `end`. No
/// `block (result i32)`.
/// SCCP: `entrypoint` has 3 blocks and 0 block arguments; 55 ops in, 55 out;
/// no activity beyond re-uniquing 8 `arith.constant`s.
#[test]
fn if_merge() {
    run_case("sccp_if_merge", include_str!("../cases/case_sccp_if_merge.rs"));
}

/// Pinned grid for [`if_merge`]: `input2`'s low bit selects the edge, so these
/// pairs take the constant edge and the computed edge at each `input1`
/// boundary (0, 1, `i32::MIN`, `u32::MAX`).
#[test]
fn if_merge_edges() {
    run_case_with_inputs(
        "sccp_if_merge_edges",
        include_str!("../cases/case_sccp_if_merge.rs"),
        &[
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (0x8000_0000, 0),
            (0x8000_0000, 1),
            (u32::MAX, 0),
            (u32::MAX, 1),
        ],
    );
}

/// Four different constants meeting at one merge, via a dense `match` that
/// stays a `br_table`: each arm also writes a computed value into a second
/// merge, so LLVM keeps four distinct blocks instead of folding the constants
/// into a select chain.
///
/// Wasm: `br_table 1 (;@4;) 2 (;@3;) 3 (;@2;) 0 (;@5;) 1 (;@4;)` — four arm
/// targets plus a default that repeats one of them. Both merges are locals
/// (`local.set 1` for the constant, `local.set 0` for the computed value in
/// every arm), no `block (result i32)`.
/// SCCP: 5 blocks, 0 block arguments, 84 ops in and out, 9 constants
/// re-uniqued, nothing folded.
#[test]
fn switch_merge() {
    run_case("sccp_switch_merge", include_str!("../cases/case_sccp_switch_merge.rs"));
}

/// Pinned grid for [`switch_merge`]: one pair per `br_table` arm (`input2 & 3`
/// = 0..=3) at two `input1` values, so each of the four constants is the
/// selected one under both a zero and a saturated operand.
#[test]
fn switch_merge_edges() {
    run_case_with_inputs(
        "sccp_switch_merge_edges",
        include_str!("../cases/case_sccp_switch_merge.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (u32::MAX, 0),
            (u32::MAX, 1),
            (u32::MAX, 2),
            (u32::MAX, 3),
            (0x8000_0000, 2),
        ],
    );
}

/// A merge feeding a merge: an inner `if`-with-result whose both arms are
/// constants is one edge of an outer `if`-with-result whose other edge is
/// computed. If the inner merge ever became a block argument, SCCP would have
/// a two-constant join feeding a second join one level up.
///
/// Wasm: both merges are locals; LLVM keeps the inner `if` because each of its
/// arms holds a distinct opaque call.
/// SCCP: 5 blocks, 0 block arguments, 78 ops in and out, 11 constants
/// re-uniqued.
#[test]
fn nested_merge() {
    run_case("sccp_nested_merge", include_str!("../cases/case_sccp_nested_merge.rs"));
}

/// Pinned grid for [`nested_merge`]: `input2`'s two low bits pick the outer
/// edge and then the inner one, so all four paths are asserted.
#[test]
fn nested_merge_edges() {
    run_case_with_inputs(
        "sccp_nested_merge_edges",
        include_str!("../cases/case_sccp_nested_merge.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (u32::MAX, 0),
            (u32::MAX, 2),
            (0x8000_0000, 1),
            (0x8000_0000, 3),
        ],
    );
}

/// Four merges of four different types in one function (u32, u64, bool, i32),
/// built so the same numeric values recur across them. The point is the
/// materialized constants: `OperationFolder`'s uniquing key is
/// `(dialect, value, type)`, and a type-blind key would be an F18-shaped
/// wrong-width push that no IR dump shows.
///
/// Wasm: all four merges are locals (`local 3`, `local 4`, `local 6`,
/// `local 1`); LLVM pre-combines the arms' constants into the sums
/// (`i32.const 10` / `i32.const 14` for the first one), which does not change
/// the merge kind.
/// SCCP: 12 blocks, 0 block arguments, 187 ops in and out, 14 constants
/// re-uniqued. The key IS type-aware in practice: the post-SCCP `entrypoint`
/// carries `12 : i32` beside `12 : u32` and `4 : i32` beside `4 : u32` as
/// separate `arith.constant`s (see [`const_ops`] for four such pairs).
#[test]
fn typed_merges() {
    run_case("sccp_typed_merges", include_str!("../cases/case_sccp_typed_merges.rs"));
}

/// Pinned grid for [`typed_merges`]: `input2`'s four low bits select the four
/// merges' edges independently, so these pairs cover all-then, all-else and
/// six mixtures.
#[test]
fn typed_merges_edges() {
    run_case_with_inputs(
        "sccp_typed_merges_edges",
        include_str!("../cases/case_sccp_typed_merges.rs"),
        &[
            (0, 0),
            (0, 15),
            (1, 5),
            (1, 10),
            (0x8000_0000, 3),
            (0x8000_0000, 12),
            (u32::MAX, 7),
            (u32::MAX, 8),
        ],
    );
}

/// Three arms with three different side effects that all yield the SAME
/// constant — the one join that may legally become a constant rather than
/// overdefined.
///
/// Wasm: there is no merge left to join. LLVM's InstSimplify folds the
/// identical-incoming phi to the constant before the wasm is emitted, so only
/// the three calls' blocks remain and the tail is an unconditional
/// `local.get 0; i32.const 1; i32.or; i32.const 5; i32.mul`. This is the reason
/// the all-edges-agree join has no plain-Rust producer.
/// SCCP: 4 blocks, 0 block arguments, 80 ops in and out, 10 constants
/// re-uniqued.
#[test]
fn same_const() {
    run_case("sccp_same_const", include_str!("../cases/case_sccp_same_const.rs"));
}

/// Pinned grid for [`same_const`]: `input2 % 3` picks the arm, so all three
/// side-effect blocks run at two `input1` boundaries.
#[test]
fn same_const_edges() {
    run_case_with_inputs(
        "sccp_same_const_edges",
        include_str!("../cases/case_sccp_same_const.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (u32::MAX, 3),
            (u32::MAX, 4),
            (u32::MAX, 5),
            (0x8000_0000, 6),
            (1, 7),
        ],
    );
}

/// The only shape in this corpus whose wasm carries a block RESULT, and hence
/// the only one for which the frontend builds an HIR block with an ARGUMENT:
/// three `break`s falling into one tail expression that is also the function's
/// result, which LLVM tail-duplicates into a `return` inside the loop, leaving
/// the loop's `end` unreachable and typed `loop (result i32)`.
///
/// Wasm: 1 `loop (result i32)`.
/// Canonicalizer (`-Z print-ir-after-pass=canonicalizer`): `entrypoint` enters
/// the FIRST canonicalizer with 16 blocks and 4 block arguments and leaves with
/// 0. Two of the four are empty, predecessor-less blocks (`^block11(%32: i32)`,
/// `^block13(%44: i32)` — the loop exit and the synthetic function-exit block);
/// the others are merged away by
/// `simplify-br-to-block-with-single-predecessor`, which logs
/// `merging ^blockN into ^blockM replacing uses of its block arguments`.
/// SCCP therefore still sees 14 blocks and 0 block arguments; 132 ops in and
/// out, 16 constants re-uniqued.
#[test]
fn loop_result() {
    run_case("sccp_loop_result", include_str!("../cases/case_sccp_loop_result.rs"));
}

/// Pinned grid for [`loop_result`]: `input2 % 53` sets the trip limit, so these
/// pairs cover the one-trip loop, a mid-length one and the longest one, each
/// leaving through a different one of the three `break`s.
#[test]
fn loop_result_edges() {
    run_case_with_inputs(
        "sccp_loop_result_edges",
        include_str!("../cases/case_sccp_loop_result.rs"),
        &[
            (0, 0),
            (1, 0),
            (1, 52),
            (0x8000_0000, 26),
            (u32::MAX, 1),
            (u32::MAX, 52),
            (0x243f_6a88, 13),
            (7, 5),
        ],
    );
}

/// Results rather than block arguments: every operator here reaches the wasm
/// with a literal operand, and several are expanded by the wasm frontend into
/// chains whose own extra operands are constants too (the rotate's `32 - count`
/// as a `u32`, the unsigned compares' bitcasts, the signed `%`'s expansion).
/// Those late-created constant-operand ops are the only candidates for SCCP to
/// fold a result the canonicalizer's folder did not already reach.
///
/// SCCP: `entrypoint` is a single block, 120 ops in and 120 out — nothing
/// folds, the canonicalizer got there first. 20 constants re-uniqued, and
/// FOUR of them are same-value/different-type pairs that stay separate:
/// `5 : i32` / `5 : u32`, `16 : i32` / `16 : u32`, `12 : i32` / `12 : u32`,
/// `8 : i32` / `8 : u32`. That is the (value, type) uniquing key doing its job.
#[test]
fn const_ops() {
    run_case("sccp_const_ops", include_str!("../cases/case_sccp_const_ops.rs"));
}

/// The 64-bit half of [`const_ops`]: literal shift and rotate counts, which the
/// frontend truncates to `u32` with its own materialized constants, so the wide
/// expansions carry mixed-width constant operands.
///
/// SCCP: single block, 132 ops in and out, nothing folded, 19 constants
/// re-uniqued — including `3 : u32` beside `3 : u64` and `1 : i64` beside the
/// `u32`-typed ones, so no width is lost in the materialization.
#[test]
fn const_wide() {
    run_case("sccp_const_wide", include_str!("../cases/case_sccp_const_wide.rs"));
}

/// The textbook dead-code shape: a flag that is the same constant on every edge
/// of a merge, used as a later branch condition.
///
/// It has no plain-Rust producer, and this case is the evidence. LLVM folds
/// `phi(1, 1)` to `1` and deletes the second `if` outright: the wasm's tail is
/// an unconditional `local.get 0; i32.const 3; i32.mul; local.get 1; i32.add`,
/// with no trace of the `0xdead_beef` arm. What is left is the flag's producing
/// merge, carried by a local whose value is then unused. SCCP: 3 blocks, 0
/// block arguments, 65 ops in and out, 9 constants re-uniqued.
///
/// The closure this case documents: a constant SCCP could see must be visible
/// in the same SSA graph LLVM optimized, so LLVM has already used it; hiding it
/// from LLVM (`black_box`, an opaque helper, a `static`) hides it from SCCP too.
#[test]
fn dead_flag() {
    run_case("sccp_dead_flag", include_str!("../cases/case_sccp_dead_flag.rs"));
}

/// Pinned grid for [`dead_flag`]: `input2`'s low bit picks the arm that
/// produces the flag, at four `input1` boundaries.
#[test]
fn dead_flag_edges() {
    run_case_with_inputs(
        "sccp_dead_flag_edges",
        include_str!("../cases/case_sccp_dead_flag.rs"),
        &[
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (0x8000_0000, 0),
            (0x8000_0000, 1),
            (u32::MAX, 0),
            (u32::MAX, 1),
        ],
    );
}

/// A five-target `br_table` whose DEFAULT arm computes a chain of values only
/// that arm consumes — the "dead block defining a value used only by other dead
/// blocks" shape.
///
/// It is not dead to anybody: the selector goes through `black_box` before the
/// `match`, so LLVM cannot prove `& 3` bounds it and keeps the default arm
/// (`br_table 1 2 3 4 0` with the arm's body intact), and SCCP cannot prove it
/// either — the selector is a `hir.load` of a shadow-stack slot, not a
/// constant. Remove the `black_box` and LLVM deletes the arm before the
/// compiler sees it; that is the whole dilemma of this class.
/// SCCP: 6 blocks, 0 block arguments, 93 ops in and out, 13 constants
/// re-uniqued. Kept as a value check of the five-target table, and as the
/// evidence that a `cf.switch` carries no successor operands here either:
/// `cf.switch %76 [#builtin.u32<0> -> ^block19, …], ^block20 : (u32)`, every
/// target bare.
#[test]
fn dead_arm() {
    run_case("sccp_dead_arm", include_str!("../cases/case_sccp_dead_arm.rs"));
}

/// Pinned grid for [`dead_arm`]: one pair per live `br_table` target
/// (`input2 & 3` = 0..=3) at two `input1` boundaries.
#[test]
fn dead_arm_edges() {
    run_case_with_inputs(
        "sccp_dead_arm_edges",
        include_str!("../cases/case_sccp_dead_arm.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (u32::MAX, 0),
            (u32::MAX, 1),
            (u32::MAX, 2),
            (u32::MAX, 3),
        ],
    );
}
