//! Realistic programs whose ALGORITHM carries many distinct rotate/shift
//! constants, pinned to `--optimize=size-min` (campaign 26, 2026-09-10):
//! hash functions, ciphers, PRNGs and bit-manipulation kernels written the
//! way a size-conscious user writes them — helpers where the algorithm has
//! them, no `#[inline(never)]` pinning, and no artificial constant sharing
//! (the sharing comes from seed/absorb + rounds + finalize).
//!
//! Why `-Oz` here: it is the escape hatch the earlier campaigns found for the
//! programs that do not compile at the default level, but it keeps the
//! CSE-merged count bands un-hoisted, so the synthetic ladders put its
//! freight boundary around ten shared constants (`opt_levels::band_guard_oz`,
//! `spills::spill_loop_mix_oz`). These programs measure what that means for
//! real code: a Keccak-f permutation with the real twenty-four rho offsets
//! compiles at every optimization level, while a fifteen-constant
//! Threefish-256 does not compile at `-Oz` at all — so the distinct-constant
//! count is not the lever, the number of count-band values live as scheduled
//! operands across the kept loop is (see `prog_threefish_oz` below).
//!
//! Every program here was checked natively on the 1225-pair boundary grid
//! before it was kept, deep-fuzzed at `FUZZA_INPUT_PAIRS=512` under the
//! pinned `-Oz` configuration (zero divergences), and run at `-Oz` with and
//! without guest DWARF and at `--optimize=max`, the default level and
//! `--optimize=basic`; each doc comment records that table. Cases that do not
//! compile in some configuration are `#[ignore]`d twins beside the compiling
//! sibling, exactly as in [`super::programs`].

use super::super::harness::{run_case, run_case_with_flags, run_case_with_flags_and_inputs};

/// Flags shared by every passing case in this module: LLVM `-Oz` for the
/// guest.
const SIZE_MIN: &[&str] = &["--optimize=size-min"];

/// Keccak-f[1600] reduced to twelve rounds (the TurboSHAKE / KangarooTwelve
/// round reduction) over a 25-lane u64 state in a stack array: absorb loop
/// over one or two 17-lane rate blocks, padding at a runtime lane, squeeze
/// loop. The rho step is unrolled with the real 24 rotation offsets, so
/// TWENTY-FOUR distinct rotate constants cross the twelve-trip round loop and
/// the permutation is called from the absorb loop, the padding step and the
/// squeeze loop. THE COUNTER-EXAMPLE TO "ten shared constants is the -Oz
/// cliff": this program compiles and matches native at `-Oz` (with and
/// without guest DWARF), at the default level, at `--optimize=max` and at
/// `--optimize=basic`. Its state lives in memory, so the rotate operands are
/// freshly loaded lanes and none of the twenty-four bands is live as a
/// scheduled operand across the round loop; `-Oz` also keeps `keccak_f12` as
/// a real call, which keeps the constants out of the caller's loops
/// entirely.
#[test]
fn prog_keccakf_oz() {
    run_case_with_flags("prog_keccakf_oz", include_str!("../cases/case_prog_keccakf.rs"), SIZE_MIN);
}

/// Pinned grid for [`prog_keccakf_oz`]: both message-block counts
/// (`input2 % 2`), both padding-lane boundaries (`input1 % 17` at 0 and 16),
/// zero / all-ones / equal pairs.
#[test]
fn prog_keccakf_oz_edges() {
    run_case_with_flags_and_inputs(
        "prog_keccakf_oz_edges",
        include_str!("../cases/case_prog_keccakf.rs"),
        SIZE_MIN,
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (16, 0),
            (17, 1),
            (0x9e37_79b9, 0x9e37_79b9),
            (1, 1),
        ],
    );
}

