//! Operand-stack pressure, spill analysis/transform, and select scheduling.

use super::super::harness::run_case;

/// Reused-condition selects with operands live past them plus a u64 select —
/// exercises dup/mov select emitter scheduling variants.
#[test]
fn select_sched() {
    run_case("select_sched", include_str!("../cases/case_select_sched.rs"));
}

/// Right-leaning single-use expression tree — ~20 simultaneously-live
/// operand-stack values, exercising spill analysis/transform.
#[test]
fn stack_pressure() {
    run_case("stack_pressure", include_str!("../cases/case_stack_pressure.rs"));
}

/// Ten u64s (20 felts) live across a branch and partially past its join —
/// CFG-form spills/reloads across control-flow edges and phi insertion
/// (`rewrite_cfg_spills`/`insert_required_phis`), beyond the single-block
/// spill path stack_pressure covers.
#[test]
fn spill_branch() {
    run_case("spill_branch", include_str!("../cases/case_spill_branch.rs"));
}

/// Ten u64s (20 felts) live across every iteration of a loop (loop-variant
/// rotates defeat LICM) and past its exit — loop-header spill placement
/// (`compute_w_entry_loop`), backedge/exit-edge reload reconciliation, and
/// loop-pressure heuristics.
#[test]
fn spill_loop() {
    run_case("spill_loop", include_str!("../cases/case_spill_loop.rs"));
}

