//! Debug-info cluster cases. Every differential guest builds with FULL DWARF
//! (harness `debug = 2` + package retention), so these cases drive the DWARF
//! decode surface (`frontend/wasm/src/module/debug_info.rs`), location
//! schedules (`function_builder_ext.rs`), the debuginfo dialect, and their
//! interactions with DCE/Local2Reg — while stressing the invariant that debug
//! info never changes program semantics.

use super::super::harness::{run_case, run_case_with_inputs};

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

// Local2Reg shapes (campaign 24, 2026-09-10). Guest DWARF is what decides
// whether the pass runs at all on a given slot: a DWARF declare over a
// candidate local makes `convert_debug_references_for_local` return false
// (rustc emits the two-op `[WasmLocal(N), StackValue]` form, which
// `declares_are_safe` rejects), so the stores are preserved; without DWARF
// there are no declares and the early "no debug references" return lets every
// eligible slot through. A user's `cargo miden build` emits no DWARF, so the
// release pipeline is the promoting one.
//
// Measured promotion over these seven cases
// (`MIDENC_TRACE='pass:local2reg=trace'`; the pass logs "found promotable
// local X" BEFORE the declare check, so the count actually promoted is
// "found" minus "declare-blocked"):
//
//   case            debug 2         debug 0    structurally rejected
//   l2r_hotloop     0  (0 found)    0          3 loaded more than once
//   l2r_matcharms   0  (0 found)    0          3 loaded >1, 1 stored >1
//   l2r_nested      0  (0 found)    0          5 loaded >1, 4 other block
//   l2r_livecall    0  (5 found)    5          4 loaded >1, 2 control flow
//   l2r_byref       1  (4 found)    4          3 loaded more than once
//   l2r_array4      3  (4 found)    4          4 loaded more than once
//   l2r_params      0  (14 found)   14         4 loaded >1, 2 control flow
//
// So the release build promotes 27 slots across these guests where the DWARF
// build promotes 4 — and every case still computes the same answer at
// `FUZZA_GUEST_DEBUG=0`, `=1` and the default `=2`. A promotion that changed a
// program's answer would fail here rather than in a user's release build.
// (The dead-store-erasure arm is NOT debug-gated: `l2r_params`'
// `l2r_unused_arg` reaches it at both levels, because an unused parameter gets
// no DWARF variable to block the conversion.)

/// A u32 and a u64 accumulator read five times per iteration of a hot loop
/// (many `local.get`s, one `local.set` per backedge): Local2Reg's
/// single-load/single-store precondition rejects both at every debug level
/// ("loaded more than once"), and only the pre-loop seeds are promotable.
#[test]
fn l2r_hotloop() {
    run_case("l2r_hotloop", include_str!("../cases/case_l2r_hotloop.rs"));
}

/// Zero-, one- and many-trip covers of [`l2r_hotloop`]'s loop.
#[test]
fn l2r_hotloop_edges() {
    run_case_with_inputs(
        "l2r_hotloop_edges",
        include_str!("../cases/case_l2r_hotloop.rs"),
        &[
            (0, 0),
            (1, 1),
            (0xffff_ffff, 0xffff_ffff),
            (0x8000_0000, 19),
            (7, 38),
            (0x7fff_ffff, 18),
        ],
    );
}

/// One local assigned in each of eight `br_table` arms and read at the join:
/// "stored more than once" rejects it at every debug level, so the arm-local
/// temporaries are the only promotable slots in the dispatch.
#[test]
fn l2r_matcharms() {
    run_case("l2r_matcharms", include_str!("../cases/case_l2r_matcharms.rs"));
}

/// One pinned pair per `match` arm of [`l2r_matcharms`].
#[test]
fn l2r_matcharms_edges() {
    run_case_with_inputs(
        "l2r_matcharms_edges",
        include_str!("../cases/case_l2r_matcharms.rs"),
        &[
            (0, 1),
            (1, 0xffff_ffff),
            (2, 0x8000_0000),
            (3, 0x7fff_ffff),
            (4, 255),
            (5, 0x1_0000),
            (6, 0),
            (7, 0xdead_beef),
        ],
    );
}

/// A local whose address is never taken beside one passed by `&mut` to an
/// `#[inline(never)]` helper: taking the address moves the slot out of the
/// wasm local space into the shadow stack, so it is not a `hir.store_local`
/// and no debug level can promote it.
#[test]
fn l2r_byref() {
    run_case("l2r_byref", include_str!("../cases/case_l2r_byref.rs"));
}

/// Values live ACROSS `#[inline(never)]` calls pinned by an opaque
/// `fetch_add(0)`: Local2Reg's heuristic 2 refuses any promotion whose store
/// and load are separated by a `CallOpInterface` op, so these slots keep
/// their store/load pair with and without DWARF.
#[test]
fn l2r_livecall() {
    run_case("l2r_livecall", include_str!("../cases/case_l2r_livecall.rs"));
}

/// A local reassigned in an inner loop and read again in the outer body,
/// beside a single-store/single-load inner temporary — the one slot in the
/// nest that Local2Reg can take. Erasing it must not change what the
/// backedge carries.
#[test]
fn l2r_nested() {
    run_case("l2r_nested", include_str!("../cases/case_l2r_nested.rs"));
}

/// Trip-count covers of [`l2r_nested`]'s two loops (2x2 up to 12x8).
#[test]
fn l2r_nested_edges() {
    run_case_with_inputs(
        "l2r_nested_edges",
        include_str!("../cases/case_l2r_nested.rs"),
        &[
            (0, 0),
            (1, 1),
            (11, 7),
            (0xffff_ffff, 0xffff_ffff),
            (10, 6),
            (0x8000_0000, 0x7fff_ffff),
        ],
    );
}

/// A constant-indexed `[u32; 4]` (SROA'd into four promotable scalars) beside
/// a runtime-indexed one (kept in the shadow stack as real loads/stores), so
/// both sides of the "is it a wasm local at all" question feed one answer.
#[test]
fn l2r_array4() {
    run_case("l2r_array4", include_str!("../cases/case_l2r_array4.rs"));
}

/// One pinned pair per runtime index of [`l2r_array4`]'s second array.
#[test]
fn l2r_array4_edges() {
    run_case_with_inputs(
        "l2r_array4_edges",
        include_str!("../cases/case_l2r_array4.rs"),
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0x8000_0000, 0x7fff_ffff),
        ],
    );
}

/// The reliable producer of promotable slots: parameters, which the frontend
/// stores unconditionally at entry. Four `#[inline(never)]` helpers with
/// three to five once-read parameters plus one never-read parameter (the
/// dead-store-erasure arm) make the promoted set as large as plain Rust can.
#[test]
fn l2r_params() {
    run_case("l2r_params", include_str!("../cases/case_l2r_params.rs"));
}