/// SHA-512 compression over one or two blocks, written inline in the block
/// loop: the real 80-entry round-constant table, an 80-word message schedule
/// and eight working variables carried through the round loop. TWELVE
/// distinct rotate/shift constants — Sigma0 (28, 34, 39), Sigma1 (14, 18,
/// 41), sigma0 (1, 8, `>> 7`) and sigma1 (19, 61, `>> 6`) — cross the
/// schedule loop, the round loop, the block loop and the digest fold.
/// Until the #1420 fix `-Oz` WAS THE ONLY CONFIGURATION THAT COMPILED IT: the
/// default level, max and basic all panicked in the F6 class (see the formerly
/// ignored [`prog_sha512`] twin below). The u32 sibling `programs::prog_sha256` compiles everywhere;
/// the difference is the operand width — the same twelve-constant shape on
/// 64-bit working variables doubles the felts each band and each working
/// variable occupies.
///
/// It compiled on `next` only because the spill transform dropped its
/// split-edge reloads at two-predecessor joins (the #1420 defect), and it
/// stopped compiling once #1420 was fixed: with those spills materialized it
/// panics with `invalid operand stack index (9): requires access to more than
/// 16 elements` at codegen/masm/src/emit/mod.rs (OpEmitter::dup) after the second spills
/// run (`edges to split = 0, values spilled = 11, reloads issued = 3`). What
/// remains is the #1422 item 2 class (over-window pressure after the second
/// spills run). Compile-time — no inputs involved.
#[test]
#[ignore = "#1422: compiler panic at --optimize=size-min: 'invalid operand stack index (9): \
            requires access to more than 16 elements' at codegen/masm/src/emit/mod.rs \
            (OpEmitter::dup) — over-window pressure after the second spills run (edges to split = \
            0, values spilled = 11, reloads issued = 3); compiled before the #1420 fix only \
            because split-edge reloads were dropped; compile-time, no inputs involved"]
fn prog_sha512_oz() {
    run_case_with_flags("prog_sha512_oz", include_str!("../cases/case_prog_sha512.rs"), SIZE_MIN);
}

/// Pinned grid for [`prog_sha512_oz`]: both block counts (`input2 % 2`), the
/// message-length byte at 0 and 1023 (`input1 % 1024`), zero / all-ones /
/// equal pairs. Ignored with [`prog_sha512_oz`] (#1422 item 2, same
/// compile-time panic since the #1420 fix); the pinned inputs never run.
#[test]
#[ignore = "#1422: same compile-time panic as prog_sha512_oz ('invalid operand stack index (9)' at \
            codegen/masm/src/emit/mod.rs (OpEmitter::dup) at --optimize=size-min, over-window \
            pressure after the second spills run); the pinned inputs never run"]
