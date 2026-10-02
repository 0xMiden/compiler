//! Cases whose value is the INTERACTION between the spill transform and the
//! canonicalization patterns that rewrite structured control flow.
//!
//! The function pass pipeline (`midenc-compile/src/pipeline/backend.rs`) runs
//! Canonicalizer → CSE → SCCP → SinkOperandDefs → Local2Reg → TransformSpills →
//! LiftControlFlowToSCF → Canonicalizer → SinkOperandDefs → TransformSpills, so
//! the post-lift canonicalizer rewrites regions on IR that the *pre-lift* spill
//! transform has already rewritten, and the post-lift SinkOperandDefs and
//! TransformSpills then run on the rewritten regions. Every case here is one of
//! the known canonicalization producers (`canon.rs`) loaded with spill freight:
//! CSE-merged masked rotate count bands used before the region, on the region's
//! carried value inside it, and after it, plus — where noted — a cluster of u64
//! values defined before the region, consumed in one wide expression inside it
//! and again afterwards, which is what pushes block pressure far enough for the
//! spill analysis to request real spills and split edges.
//!
//! A wrong interaction here is a SILENT MISCOMPILE — a reload dropped together
//! with a removed result column, a select hoisted across a reload, a spill slot
//! read on the wrong path after a passthrough collapse — so every case has an
//! `_edges` twin pinning one input per path, asserted against the native build
//! on every run rather than only when the fuzzer happens to draw it.
//!
//! Two facts hold across the whole module and are the reason these are guards
//! rather than findings. First, freight does not move the fire count of the
//! pattern each case targets: every one of them fires exactly as often as its
//! freight-free sibling in `canon.rs`, at both opt levels. (It does change
//! WHICH patterns fire at all — see `sink_spill` and `scan_spill`.) Second, the
//! freight is real — the spills trace
//! (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`) shows spills,
//! reloads and edge splits in every case, and the pass converts each reload to
//! a spill-slot `hir.load_local` ("convert reload to load"), which is the form
//! the post-lift canonicalizer then sees inside the regions it rewrites. Several
//! cases also carry the erased split-edge reloads and the dead "unused phi"
//! block arguments that the ignored `pressure::zero_trip_frontier` /
//! `pressure::zero_trip_overflow` reproducers blame their panics on: here the
//! same defects are present and the program still computes the right answer,
//! which is what these cases pin.

use super::super::harness::{run_case, run_case_with_inputs};

/// Column-removal cascade over regions that contain spill-slot reloads.
///
/// Two sequential 8-trip loops whose only in-body branch is an empty `continue`
/// arm — the sole producer of the `WhileUnusedResult` →
/// `IndexSwitchRemoveUnusedResults` cascade — carry eight shared rotate count
/// bands and an eight-u64 cluster across both loops. Fired counts, identical at
/// the default level and at `--optimize=size-min`: `while-unused-result` 2,
/// `index-switch-remove-unused-results` 2, `while-remove-unused-args` 2,
/// `split-critical-edges` 2, `convert-trivial-if-to-select` 4 — exactly the
/// per-loop counts `canon::col_cascade` gets with no freight at all, measured
/// over bands 2..12 without a single count changing.
///
/// Spill evidence: 57 spills, 84 reloads, six split control-flow edges, 52
/// reloads lowered to spill-slot loads and 32 of them erased again as unused —
/// the stale-dominator-tree erasure the `pressure::zero_trip_overflow` panic is
/// attributed to. The values still agree with native everywhere.
///
/// The interaction itself is visible in the IR: `-Z print-ir-after-pass=
/// local2reg,transform-spills` shows seventeen user locals before the pre-lift
/// spill pass and thirty-three after it, so every `local_variable` from 17 up
/// is a spill slot — and the dump taken after the POST-LIFT canonicalizer has
/// eighteen such slot accesses inside the two `scf.index_switch` regions that
/// `IndexSwitchRemoveUnusedResults` rebuilt.
///
/// Boundary (campaign 20): with the u64 cluster removed, this shape compiles at
/// 2, 4, 6, 8, 9, 12, 13 and 14 shared bands and panics at 10, 11, 15 and 16 —
/// the band count is NOT a monotone boundary — and the failing rungs are the
/// same for one, two and four loops. With four bands the cluster may hold seven
/// values but not eight.
#[test]
fn cascade_spill() {
    run_case("cascade_spill", include_str!("../cases/case_cascade_spill.rs"));
}

/// Pinned grid for [`cascade_spill`]: `input2`'s bits decide which trip of which
/// loop takes its `continue` edge, so these pairs pin all-continue, never-
/// continue, two alternating masks and two mixed ones. A reload dropped with a
/// removed column would change exactly one of these values.
#[test]
fn cascade_spill_edges() {
    run_case_with_inputs(
        "cascade_spill_edges",
        include_str!("../cases/case_cascade_spill.rs"),
        &[
            (0, 0),
            (0, u32::MAX),
            (1, 0xaaaa_aaaa),
            (u32::MAX, 0x5555_5555),
            (0x1234_5678, 0xdead_beef),
            (1, 0),
        ],
    );
}

