//! Scale-stress shapes: long blocks, wide dispatch, deep nesting/calls, many segments.

use super::super::harness::run_case;

/// ~400 non-reassociable mixed u32/u64 ops in one basic block (four
/// right-leaning sub/rotate/xor waves plus select combiners) — operand
/// scheduling and single-block spill/reload at ~15x the corpus scale.
#[test]
fn chain300() {
    run_case("chain300", include_str!("../cases/case_chain300.rs"));
}

/// A single dense `match h & 63` with 64 structurally-varied arms — wasm
/// `br_table` with 64 targets and switch lowering at 8x the corpus's
/// previous width.
#[test]
fn match64() {
    run_case("match64", include_str!("../cases/case_match64.rs"));
}

/// Twelve levels of mixed while-loop/conditional nesting with all state
/// threaded through every level — cfg-to-scf structural recursion and scf
/// region nesting at depth.
#[test]
fn deep_nest() {
    run_case("deep_nest", include_str!("../cases/case_deep_nest.rs"));
}

/// Thirty #[inline(never)] helpers: a 20-deep non-recursive call chain with
/// per-level fan-out to ten leaves — 30 MAST procedure digests and a
/// 21+-frame VM call stack at runtime.
#[test]
fn call_web() {
    run_case("call_web", include_str!("../cases/case_call_web.rs"));
}

/// 24 odd-size statics (u8/u16/u32/u64 tables), three restored AtomicU32
/// mutables, and a ~4KB const-generated table — data-segment layout and
/// runtime reads at segment counts the corpus never had.
#[test]
fn seg24() {
    run_case("seg24", include_str!("../cases/case_seg24.rs"));
}