/// Sixteen masked rotate counts shared between pre-loop code and rotates of
/// the loop-carried accumulator (CSE merges the translator's count bands
/// into cross-block SSA values) — 18 felts alive at the loop header drive
/// the W^entry over-capacity arm, with edge splits on the preheader edge
/// and the loop backedge; a second light loop carries two more shared
/// counts across it.
#[test]
fn spill_loop_mix() {
    run_case("spill_loop_mix", include_str!("../cases/case_spill_loop_mix.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-08-27): building this case
/// panics with `attempt to subtract with overflow` in `Stack::movdn` at
/// codegen/masm/src/opt/operands/stack.rs:80. Trigger: LLVM runtime-unrolls
/// the `% 97`-bounded round `acc = (acc.wrapping_mul(33) ^ i).rotate_left(5)`
/// 4x, whose live state spills. Root cause (triage 2026-08-27): the SAME
/// TransformSpills defect as `unroll_chain` (control_flow.rs) —
/// `insert_required_phis` seeds the loop-bypass edge with non-dominating
/// spilled values, leaving SSA-invalid IR that cfg-to-scf threads into a
/// sibling-region `scf.yield %310, %309, %342` of which only `%342` is on
/// the operand stack; the scheduler is handed this UNSATISFIABLE arity-3
/// problem and `MoveDownAndSwap` runs `movdn` past the end of its stack
/// model. Not a solver defect and provably not a miscompile risk (a live
/// use of the poisoned phi args would have been invalid SSA before the
/// pass, so they are always dead; the solver matches values by identity and
/// validates accepted solutions). Bounded by: the xor-rotl and mul-rotl
/// rounds of the identical loop compile and pass; the rotate-less round is
/// the arity-2 symptom (`unroll_chain`, NoSolution at lowering.rs:109).
/// Compile-time — no inputs involved. Un-ignore when this case compiles.
#[test]
#[ignore = "compiler panic: 'attempt to subtract with overflow' in Stack::movdn at \
            codegen/masm/src/opt/operands/stack.rs:80 while applying the scheduler solution for \
            the 4x-unrolled mul-xor-rotl loop chain (compile-time, no inputs involved)"]
fn unroll_rotmix() {
    run_case("unroll_rotmix", include_str!("../cases/case_unroll_rotmix.rs"));
}

/// Six shared masked rotate counts crossing a dense 6-way `match` inside a
/// `% 97`-bounded loop — spilled values crossing scf.index_switch arm
/// edges, per-arm edge reconciliation, and multi-region successor
/// traversals under TransformSpills' liveness walks. (The ten-count version
/// of this shape hits the known NoSolution panic on an arity-2 rotl —
/// see the scratch log; unroll_chain is the documented reproducer.)
#[test]
fn spill_switch() {
    run_case("spill_switch", include_str!("../cases/case_spill_switch.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-08-27): building this case
/// panics with `NoSolution` at codegen/masm/src/lower/lowering.rs:109 while
/// scheduling `arith.rotl %167, %397` (constraints `[Move, Copy]`) over a
/// 15-felt operand stack. Root cause (triage 2026-08-27, distinct from
/// `unroll_chain`): for arity-2 problems the solver pushes ONLY the
/// `TwoArgs` tactic (codegen/masm/src/opt/operands/solver.rs, the
/// `is_binary` branch); its fixed `dup(copy)` + `movup(move)` pattern here
/// requires a 17-felt stack access, the solution is rejected by the 16-felt
/// MASM window check, and there is no fallback tactic — although a valid
/// in-window schedule exists (`MoveUp(7), Copy(7), Swap(1)`, exactly what
/// `LinearStackWindow` + `Linear` produce for the same problem at other
/// arities). The problem is in-contract: spill analysis correctly capped
/// live pressure at 15 felts <= K=16. This is the ten-count variant of the
/// passing `spill_switch` (six counts) — the boundary is copy-constraint
/// depth, not unrolling; no unrolled chain and no invalid IR are involved
/// (unlike `unroll_chain`/`unroll_rotmix`, whose panics stem from
/// TransformSpills-produced non-dominating phi operands). Un-ignore when
/// this case compiles (binary problems get a window-aware fallback tactic).
#[test]
#[ignore = "compiler panic: 'with error: NoSolution' at codegen/masm/src/lower/lowering.rs:109 \
            scheduling an arity-2 arith.rotl with a Copy-constrained count at the bottom of a full \
            15-felt window (TwoArgs-only tactic list, no window-aware fallback; compile-time, no \
            inputs involved)"]
fn rotl_window() {
    run_case("rotl_window", include_str!("../cases/case_rotl_window.rs"));
}

/// u32 variant of the unrolled mul-xor-rotate round — schedulable single-
/// felt interleaved chains pressing the scheduler tactic interiors (the u64
/// twins are the ignored unroll_chain / unroll_rotmix panics).
#[test]
fn unroll_u32() {
    run_case("unroll_u32", include_str!("../cases/case_unroll_u32.rs"));
}

/// Two sequential diamonds with wide mixed-width (u64/u32) arm trees over the
/// same locals — spill uses inside two scf regions, sibling-arm reloads
/// joined by phis at two joins, and size tie-breaking among spill candidates.
#[test]
fn spill_twin() {
    run_case("spill_twin", include_str!("../cases/case_spill_twin.rs"));
}

/// Asymmetric-pressure diamond: cross-edge SSA values (CSE-merged masked
/// rotate-count bands shared by both arms and the join) are spilled in the
/// heavy arm only, so control-flow edge reconciliation records edge splits
/// — a reload split on the heavy edge and a compensating spill split on the
/// cheap edge — driving `SpillAnalysis::split` and the transform's
/// split-materialization loop (`Placement::Split` insertion and branch
/// redirection).
#[test]
fn spill_split() {
    run_case("spill_split", include_str!("../cases/case_spill_split.rs"));
}

/// Each arm calls a non-inlinable helper, spills the call result under
/// wide-tree pressure, then yields it, so the spilled value crosses the
/// control-flow edge as the arm's result. A second shape (nested wide
/// diamonds) exercises the same edge-split path.
///
/// Formerly `#[ignore]`d as a compile-time panic reproducer (i1289:
/// TransformSpills `convert_reload_to_load` unwrapped None on a spilled
/// value crossing a CF edge as a successor arg / scf.yield operand);
/// re-verified compiling and passing differentially 2026-08-27 — kept as
/// the regression guard for the edge-split spill cluster.
#[test]
fn spill_edge() {
    run_case("spill_edge", include_str!("../cases/case_spill_edge.rs"));
}
