//! Debug-info cluster cases. Every differential guest builds with FULL DWARF
//! (harness `debug = 2` + package retention), so these cases drive the DWARF
//! decode surface (`frontend/wasm/src/module/debug_info.rs`), location
//! schedules (`function_builder_ext.rs`), the debuginfo dialect, and their
//! interactions with DCE/Local2Reg — while stressing the invariant that debug
//! info never changes program semantics.

use super::super::harness::run_case;

/// Sequential rebinds + nested block shadows of named locals — multi-range
/// DWARF location lists, schedule declare/kill events.
#[test]
fn dbg_rebind() {
    run_case("dbg_rebind", include_str!("../cases/case_dbg_rebind.rs"));
}

/// Const-folded named locals (negative i64, above-felt u64) — constant DWARF
/// location expressions and their felt-range lowering guards.
#[test]
fn dbg_negconst() {
    run_case("dbg_negconst", include_str!("../cases/case_dbg_negconst.rs"));
}

/// Dead named computations — LLVM salvages their dbg.values into arithmetic
/// DWARF expressions the decoder must drop via its catch-all.
#[test]
fn dbg_salvage() {
    run_case("dbg_salvage", include_str!("../cases/case_dbg_salvage.rs"));
}

/// Loop-carried named accumulators + pre-loop value read only after the loop
/// — location ranges across backedges, debug vs real liveness.
#[test]
fn dbg_loop() {
    run_case("dbg_loop", include_str!("../cases/case_dbg_loop.rs"));
}

/// By-value struct params (pointer in local 0) with one- and two-field reads
/// — Local2Reg promotion vs preserved stores under DWARF declares.
#[test]
fn dbg_byval() {
    run_case("dbg_byval", include_str!("../cases/case_dbg_byval.rs"));
}

/// Ten staggered simultaneously-live named locals across a branch and an
/// opaque call — dense schedules; DebugVar + Nop woven through scheduling.
#[test]
fn dbg_manylive() {
    run_case("dbg_manylive", include_str!("../cases/case_dbg_manylive.rs"));
}

/// Named u64 locals + shared count bands across an asymmetric diamond —
/// debug decorators interleaved with spill edge splits/reloads.
#[test]
fn dbg_spillmix() {
    run_case("dbg_spillmix", include_str!("../cases/case_dbg_spillmix.rs"));
}

/// Match-arm-scoped named locals over a dense br_table — arm-boundary
/// declare/kill schedule events through the switch lowering.
#[test]
fn dbg_match() {
    run_case("dbg_match", include_str!("../cases/case_dbg_match.rs"));
}
