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

/// Exercises wasm `call_indirect` (funcref table dispatch through function pointers).
#[test]
fn call_indirect() {
    run_case("call_indirect", include_str!("../cases/case_call_indirect.rs"));
}

/// Two fn-pointer arrays of different fn types dispatched at runtime — the one
/// funcref table holds entries with two distinct signature tags, so each
/// `hir.exec_indirect` call site must tag-filter the other signature's entries
/// (verifier/possible_callees skip arms) and the runtime tag check passes only
/// for its own; also the first u64-carrying indirect signature.
#[test]
fn indirect_sigs() {
    run_case("indirect_sigs", include_str!("../cases/case_indirect_sigs.rs"));
}

/// A user `#[no_mangle]` function named exactly `__indirect_function_table_0`
/// collides with the symbol the frontend generates for the lowered funcref
/// table, forcing the collision-rename (counter-bump) path in
/// `get_or_build_table` while dispatch still works through the renamed table.
#[test]
fn indirect_collision() {
    run_case("indirect_collision", include_str!("../cases/case_indirect_collision.rs"));
}

/// `dyn Trait` dispatch through runtime-selected trait objects: vtables are
/// `.rodata` arrays of funcref-table indices, each method call loads its
/// vtable slot and dispatches via `call_indirect` — a dispatch shape (vtable
/// slot load + receiver pointer argument) no fn-pointer-array sibling covers.
#[test]
fn dyn_trait() {
    run_case("dyn_trait", include_str!("../cases/case_dyn_trait.rs"));
}

/// Function pointers as first-class values: returned from / passed to
/// `#[inline(never)]` helpers, a loop-carried fn-pointer state machine, a
/// non-capturing closure coerced to `fn` (anonymous table entry), and fn-ptr
/// `==` (funcref-index comparison) — table-index data flow no sibling covers.
#[test]
fn fnptr_value() {
    run_case("fnptr_value", include_str!("../cases/case_fnptr_value.rs"));
}

/// Chained indirect dispatch — an indirect callee that itself dispatches
/// through a second fn-pointer array (nested `dynexec` frames) — plus
/// dispatch inside a loop and in a single branch arm.
#[test]
fn indirect_chain() {
    run_case("indirect_chain", include_str!("../cases/case_indirect_chain.rs"));
}

/// The widest accepted indirect signature — 7 u64 parameters (14 felts) plus
/// the table index fills 15 of the 16-element operand-stack window — dynexec
/// with a full argument window and u64 values crossing the dispatch boundary.
#[test]
fn indirect_wide() {
    run_case("indirect_wide", include_str!("../cases/case_indirect_wide.rs"));
}
