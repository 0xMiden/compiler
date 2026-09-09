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

use super::super::harness::{run_case_with_flags, run_case_with_flags_and_inputs};

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
///
/// Campaign-18 ladder around this shape (all rungs value-checked at -Oz):
/// the boundary tracks the TOTAL number of shared bands, not the shape of the
/// post-loop code. With `M` bands used after the loop and `N` dying inside
/// it, `N + M <= 11` compiles and `N + M >= 12` panics for `M = 1..4`; the
/// KIND of the first post-loop op (an arith op, a call to a kept helper, a
/// runtime-indexed store, a `select`, a second loop) does not move the
/// boundary by a single rung. The pre-loop DEFINITION ORDER of the bands
/// does: defining the dying bands first (as here) reaches 11, while
/// live-first and alternating orders cap at 10 and a live/dead/live sandwich
/// at 9. With `M = 0` there is no boundary at all (`drop_batch_oz`, twenty
/// bands).
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

/// Whole-stack batch drop at the pre-terminator program point: twenty count
/// bands are live from the pre-loop code through the loop body and dead at
/// the `return`, so the block emitter finds the ENTIRE operand stack unused
/// ("0 used operands out of 11") and takes `drop_unused_operands_at`'s
/// all-unused batch arm, which asserts that the batch covers the whole stack.
/// Boundary fact (campaign 18): with no post-loop use of any band this ladder
/// has no window boundary — twenty shared counts compile and pass, whereas
/// `band_guard_oz` (one band used after the loop) caps out at nine. Passes at
/// `--optimize=basic`, the default level and `--optimize=max` too.
#[test]
fn drop_batch_oz() {
    run_case_with_flags("drop_batch_oz", include_str!("../cases/case_drop_batch_oz.rs"), SIZE_MIN);
}

/// The post-op drop's SOLVER path: three dead count bands under six live ones
/// ("6 used operands out of 9"), so `drop_unused_operands_at` takes its
/// non-pathological branch and asks the operand scheduler to bring the unused
/// values to the top under all-`Move` constraints before `dropn`. Every other
/// drop-bearing case in the corpus has more dead operands than live ones and
/// takes the manual interleave (`band_guard_oz`) or whole-stack batch
/// (`drop_batch_oz`) arms instead. A wrong schedule here is a silent
/// miscompile, not a panic, which is why the shape is value-checked
/// differentially. Passes at every opt-level.
#[test]
fn drop_solver_oz() {
    run_case_with_flags(
        "drop_solver_oz",
        include_str!("../cases/case_drop_solver_oz.rs"),
        SIZE_MIN,
    );
}

/// Dead results of a multi-result op: four `mulhi` rounds lower to
/// `i64.mul_wide_u` whose LOW result is dead, so the emitter's
/// dead-instruction-result drop fires four times on a two-felt operand, under
/// a shelf of eight live u32 values. Closure fact (campaign 18): the dead
/// result is always at index 0, because the only plain-Rust producer of a
/// dead result is a wide op's unused low half — LLVM emits the narrow op
/// instead whenever the HIGH half is the dead one, so the non-zero-index
/// (swap/movup) arms of `drop_operand_at_position` have no producer at this
/// site. Passes at every opt-level.
#[test]
fn mulhi_dead_oz() {
    run_case_with_flags("mulhi_dead_oz", include_str!("../cases/case_mulhi_dead_oz.rs"), SIZE_MIN);
}

/// A helper -Oz keeps as a real call (LLVM's own size arithmetic, no inline
/// attributes) called from three sites, two of them inside a loop across
/// which five masked rotate count bands are live, with the arguments reused
/// after each call and the result consumed at depth. Call marshalling with
/// un-hoisted count bands under the argument window. Passes at every
/// opt-level.
#[test]
fn call_bands_oz() {
    run_case_with_flags("call_bands_oz", include_str!("../cases/case_call_bands_oz.rs"), SIZE_MIN);
}

/// Dead-fallthrough loop frame: an inner loop whose only exits are five
/// in-loop `return`s plus a `break`, nested in a kept outer loop, with four
/// count bands crossing both. This is the corpus's only producer of the
/// `SimplifyPassthroughCondBr` cf canonicalization — it rewrites ten times
/// here, interleaved with twelve `SplitCriticalEdges` rewrites, which is the
/// mechanism: splitting a critical edge leaves a passthrough block whose
/// target has a unique predecessor, satisfying the guard that made the
/// pattern look unreachable. `deadfall_oz_edges` pins one input per exit.
/// Passes at every opt-level.
#[test]
fn deadfall_oz() {
    run_case_with_flags("deadfall_oz", include_str!("../cases/case_deadfall_oz.rs"), SIZE_MIN);
}

/// Pinned exit grid for [`deadfall_oz`]: one native-verified input pair per
/// return site (tags 1..5 are the five in-loop returns, tag 6 is the outer
/// loop's normal exit), so every exit is asserted on every run rather than
/// only when the fuzzer happens to draw it.
#[test]
fn deadfall_oz_edges() {
    run_case_with_flags_and_inputs(
        "deadfall_oz_edges",
        include_str!("../cases/case_deadfall_oz.rs"),
        SIZE_MIN,
        &[(0, 21), (0, 1), (0, 0), (0, 12), (0, 56), (512, 122)],
    );
}