/// Passthrough-branch collapse over reloads: `SimplifyPassthroughCondBr` and
/// `SplitCriticalEdges` alternating to a fixpoint on a two-level frame with
/// TWELVE shared bands crossing both loops.
///
/// Fired counts: ten `simplify-passthrough-cond-br` with thirteen
/// `split-critical-edges` at the default level and ten with twelve at
/// `--optimize=size-min` — the same ten collapses as the freight-free
/// `canon::passthru_frame`, so the freight changes what the collapsed paths
/// carry but not how often the pattern fires.
///
/// Spill evidence: 24 spills, 48 reloads, three split edges, 20 erased split
/// reloads and five "unused phi" warnings from the spill transform's own SSA
/// reconstruction — the F1 marker and the F6 marker both present in a program
/// that computes the right answer on every pinned path.
///
/// Boundary (campaign 20): the band axis has no boundary at the default level
/// through sixteen bands, but `--optimize=size-min` panics from fourteen up;
/// making the outer loop zero-trip-capable does not move either. The u64
/// cluster is what breaks this shape — four values at four bands already panic,
/// which makes the passthrough frame by far the least freight-tolerant of the
/// composed shapes.
#[test]
fn passthru_spill() {
    run_case("passthru_spill", include_str!("../cases/case_passthru_spill.rs"));
}

/// Pinned exit grid for [`passthru_spill`], one native-verified pair per exit:
/// tags 1-4 are the four in-loop `return`s (the sites whose passthrough blocks
/// the pattern collapses) and tag 5 is the outer loop's normal exit.
#[test]
fn passthru_spill_edges() {
    run_case_with_inputs(
        "passthru_spill_edges",
        include_str!("../cases/case_passthru_spill.rs"),
        &[(0, 25), (0, 13), (0, 1), (0, 0), (1024, 0)],
    );
}

/// Switch arm merging over a dispatch with ten bands live across it.
/// `SimplifySwitchFallbackOverlap` rebuilds the in-loop `cf.switch` without all
/// five overlapping cases at once (one rewrite at both opt levels, exactly as
/// the freight-free `canon::arms_merge`), while twenty spills and twenty
/// reloads cross the same loop and four split-edge reloads are erased.
///
/// Boundary (campaign 20): twelve bands still compile at the default level but
/// panic at `--optimize=size-min`, and sixteen bands panic at both levels — at
/// `hir/src/ir/dominance/frontier.rs:123`, the unwrap the ignored
/// `pressure::zero_trip_frontier` documents, without any zero-trip-capable loop
/// in the program. A six-u64 cluster on top of four bands panics as well.
#[test]
fn dispatch_spill() {
    run_case("dispatch_spill", include_str!("../cases/case_dispatch_spill.rs"));
}

/// Per-arm pinned grid for [`dispatch_spill`]: the selector is
/// `(input1 + i) & 15` and `input2 = 11` makes the loop run thirteen trips, so
/// each of these sixteen pairs enters the dispatch at a different arm — the
/// five merged arms, the ten kept arms and the true default alike. A case key
/// rebuilt at the wrong index changes exactly one of these values.
#[test]
fn dispatch_spill_edges() {
    run_case_with_inputs(
        "dispatch_spill_edges",
        include_str!("../cases/case_dispatch_spill.rs"),
        &[
            (0, 11),
            (1, 11),
            (2, 11),
            (3, 11),
            (4, 11),
            (5, 11),
            (6, 11),
            (7, 11),
            (8, 11),
            (9, 11),
            (10, 11),
            (11, 11),
            (12, 11),
            (13, 11),
            (14, 11),
            (15, 11),
        ],
    );
}

/// If-to-select over an asymmetric diamond whose then-arm reloads six bands and
/// whose else-arm reloads none. The pressure difference is what makes the spill
/// analysis reconcile the two edges — a value in the join's `W^entry` that one
/// predecessor does not carry gets a reload split onto that edge — so the
/// diamond `ConvertTrivialIfToSelect` collapses (one rewrite at both levels) has
/// band traffic on one side only. Ten spills, ten reloads, one split edge, two
/// erased split reloads.
///
/// Boundary (campaign 20): eight bands still compile at the default level but
/// panic at `--optimize=size-min`; making the arms SYMMETRIC (both reloading
/// every band) panics at eight bands at BOTH levels, so the asymmetry this case
/// is built around is what lets eight bands compile at the default level at
/// all. A six-u64 cluster on four bands panics at both levels.
#[test]
fn select_spill() {
    run_case("select_spill", include_str!("../cases/case_select_spill.rs"));
}

