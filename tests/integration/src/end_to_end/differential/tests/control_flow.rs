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
