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

/// Two sequential diamonds with wide mixed-width (u64/u32) arm trees over the
/// same locals — spill uses inside two scf regions, sibling-arm reloads
/// joined by phis at two joins, and size tie-breaking among spill candidates.
#[test]
fn spill_twin() {
    run_case("spill_twin", include_str!("../cases/case_spill_twin.rs"));
}

/// Reproducer for a compile-time spill-transform panic: each arm calls a
/// non-inlinable helper, spills the call result under wide-tree pressure,
/// then yields it, so the spilled value crosses the control-flow edge as the
/// arm's result. A second shape (nested wide diamonds) hits the same panic.
#[test]
fn spill_edge() {
    run_case("spill_edge", include_str!("../cases/case_spill_edge.rs"));
}