fn prog_sha512_oz_edges() {
    run_case_with_flags_and_inputs(
        "prog_sha512_oz_edges",
        include_str!("../cases/case_prog_sha512.rs"),
        SIZE_MIN,
        &[
            (0, 0),
            (1023, 1),
            (1024, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x9e37_79b9, 0x9e37_79b9),
            (1, 1),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (safe Rust,
/// campaign 26): the SHA-512 compression of [`prog_sha512_oz`] built without
/// pinned flags panicked with `invalid operand stack index (9): requires access
/// to more than 16 elements` at codegen/masm/src/emit/mod.rs:623. F6 class:
/// the spills trace (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`)
/// shows 40 and 53 values spilled in the two analysis runs, FOUR "edges to
/// split" and 14 `erase unused reload` lines — the SSA reconstruction walking
/// the dominator tree cached before the transform's own splits. Per level:
/// default PANIC (index 9), `--optimize=max` PANIC (index 12),
/// `--optimize=basic` PANIC (index 10), `--optimize=size-min` PASSED until the
/// #1420 fix (the pinned [`prog_sha512_oz`] above, deep-fuzzed at 512 pairs). Guest DWARF
/// did not move it. This was the headline "-Oz is the only escape" row: no
/// source change is involved, only the guest optimization level.
/// Compile-time — no inputs involved. It was `#[ignore]`d until the #1420 fix
/// (the spill transform now recomputes dominance after splitting edges and
/// prunes its unused phis) and now compiles.
#[test]
fn prog_sha512() {
    run_case("prog_sha512", include_str!("../cases/case_prog_sha512.rs"));
}

/// xxHash64 over a 64-byte buffer with a runtime length plus the
/// MurmurHash3-x64 finalizer: ELEVEN distinct rotate/shift constants by
/// construction — the stripe round's 31, the convergence rotations 1, 7, 12
/// and 18, the 8-, 4- and 1-byte tail rotations 27, 23 and 11, and the
/// avalanche shifts 33, 29 and 32 — crossing the stripe loop, the three tail
/// loops and the finalization. Sixty-four-bit multiplies only, so the F9
/// guest-toolchain family is out of the picture. `-Oz` INLINES its `round`
/// helper (the guest wasm has a single function and no calls, with eight kept
/// loops), so all eleven bands really do share one function here — unlike
/// [`prog_keccakf_oz`], whose permutation stays a call. Compiles and matches
/// native at `-Oz` (with and without guest DWARF), at the default level and
/// at max;
/// `--optimize=basic` panicked in the F12 class until #1419 was fixed (see
/// [`prog_xxh64_o1`]).
#[test]
fn prog_xxh64_oz() {
    run_case_with_flags("prog_xxh64_oz", include_str!("../cases/case_prog_xxh64.rs"), SIZE_MIN);
}

/// Pinned grid for [`prog_xxh64_oz`]: every length path — 0, the short-input
/// path (31), exactly one stripe (32), two stripes (64), and the tails that
/// take the 8-byte loop plus the 4-byte and 1-byte branches (45, 36, 33) —
/// plus zero / all-ones / equal pairs.
#[test]
fn prog_xxh64_oz_edges() {
    run_case_with_flags_and_inputs(
        "prog_xxh64_oz_edges",
        include_str!("../cases/case_prog_xxh64.rs"),
        SIZE_MIN,
        &[
            (0, 0),
            (7, 31),
            (7, 32),
            (7, 64),
            (7, 45),
            (7, 36),
            (7, 33),
            (0xffff_ffff, 0xffff_ffff),
            (1, 1),
        ],
    );
}

/// FORMER CONFIGURATION-DEPENDENT COMPILE-TIME COMPILER PANIC (#1419, fixed):
/// the xxHash64 program above panicked at `--optimize=basic` with
/// `AliasingViolationError { kind: Mutable, location:
/// hir/src/ir/operation.rs:877 }` at
/// hir/src/patterns/rewriter.rs:335, the driver's last line under
/// `MIDENC_TRACE='pattern-rewrite-driver=trace'` being `trying to match
/// 'remove-loop-invariant-args-from-before-block' dialect=scf op=while` — the
/// F12 class. Notable for the producer rule: this program contains NO
/// `return`, `break` or `continue` at all (campaign 22 derived the rule from
/// a `return` leaving the function from a two-level nest), so plain `while`
/// nests with an `if`-guarded tail branch reach the same poison-carrying
/// payload column. Compile-time — no inputs involved. It was `#[ignore]`d
/// until the #1419 fix (see `compose::invariant_args_min`).
#[test]
fn prog_xxh64_o1() {
    run_case_with_flags(
        "prog_xxh64_o1",
        include_str!("../cases/case_prog_xxh64.rs"),
        &["--optimize=basic"],
    );
}

/// BLAKE2b compression over one or two blocks: the 16-word working vector,
/// the ten-row SIGMA schedule in `.rodata`, twelve rounds of eight G
/// functions, the counter and the last-block flag. Only FOUR distinct
/// rotation constants (32, 24, 16, 63) — the low-constant calibration point
/// of the campaign — used in eight places per round, so the merged bands
/// cross the G loop, the round loop, the block loop and the fold. Compiles
/// and matches native at `-Oz` with and without guest DWARF; the default
/// level, max and basic all panicked in the F12 class until #1419 was fixed
/// and pass with guest DWARF since (see [`prog_blake2b`]); without guest DWARF
/// the default level hits the arity-2 `NoSolution` gap (#1422, see
/// [`prog_blake2b_nodwarf`]).
#[test]
fn prog_blake2b_oz() {
    run_case_with_flags("prog_blake2b_oz", include_str!("../cases/case_prog_blake2b.rs"), SIZE_MIN);
}

/// FORMER COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (#1419,
/// fixed; safe Rust, campaign 26): the BLAKE2b compression of
/// [`prog_blake2b_oz`] built without pinned flags panicked with `AliasingViolationError { kind: Mutable,
/// location: hir/src/ir/operation.rs:877 }` at
/// hir/src/patterns/rewriter.rs:335 — F12, confirmed by the driver's last
/// line `trying to match 'remove-loop-invariant-args-from-before-block'
/// dialect=scf op=while`. It also panicked at `--optimize=max` and
/// `--optimize=basic`, and compiled only at `--optimize=size-min`.
/// Two things that panic showed. First, F12 is not a count-band phenomenon: this
/// program has the FEWEST distinct rotation constants in the module (four).
/// Second, its control flow contains no `return`, `break` or `continue`
/// whatsoever — a three-level `while` nest over array-indexed state is
/// enough — so campaign 22's "a `return` that leaves the function from a
/// two-level nest" is one producer, not the producer.
/// Since the #1419 fix it compiles and matches native at the default level,
/// max and basic, and with guest `debug = 1` as well as full DWARF. Without
/// guest DWARF it hits the arity-2 gap instead (see [`prog_blake2b_nodwarf`]).
/// The former panic was compile-time — no inputs involved.
#[test]
fn prog_blake2b() {
    run_case("prog_blake2b", include_str!("../cases/case_prog_blake2b.rs"));
}

/// COMPILE-TIME COMPILER PANIC WITHOUT GUEST DWARF: [`prog_blake2b`] at the
/// default level with guest `debug = 0` panics with `failed to schedule
/// operands: [%1926, %1362] for inst 'arith.rotl' with error: NoSolution,
/// constraints: [Move, Copy]` at codegen/masm/src/lower/lowering.rs:113 over a
/// 9-operand / 15-felt stack — the arity-2 gap. Compile-time — no inputs
/// involved.
#[test]
#[ignore = "#1422: compiler panic without guest DWARF: 'with error: NoSolution' on 'arith.rotl' \
            [Move, Copy] at codegen/masm/src/lower/lowering.rs:113 over a 15-felt operand stack; \
            compile-time, no inputs involved"]
fn prog_blake2b_nodwarf() {
    run_case_with_flags(
        "prog_blake2b_nodwarf",
        include_str!("../cases/case_prog_blake2b.rs"),
        &["--guest-debug=0"],
    );
}

/// THE -Oz ESCAPE HATCH CLOSING: Threefish-256 (the Skein block cipher)
/// reduced to sixteen of its seventy-two rounds — four u64 words carried in
/// scalars, a key injection every four rounds with the tweak schedule, the
/// eight rotation rows unrolled inside a two-trip round-group loop the way
/// reference implementations write them. Threefish-256's rotation table has
/// FIFTEEN distinct constants (14, 16, 52, 57, 23, 40, 5, 37, 25, 33, 46, 12,
/// 58, 22, 32), reused by the output whitening and the final fold.
/// At `--optimize=size-min` it panics with `invalid operand stack index (11):
/// requires access to more than 16 elements` at
/// codegen/masm/src/emit/mod.rs:623, identically with and WITHOUT guest DWARF
/// — while the default level and `--optimize=basic` compile it and match
/// native. At `--optimize=max` it panicked differently until the #1420 fix: `failed to schedule
/// operands: [%739, %488] for inst 'arith.rotl' with error: NoSolution,
/// constraints: [Move, Copy]` at codegen/masm/src/lower/lowering.rs:109 over
/// an 8-operand / 15-felt stack.
/// CLASSIFICATION — a THIRD window-overflow mechanism (ledger F17), neither
/// the stale-dominator-tree defect (F6) nor the arity-2 solver gap (F2):
/// under `MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'` the `-Oz`
/// build spills (10 and 8 values in the two analysis runs, twelve reloads
/// converted, fourteen `max usage on exit (17)/(18) exceeds K (16),
/// additional spills required` lines) yet reports `edges to split = 0` in
/// both runs and erases NO reload — so the analysis ran and still let the
/// emitter's stack exceed sixteen felts. The emitter drop trace
/// (`MIDENC_TRACE='codegen:operand-scheduling=trace'`) shows the failing op
/// is the SPILL STORE itself, `hir.store_local %15` into spill slot 27: the
/// value chosen for spilling already sits at operand index 11, past the
/// window, when the store is emitted. The former `--optimize=max` panic of the
/// same source was a different class again; it compiles now, see
/// [`prog_threefish_o3`].
/// LADDER (all rungs value-checked natively, `scratch/c26gen_tf.py`): merging
/// rotation rows to reduce the distinct-constant count does NOT give a
/// boundary — 13, 12, 11, 9 and 6 constants all panic, 8 and 4 compiled, so
/// the ladder is non-monotone and the largest compiling variant was the
/// eight-constant [`prog_threefish_oz_guard`] (until the #1420 fix). Removing every post-loop use
/// of the constants (the `M = 0` rescue of campaign 18's synthetic ladder)
/// does not rescue this program either. What does: `black_box` on the counts
/// ([`prog_threefish_oz_wa`]) and moving the round group into an
/// `#[inline(never)]` helper taking `&mut [u64; 4]`.
/// Compile-time — no inputs involved. Un-ignore when the spill analysis
/// places its spills where the spilled value is still inside the window (or
/// the emitter spills from below it).
#[test]
#[ignore = "#1422: compiler panic at --optimize=size-min (with AND without guest DWARF): 'invalid \
            operand stack index (11): requires access to more than 16 elements' at \
            codegen/masm/src/emit/mod.rs:623 while emitting a spill store — over-window pressure \
            with the spill analysis run, no edge splits, no erased reloads (ledger F17; not F6, \
            not F2); the default level and --optimize=basic compile the same source; compile-time, \
            no inputs involved"]
fn prog_threefish_oz() {
    run_case_with_flags(
        "prog_threefish_oz",
        include_str!("../cases/case_prog_threefish.rs"),
        SIZE_MIN,
    );
}

/// The `--optimize=max` face of [`prog_threefish_oz`]: the same source panicked
/// with `failed to schedule operands: [%739, %488] for inst 'arith.rotl' with
/// error: NoSolution, constraints: [Move, Copy]` at
/// codegen/masm/src/lower/lowering.rs:109. The dumped operand stack holds
/// eight operands — seven u64 and one u32, fifteen felts — i.e. it is IN the
/// window, with a Copy-constrained operand under seven u64 words: the
/// arity-2 solver gap (F2, `spills::rotl_window` class) reached by a
/// REALISTIC program for the first time. The spills trace does not change
/// that — the function spills eighteen values elsewhere and erases one
/// reload, but `edges to split = 0` in both analysis runs, so the erased
/// reload is not a split-edge reload and the stale-dominator-tree defect
/// (F6) is not involved. Kept so a corpus-wide `--optimize=max` sweep does
/// not rediscover it as a new finding. Compile-time — no inputs involved.
/// Filed under #1422; it stopped panicking with the #1420 fix (fresh
/// dominator tree after edge splits, pruning of the transform's unused phis),
/// was `#[ignore]`d until then, and no longer reproduces the panic it was
/// filed for.
#[test]
fn prog_threefish_o3() {
    run_case_with_flags(
        "prog_threefish_o3",
        include_str!("../cases/case_prog_threefish.rs"),
        &["--optimize=max"],
    );
}

/// The largest variant of [`prog_threefish_oz`] that compiled at `-Oz` until
/// the #1420 fix: the eight-row rotation table halved, so four rows are used
/// twice each and EIGHT distinct rotation constants cross the round-group
/// loop, the block loop, the whitening and the fold. Everything else is
/// unchanged. Passed at `-Oz` with and without guest DWARF, at the default
/// level, at max and at basic. Note that the ladder between it and the full cipher is
/// non-monotone: nine, eleven, twelve and thirteen constants all panic, but
/// so does the SIX-constant rung — "use fewer distinct rotation constants" is
/// therefore not a reliable user-level fix, which is why
/// [`prog_threefish_oz_wa`] is the recommended one.
///
/// It compiled on `next` only because the spill transform dropped its
/// split-edge reloads at two-predecessor joins (the #1420 defect), and it
/// stopped compiling at `-Oz` once #1420 was fixed: with those spills
/// materialized it panics with `invalid operand stack index (10): requires
/// access to more than 16 elements` at codegen/masm/src/emit/mod.rs (OpEmitter::dup) after
/// the second spills run (`edges to split = 0, values spilled = 8, reloads
/// issued = 2`). What remains is the #1422 item 2 class (over-window pressure
/// after the second spills run). Compile-time — no inputs involved.
#[test]
#[ignore = "#1422: compiler panic at --optimize=size-min: 'invalid operand stack index (10): \
            requires access to more than 16 elements' at codegen/masm/src/emit/mod.rs \
            (OpEmitter::dup) — over-window pressure after the second spills run (edges to split = \
            0, values spilled = 8, reloads issued = 2); compiled before the #1420 fix only because \
            split-edge reloads were dropped; compile-time, no inputs involved"]
fn prog_threefish_oz_guard() {
    run_case_with_flags(
        "prog_threefish_oz_guard",
        include_str!("../cases/case_prog_threefish_oz_guard.rs"),
        SIZE_MIN,
    );
}

/// Pinned grid for [`prog_threefish_oz_guard`]: all three block counts
/// (`input2 % 3`), zero / all-ones / equal pairs and the sign boundary.
/// Ignored with [`prog_threefish_oz_guard`] (#1422 item 2, same compile-time
/// panic since the #1420 fix); the pinned inputs never run.
#[test]
#[ignore = "#1422: same compile-time panic as prog_threefish_oz_guard ('invalid operand stack \
            index (10)' at codegen/masm/src/emit/mod.rs (OpEmitter::dup) at --optimize=size-min, \
            over-window pressure after the second spills run); the pinned inputs never run"]
fn prog_threefish_oz_guard_edges() {
    run_case_with_flags_and_inputs(
        "prog_threefish_oz_guard_edges",
        include_str!("../cases/case_prog_threefish_oz_guard.rs"),
        SIZE_MIN,
        &[
            (0, 0),
            (1, 1),
            (2, 2),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x9e37_79b9, 0x9e37_79b9),
            (7, 0x7fff_ffff),
        ],
    );
}

/// THE RESCUE THAT WORKS for [`prog_threefish_oz`]: the full fifteen-constant
/// cipher with every rotation count wrapped in `core::hint::black_box`
/// (twenty-five one-token edits, no restructuring), so the counts arrive as
/// runtime values and no CSE-merged band is live across the round-group loop.
/// Computes the same answer as the unmodified program on the 1225-pair native
/// boundary grid (checksum 0x01a9324cf6210b97), and — unlike the
/// release-configuration result for `programs::prog_rkscan_wa`, where the
/// same rescue fell into F12 (#1419, fixed) — it holds in every configuration measured
/// here: `-Oz` with DWARF, `-Oz` without DWARF, the default level, max and
/// basic. Moving the round group into an `#[inline(never)]` helper taking
/// `&mut [u64; 4]` (the campaign-22 by-reference rescue) also works in all
/// five, at the cost of a real restructure; keeping fewer distinct rotation
/// constants does not (see the non-monotone ladder at
/// [`prog_threefish_oz_guard`]).
#[test]
fn prog_threefish_oz_wa() {
    run_case_with_flags(
        "prog_threefish_oz_wa",
        include_str!("../cases/case_prog_threefish_oz_wa.rs"),
        SIZE_MIN,
    );
}

/// A simulation driven by three real PRNGs: SplitMix64 seeding (shifts 30,
/// 27, 31), xoshiro256** as the stream (rotations 7 and 45, shift 17) and a
/// PCG-style output permutation (shifts 18, 27, 59 plus a RUNTIME rotate by
/// the top five bits). EIGHT distinct constants — one rung below the
/// synthetic `-Oz` band cap — cross the seeding loop, the simulation loop and
/// the statistics fold, with five u64 statistics live across the loop.
/// `-Oz` inlines the `splitmix` helper (one guest function, no calls).
/// Compiles and matches native in all five configurations; kept as the PRNG
/// family's representative and as the in-corpus example of a real program
/// mixing constant and runtime rotate counts.
#[test]
fn prog_prngsim_oz() {
    run_case_with_flags("prog_prngsim_oz", include_str!("../cases/case_prog_prngsim.rs"), SIZE_MIN);
}

/// ChaCha20 keystream encryption followed by a Salsa20-core checksum over the
/// ciphertext: SEVEN distinct rotation constants on a 32-BIT state (the
/// ChaCha quarter-round's 16, 12, 8, 7 and the Salsa round's 7, 9, 13, 18)
/// crossing the keystream-block loop, both twenty-round cores and the fold.
/// The width control for this module: the same constant-count range that
/// breaks the u64 programs is harmless on u32 state, where every band and
/// every state word occupies one felt instead of two. Compiles and matches
/// native in all five configurations.
#[test]
fn prog_chacha_salsa_oz() {
    run_case_with_flags(
        "prog_chacha_salsa_oz",
        include_str!("../cases/case_prog_chacha_salsa.rs"),
        SIZE_MIN,
    );
}

/// Bit-permutation kernel: the mask/shift u64 bit reversal, Morton (Z-order)
/// encode and decode, and a byte swap over a table of points. SIX distinct
/// shift constants (1, 2, 4, 8, 16, 32 — the halving ladder every one of
/// these algorithms uses) shared between three helpers, the point loop and
/// the fold. `-Oz` keeps `morton_spread` and `morton_compact` as real calls
/// (four call sites) and inlines the single-use `reverse_bits64`. Compiles
/// and matches native in all five configurations: shift
/// constants applied to freshly computed values, rather than to state carried
/// across the loop, do not accumulate freight.
#[test]
fn prog_bitperm_oz() {
    run_case_with_flags("prog_bitperm_oz", include_str!("../cases/case_prog_bitperm.rs"), SIZE_MIN);
}

/// SipHash-2-4 over a message of runtime length with the length byte: FIVE
/// distinct rotation constants (13, 16, 17, 21, 32) living in the four u64
/// state words across the absorb loop, the two compression rounds per word,
/// the tail word and the four finalization rounds. Compiles and matches
/// native in all five configurations; the low-constant, high-reuse control
/// for the u64 side of the module.
#[test]
fn prog_siphash_oz() {
    run_case_with_flags("prog_siphash_oz", include_str!("../cases/case_prog_siphash.rs"), SIZE_MIN);
}
