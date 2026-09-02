//! Cases pinned to a non-default guest optimization level. Every test here
//! passes its own `midenc` flags through `run_case_with_flags` (currently
//! `--optimize=size-min`, LLVM `-Oz` for the guest wasm), so the shapes it
//! asserts never depend on `MIDENC_DIFF_FLAGS`. The native reference build
//! is not affected by the flags: outputs must match under every guest
//! opt-level, so a divergence here is a real compiler bug. Under a
//! `MIDENC_DIFF_FLAGS` sweep the pinned flags win for the same option, so
//! these cases keep their opt-level while other env flags still apply.
//!
//! Why -Oz: at opt-level 2 LLVM unrolls small loops, inlines every helper,
//! and expands constant-size copies inline, so the rest of the corpus never
//! produces kept loops, real helper calls, or constant-length
//! `memory.copy`/`memory.fill`. These cases keep those guest shapes in the
//! committed suite (see KNOWLEDGE.md, "Compiler-configuration axes").

use super::super::harness::run_case_with_flags;

/// Flags shared by every case in this module: LLVM `-Oz` for the guest.
const SIZE_MIN: &[&str] = &["--optimize=size-min"];

/// Passing guard at the -Oz count-band window boundary: the `spill_loop_mix`
/// shape (masked rotate counts shared between pre-loop code and rotates of
/// the loop-carried accumulator, plus the 28/30 live-through pair and a light
/// second loop) with NINE shared counts. At -Oz ten or more shared counts hit
/// the known arity-2 `NoSolution` panic (the ignored `spill_loop_mix_oz` in
/// `spills.rs` is the sixteen-count reproducer); nine is the largest count
/// that compiles, and the same source also passes at O2. At -Oz the count
/// bands stay un-hoisted, so the nine dead bands are dropped AFTER the loop
/// (the post-op drop site's used/unused interleave arms).
#[test]
fn band_guard_oz() {
    run_case_with_flags("band_guard_oz", include_str!("../cases/case_band_guard_oz.rs"), SIZE_MIN);
}

/// Small constant-trip loops (4-trip fill, early-`break` scan, 3x4 nested
/// counted loops with a u64 accumulator, 8-trip `continue` loop) that O2
/// unrolls and -Oz keeps: six kept loops lift to `scf.while` ops whose
/// exit-dispatch `scf.index_switch` continuations carry unused result
/// columns — the only producer of the `WhileUnusedResult` and
/// `IndexSwitchRemoveUnusedResults` rewrite interiors (the O2 corpus only
/// reaches their bail paths).
#[test]
fn loop_keep_oz() {
    run_case_with_flags("loop_keep_oz", include_str!("../cases/case_loop_keep_oz.rs"), SIZE_MIN);
}

/// Helpers without inline attributes called from several sites: -Oz keeps
/// `mix` and `pick` (three return sites) as real calls with u32/u64 values
/// live across the call boundaries, where O2 inlines everything into one
/// function. Zero-delta guest-shape guard for -Oz call marshalling.
#[test]
fn helper_calls_oz() {
    run_case_with_flags(
        "helper_calls_oz",
        include_str!("../cases/case_helper_calls_oz.rs"),
        SIZE_MIN,
    );
}

/// Constant-length aggregate copies above LLVM's size-mode inline-store
/// threshold: 48-byte struct copies and a 64-byte zero-init lower to four
/// `memory.copy` and one `memory.fill` with immediate lengths at -Oz (O2:
/// inline i64 load/store pairs), read back at runtime indexes. Zero-delta
/// guest-shape guard: the memcpy/memset lowerings treat the length as a
/// runtime operand, so no new emitter arm exists, but the inputs are new.
#[test]
fn mem_libcalls_oz() {
    run_case_with_flags(
        "mem_libcalls_oz",
        include_str!("../cases/case_mem_libcalls_oz.rs"),
        SIZE_MIN,
    );
}

/// A dense 8-arm `match` inside a 5-trip loop plus a sparse 3-arm match in a
/// 4-trip loop: -Oz keeps both loops and emits ONE in-loop `br_table` with a
/// loop-carried selector (O2: nine unrolled br_tables, no loop). Zero-delta
/// guest-shape guard for br_table-in-kept-loop dispatch.
#[test]
fn switch_loop_oz() {
    run_case_with_flags(
        "switch_loop_oz",
        include_str!("../cases/case_switch_loop_oz.rs"),
        SIZE_MIN,
    );
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, 2026-09-02, campaign 11), -Oz
/// twin of `control_flow::deep_nest_overflow`: the eight-level loop nest
/// that passes at the default opt-level (`control_flow::nest8`) panics at
/// -Oz with `implicit operand stack overflow along incoming control flow
/// edges of after(scf.if in ^block129)` at
/// hir-analysis/src/analyses/spills.rs:1533. At -Oz LLVM keeps every level
/// as a loop (no peeling of the `% 3 + 1` levels), so cfg-to-scf's
/// exit-dispatch result columns reach the 16-felt budget one nesting level
/// earlier than at O2: the generated `% 3` nests fail from depth SEVEN at
/// -Oz (six passes) versus depth nine at O2. Compile-time, no inputs
/// involved. Un-ignore together with `deep_nest_overflow`.
#[test]
#[ignore = "compiler panic: 'implicit operand stack overflow along incoming control flow edges of \
            after(scf.if in ^block129)' at hir-analysis/src/analyses/spills.rs:1533 at -Oz — \
            cfg-to-scf exit-dispatch result columns of the eight-deep nest exceed the 16-felt \
            budget (compile-time, no inputs involved)"]
fn nest8_oz() {
    run_case_with_flags("nest8_oz", include_str!("../cases/case_nest8.rs"), SIZE_MIN);
}

/// u64 checked/saturating/overflowing arithmetic, signed and unsigned compare
/// chains, and guarded unsigned div/rem inside a 6-trip loop. The loop stays
/// a loop at O2 as well (the body is too heavy to unroll), so the -Oz shape
/// difference is only in the loop body's scheduling — which is enough to
/// reach post-op drop arms and the last `OpEmitter::swap` arms no other case
/// warms. Kept as the -Oz wide-arithmetic differential guard.
#[test]
fn u64_checks_oz() {
    run_case_with_flags("u64_checks_oz", include_str!("../cases/case_u64_checks_oz.rs"), SIZE_MIN);
}
