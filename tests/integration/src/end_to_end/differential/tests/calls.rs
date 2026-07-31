//! Function-call boundaries: sret aggregates, at-limit signatures, call placement.

use super::super::harness::run_case;

/// Non-inlined helper calls (multi-arg, u64, bool) plus reused selects —
/// exercises call translation/lowering and select emitter variants.
#[test]
fn calls_selects() {
    run_case("calls_selects", include_str!("../cases/case_calls_selects.rs"));
}

/// Tuple/struct/array returns and big by-value params — the aggregate (sret)
/// call path: zero-result `hir.exec` with sret pointers into the caller's
/// frame (multi-value returns are impossible: no `+multivalue` in
/// cargo-miden's target features).
#[test]
fn sret_shapes() {
    run_case("sret_shapes", include_str!("../cases/case_sret_shapes.rs"));
}

/// 16-u32 and 8-u64 helper signatures — exactly 16 stack felts each, the
/// call-site scheduling limit (20 felts is a verified compile-time spills
/// panic) — with u64 values live across both call sites.
#[test]
fn wide_calls() {
    run_case("wide_calls", include_str!("../cases/case_wide_calls.rs"));
}

/// Zero-arg zero-result / zero-arg-with-result helpers plus calls inside a
/// loop body and both branches of a conditional — call ops with empty operand
/// lists (scheduling early return) and in non-entry regions.
#[test]
fn call_mix() {
    run_case("call_mix", include_str!("../cases/case_call_mix.rs"));
}