/// Pinned direction grid for [`select_spill`]: bit `i` of `input1` picks the
/// arm on trip `i`, so these pairs pin all-else, all-then and four mixed
/// sequences — a select whose operands were swapped, or an arm hoisted across a
/// reload, changes one of them.
#[test]
fn select_spill_edges() {
    run_case_with_inputs(
        "select_spill_edges",
        include_str!("../cases/case_select_spill.rs"),
        &[
            (0, 7),
            (u32::MAX, 7),
            (0xaaaa_aaaa, 7),
            (0x5555_5555, 7),
            (0x1234_5678, 9),
            (1, 0),
        ],
    );
}

/// Twelve bands defined in the entry block whose only later use is in the
/// deepest else-arm of a three-level diamond, with twenty spills, twenty
/// reloads and four "unused phi" warnings and no split edge at all.
///
/// Mechanism correction (campaign 21): this case does NOT pin reloads being
/// sunk into a region, and nothing in the pipeline can do that. `SinkOperandDefs`
/// moves the DEFINING op of an operand next to its use inside the same block;
/// the pass that moves ops into regions is `ControlFlowSink`, which is
/// registered but never scheduled (hir-transform/src/sink.rs). And spill-slot
/// reloads are ineligible either way: `hir.load_local` carries
/// `MemoryEffect::Read`, so the trace
/// (`-Z print-ir-after-pass=sink-operand-defs` +
/// `MIDENC_TRACE='sink-operand-defs=trace'`) logs `defining 'hir.load_local'
/// cannot be moved: * op has memory effects` SEVENTY-FOUR times on this case
/// and moves none of them. What the case actually pins is the band traffic
/// through a deep arm: the `IfRemoveUnusedResults` production below, and that
/// twelve bands whose only consumer is the deepest arm of a three-level nest
/// still compute the right answer on every path.
///
/// Closure correction (campaign 20): this case fires
/// `if-remove-unused-results` THREE times at both opt levels, and THE FREIGHT
/// IS WHAT PRODUCES IT. That pattern was recorded as having no producer at all,
/// on the argument that cfg-to-scf builds a payload column only for a value
/// with a use outside the region. Bisected here against a control with the same
/// three-level diamond and ZERO bands, which fires it zero times: four, eight
/// and twelve bands each fire it three times, so it is the band traffic
/// crossing the nest — not the nest — that leaves an `scf.if` result with no
/// real use. This is the only pattern in the corpus whose fire count freight
/// changes from zero to non-zero.
///
/// Boundary (campaign 20): twelve bands are fine in this arm shape, but moving
/// the consuming region from an arm to the SECOND of two sequential loops
/// panics with only eight — at the default level (`frontier.rs:123`) while
/// still compiling at `--optimize=size-min`.
#[test]
fn sink_spill() {
    run_case("sink_spill", include_str!("../cases/case_sink_spill.rs"));
}

/// Per-path pinned grid for [`sink_spill`]: bits 4-6 of `input1` select the
/// path and the taken path is echoed in the top nibble, so these pairs pin all
/// four exits — including the deep arm (tag 2) that is the only consumer of the
/// twelve sunk bands.
#[test]
fn sink_spill_edges() {
    run_case_with_inputs(
        "sink_spill_edges",
        include_str!("../cases/case_sink_spill.rs"),
        &[(0, 0), (16, 0), (48, 0), (112, 0), (112, u32::MAX), (112, 0x1234_5678)],
    );
}

/// Four sequential early-`break` scan loops — the `WhileRemoveUnusedArgs`
/// producer — with eight bands crossing all four. Fired counts:
/// `while-remove-unused-args` 4, `split-critical-edges` 8,
/// `convert-trivial-if-to-select` 4, at the default level AND at
/// `--optimize=size-min`. The freight-free chain of the same length fires this
/// pattern K times at `-Oz` and ZERO times at the default level, because LLVM
/// unrolls the scans away there; the band traffic in the body is what keeps the
/// loops rolled at O2, so the composition also widens where the pattern fires
/// at all. Eighteen spills, eighteen reloads, one split edge, four erased.
///
/// Boundary (campaign 20): twelve bands still pass. On the cluster axis, the
/// PREDICATE of the break matters — with an accumulator-derived break condition
/// the same four loops carry a ten-u64 cluster, while with this case's
/// input-bit-driven break a six-u64 cluster already panics.
#[test]
fn scan_spill() {
    run_case("scan_spill", include_str!("../cases/case_scan_spill.rs"));
}

/// Pinned trip-count grid for [`scan_spill`]: bit `k*8 + u` of `input2` breaks
/// loop `k` on trip `u`, so these pairs pin never-break (all four loops run
/// their eight trips), break-on-the-first-trip for all four, and four mixed
/// trip-count mixtures.
#[test]
fn scan_spill_edges() {
    run_case_with_inputs(
        "scan_spill_edges",
        include_str!("../cases/case_scan_spill.rs"),
        &[
            (7, 0),
            (7, u32::MAX),
            (7, 0xaaaa_aaaa),
            (7, 0x5555_5555),
            (0x1234_5678, 0x0f0f_0f0f),
            (1, 1),
        ],
    );
}
