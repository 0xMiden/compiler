//! Differential cases. One `#[test]` per file under `cases/`, driven by `run_case`.

use super::harness::{run_case, run_case_with_inputs};

#[test]
fn add() {
    run_case("add", include_str!("cases/case_add.rs"));
}

#[test]
fn xor() {
    run_case("xor", include_str!("cases/case_xor.rs"));
}

/// Non-commutative — exercises argument ordering (`input1 - input2`).
#[test]
fn sub() {
    run_case("sub", include_str!("cases/case_sub.rs"));
}

#[test]
fn branchy() {
    run_case("branchy", include_str!("cases/case_branchy.rs"));
}

/// Exercises bounded loops with carried values and nested conditional control flow.
#[test]
fn while_carried() {
    run_case("while_carried", include_str!("cases/case_while_carried.rs"));
}

/// Exercises dense match/switch control flow, including wasm `br_table` translation.
#[test]
fn dense_match() {
    run_case("dense_match", include_str!("cases/case_dense_match.rs"));
}

/// Exercises nested loops, local breaks, and labelled non-local loop exits.
#[test]
fn nested_breaks() {
    run_case("nested_breaks", include_str!("cases/case_nested_breaks.rs"));
}

/// Exercises sparse/default-heavy switch control flow.
#[test]
fn sparse_match() {
    run_case("sparse_match", include_str!("cases/case_sparse_match.rs"));
}

/// Exercises compile-time translation of an unreachable panic edge.
#[test]
fn unreachable_guard() {
    run_case("unreachable_guard", include_str!("cases/case_unreachable_guard.rs"));
}

#[test]
fn muladd() {
    run_case("muladd", include_str!("cases/case_muladd.rs"));
}

/// Exercises integer width conversions and per-width bit-counting arms in
/// `codegen/masm/src/emit/unary.rs` (`!x` lowers to xor, never `bnot`).
#[test]
fn widening() {
    run_case("widening", include_str!("cases/case_widening.rs"));
}

/// Exercises u32 bitwise / shift / rotate / comparison emitter arms in
/// `codegen/masm/src/emit/binary.rs`.
#[test]
fn bitops() {
    run_case("bitops", include_str!("cases/case_bitops.rs"));
}

/// Bounded loop whose Rust-level duplicated/dead/loop-invariant carried values
/// all travel through wasm locals, so the lifted scf.while forwards no values
/// and the while arg/result canonicalization patterns are invoked but bail
/// early (the locals argument, see KNOWLEDGE.md) — covers those bail paths.
#[test]
fn loop_results() {
    run_case("loop_results", include_str!("cases/case_loop_results.rs"));
}

/// Loop with three distinct exit edges — exercises cfg-to-scf exit
/// multiplexing (`transform_to_reduce_loop`) and scf.while arg/result
/// canonicalization.
#[test]
fn multi_exit_loop() {
    run_case("multi_exit_loop", include_str!("cases/case_multi_exit_loop.rs"));
}

/// Dynamically-impossible panic path (cross-modulus contradiction) — the
/// surviving trap exercises `ub::Unreachable` translation and lowering.
#[test]
fn trap_branch() {
    run_case("trap_branch", include_str!("cases/case_trap_branch.rs"));
}

/// Non-inlined helper calls (multi-arg, u64, bool) plus reused selects —
/// exercises call translation/lowering and select emitter variants.
#[test]
fn calls_selects() {
    run_case("calls_selects", include_str!("cases/case_calls_selects.rs"));
}

/// Four-exit loop plus eq-chains that canonicalize into contiguous-at-7 and
/// sparse cf.switch ops — exercises binary-search (interval guard) and
/// linear-search switch lowering.
#[test]
#[ignore = "flaky native/MASM divergence: mismatch on inputs (1669775643, 1062584501); separate \
            run hit VM assert 'value does not fit in i32' at cycle 2474"]
fn switch_shapes() {
    run_case("switch_shapes", include_str!("cases/case_switch_shapes.rs"));
}

/// Deterministic reproducer for the `switch_shapes` divergence: pins the
/// exact `(input1, input2)` pair the fuzzer flagged, so the bug fails
/// reliably on that input rather than only when proptest happens to draw it.
#[test]
#[ignore = "MASM VM aborts on pinned input (1669775643, 1062584501): 'value does not fit in i32'; \
            deterministic reproducer for the switch_shapes divergence"]
fn switch_shapes_repro() {
    run_case_with_inputs(
        "switch_shapes_repro",
        include_str!("cases/case_switch_shapes.rs"),
        &[(1669775643, 1062584501)],
    );
}

/// Loop with multiple `continue` backedges and a mid-body break — exercises
/// cfg-to-scf latch multiplexing and undef discriminator threading.
#[test]
fn continue_paths() {
    run_case("continue_paths", include_str!("cases/case_continue_paths.rs"));
}

/// br_table dispatch with one impossible-panic arm — switch successor
/// regions with mixed return-like terminators (ret vs unreachable).
#[test]
fn switch_trap_arm() {
    run_case("switch_trap_arm", include_str!("cases/case_switch_trap_arm.rs"));
}

/// Reused-condition selects with operands live past them plus a u64 select —
/// exercises dup/mov select emitter scheduling variants.
#[test]
fn select_sched() {
    run_case("select_sched", include_str!("cases/case_select_sched.rs"));
}

/// Mid-loop exit with a rotation-resistant body — produces an scf.while
/// with a non-empty `after` region.
#[test]
fn midloop_exit() {
    run_case("midloop_exit", include_str!("cases/case_midloop_exit.rs"));
}

/// Right-leaning single-use expression tree — ~20 simultaneously-live
/// operand-stack values, exercising spill analysis/transform.
#[test]
fn stack_pressure() {
    run_case("stack_pressure", include_str!("cases/case_stack_pressure.rs"));
}

/// Tail-merged return paths (exit block with args) plus an impossible trap
/// exit — cf.cond_br lowering with successor block arguments.
#[test]
fn ret_args() {
    run_case("ret_args", include_str!("cases/case_ret_args.rs"));
}

/// u64-returning helper with early returns, trap exit, and loop exit —
/// multi-word successor operands through branch lowering.
#[test]
fn u64_exits() {
    run_case("u64_exits", include_str!("cases/case_u64_exits.rs"));
}

/// u128 arithmetic feeding branch conditions — wide-arithmetic wasm ops
/// (add128/sub128/mul_wide) and their lowering.
#[test]
fn u128_mix() {
    run_case("u128_mix", include_str!("cases/case_u128_mix.rs"));
}

/// Runtime-indexed u32 array — dynamic i32.load/i32.store addressing
/// (`prepare_addr`, word load/store emitter paths).
#[test]
fn mem_indexed() {
    run_case("mem_indexed", include_str!("cases/case_mem_indexed.rs"));
}

/// Runtime-length `copy_from_slice`/`copy_within` — wasm `memory.copy` /
/// HIR MemCpy lowering (element fast path + byte fallback loop).
#[test]
fn mem_copy() {
    run_case("mem_copy", include_str!("cases/case_mem_copy.rs"));
}

/// Overlapping `copy_within` (dst > src) — wasm `memory.copy` memmove
/// semantics vs forward-copying MASM lowering.
#[test]
#[ignore = "native/MASM divergence: memory.copy with overlapping dst > src ranges (original repro: \
            inputs (91264998, 3811523388) in pre-split mem_copy)"]
fn mem_overlap() {
    run_case("mem_overlap", include_str!("cases/case_mem_overlap.rs"));
}

/// `static` lookup tables — wasm data segments through rodata layout,
/// merging, padding, and init-code emission.
#[test]
fn mem_static() {
    run_case("mem_static", include_str!("cases/case_mem_static.rs"));
}

/// Signed sub-word loads (i32/i64.load8_s/16_s) and unaligned u16/u32/u64
/// loads/stores via `from_le_bytes`/`to_le_bytes` at odd offsets.
#[test]
fn mem_bytes() {
    run_case("mem_bytes", include_str!("cases/case_mem_bytes.rs"));
}

/// Atomic statics (`.data` segment) plus a `.rodata` table — multi-segment
/// data layout, merging, and overlap validation; constant-address stores.
#[test]
fn mem_globals() {
    run_case("mem_globals", include_str!("cases/case_mem_globals.rs"));
}

/// `memory_grow(0, 0)` twice — MemoryGrow translation and `OpEmitter::mem_grow`.
///
/// Permanently ignored as out of scope rather than filed as a bug to fix:
/// `memory.grow` is unreachable from real Miden programs. It is only emitted by a
/// heap allocator growing linear memory, but the SDK's `BumpAlloc` (the global
/// allocator every program links, see `sdk/alloc`) bump-allocates within a fixed
/// region and aborts on exhaustion — it never grows. So the only way to reach the
/// (genuinely buggy) intrinsic is a direct `core::arch::wasm32::memory_grow` call,
/// which this case makes but no real program does. Kept as a coverage/repro
/// artifact for the MemoryGrow translation arm.
#[test]
#[ignore = "out of scope: memory.grow is unreachable from real Miden code (the SDK BumpAlloc never \
            grows linear memory); only a direct core::arch::wasm32::memory_grow call reaches the \
            intrinsic, which aborts 'if statement expected a binary value ... but got 1179648'"]
fn mem_grow() {
    run_case("mem_grow", include_str!("cases/case_mem_grow.rs"));
}

/// `memory_size(0)` twice around an impossible `memory_grow` — MemorySize
/// translation and `OpEmitter::mem_size`, deterministic zero difference.
#[test]
fn mem_size() {
    run_case("mem_size", include_str!("cases/case_mem_size.rs"));
}

/// Labeled break/continue through two loop levels, all-state-in-locals exits
/// (zero-result index_switch), loop-produced bool, and distinct-constant
/// match returns — nested scf.while + chained discriminator index_switches.
#[test]
fn cf_shapes() {
    run_case("cf_shapes", include_str!("cases/case_cf_shapes.rs"));
}

/// Statically-infinite loop behind an impossible guard plus two planted wasm
/// `unreachable` sites — cfg-to-scf `create_unreachable_terminator`, mixed
/// return-like exit kinds, and `ub.unreachable`-terminated region lowering.
#[test]
fn unreachable_exits() {
    run_case("unreachable_exits", include_str!("cases/case_unreachable_exits.rs"));
}

/// br_table in a loop with break/continue/return/trap arms — nested user +
/// discriminator index_switches and mixed in-/out-of-loop switch successors.
#[test]
fn switch_loop_mix() {
    run_case("switch_loop_mix", include_str!("cases/case_switch_loop_mix.rs"));
}

/// Signed widening shapes (the corpus otherwise never creates `arith.sext`):
/// extend_i32_s, extend8/16/32_s, and `i64.mul_wide_s` whose constant
/// multiplicand folds via `Sext::fold`'s I128 arm.
///
/// Passing siblings bound the divergence: `sext_widths` (pure extend chains),
/// `mulwide_dyn` (dynamic-by-dynamic `mul_wide_s`), and `mulwide_fold`
/// (positive-constant fold) all pass — suspicion falls on the
/// negative-constant multiplicand path or a shape interaction.
#[test]
#[ignore = "native/masm divergence: inputs (3022925119, 3340151117) -> native 3550407903, masm \
            3550391763; signed i128 widening-multiply/sign-extension shapes"]
fn sext_shapes() {
    run_case("sext_shapes", include_str!("cases/case_sext_shapes.rs"));
}

/// Deterministic reproducer for the `sext_shapes` divergence: pins the exact
/// `(input1, input2)` pair the fuzzer flagged, so the mismatch fails reliably
/// on that input rather than only when proptest happens to draw it.
#[test]
#[ignore = "native/masm divergence on pinned input (3022925119, 3340151117): native 3550407903 vs \
            masm 3550391763; deterministic reproducer for the sext_shapes divergence"]
fn sext_shapes_repro() {
    run_case_with_inputs(
        "sext_shapes_repro",
        include_str!("cases/case_sext_shapes.rs"),
        &[(3022925119, 3340151117)],
    );
}

/// `i64.mul_wide_u` with a constant multiplicand (reaches `Zext::fold`'s
/// U128 success arm) plus first genuine `i32.ctz`/`i64.ctz` uses.
#[test]
fn zext_wide_ctz() {
    run_case("zext_wide_ctz", include_str!("cases/case_zext_wide_ctz.rs"));
}

/// Tuple/struct/array returns and big by-value params — the aggregate (sret)
/// call path: zero-result `hir.exec` with sret pointers into the caller's
/// frame (multi-value returns are impossible: no `+multivalue` in
/// cargo-miden's target features).
#[test]
fn sret_shapes() {
    run_case("sret_shapes", include_str!("cases/case_sret_shapes.rs"));
}

/// 16-u32 and 8-u64 helper signatures — exactly 16 stack felts each, the
/// call-site scheduling limit (20 felts is a verified compile-time spills
/// panic) — with u64 values live across both call sites.
#[test]
fn wide_calls() {
    run_case("wide_calls", include_str!("cases/case_wide_calls.rs"));
}

/// Zero-arg zero-result / zero-arg-with-result helpers plus calls inside a
/// loop body and both branches of a conditional — call ops with empty operand
/// lists (scheduling early return) and in non-entry regions.
#[test]
fn call_mix() {
    run_case("call_mix", include_str!("cases/case_call_mix.rs"));
}

/// Ten u64s (20 felts) live across a branch and partially past its join —
/// CFG-form spills/reloads across control-flow edges and phi insertion
/// (`rewrite_cfg_spills`/`insert_required_phis`), beyond the single-block
/// spill path stack_pressure covers.
#[test]
fn spill_branch() {
    run_case("spill_branch", include_str!("cases/case_spill_branch.rs"));
}

/// Ten u64s (20 felts) live across every iteration of a loop (loop-variant
/// rotates defeat LICM) and past its exit — loop-header spill placement
/// (`compute_w_entry_loop`), backedge/exit-edge reload reconciliation, and
/// loop-pressure heuristics.
#[test]
fn spill_loop() {
    run_case("spill_loop", include_str!("cases/case_spill_loop.rs"));
}

/// Two sequential diamonds with wide mixed-width (u64/u32) arm trees over the
/// same locals — spill uses inside two scf regions, sibling-arm reloads
/// joined by phis at two joins, and size tie-breaking among spill candidates.
#[test]
fn spill_twin() {
    run_case("spill_twin", include_str!("cases/case_spill_twin.rs"));
}

/// Unsigned u64 comparisons (branches + select), dynamic-count rotates, and
/// u64 leading_zeros — exercises the `lt/lte/gt/gte_u64`, `rotr_u64`, and u64
/// `clz` emitter arms.
#[test]
fn u64_ucmp() {
    run_case("u64_ucmp", include_str!("cases/case_u64_ucmp.rs"));
}

/// Sign-extension width conversions (extend8/16/32_s, extend_i32_s) —
/// `wasm.SignExtend` lowers to `trunc(src)` + `sext(dst)`, covering
/// `trunc_int32`/`trunc_int64` small-width arms, `sext_smallint`
/// (8/16 -> 32/64), and `sext_int32(64)`; no i128 shapes.
#[test]
fn sext_widths() {
    run_case("sext_widths", include_str!("cases/case_sext_widths.rs"));
}

/// Sub-word loads widened straight to 64 bits (i64.load8/16/32_u and _s) at
/// runtime indexes — U8/U16/U32-typed loads + `arith.zext`/`sext` to 64-bit,
/// covering the 64-bit arms of `zext_smallint`/`zext_int32` and the
/// memory-flavored sign-extension entries.
#[test]
fn loadwiden() {
    run_case("loadwiden", include_str!("cases/case_loadwiden.rs"));
}

/// Dynamic-by-dynamic `i64.mul_wide_s` — both operands sign-extended to i128
/// (`sext_int64(128)`, its only Rust-reachable producer) plus the signed
/// wide-multiply hi/lo recombination, without the constant-fold shape of the
/// ignored sext_shapes case.
#[test]
fn mulwide_dyn() {
    run_case("mulwide_dyn", include_str!("cases/case_mulwide_dyn.rs"));
}

/// `i64.mul_wide_s` with a positive constant multiplicand — `Sext::fold`
/// materializes an I128 immediate that the scheduler pushes via `push_i128`,
/// its only Rust-reachable producer.
#[test]
fn mulwide_fold() {
    run_case("mulwide_fold", include_str!("cases/case_mulwide_fold.rs"));
}

/// Unsigned u64 division/remainder with dynamic non-zero divisors —
/// `checked_div_u64`/`checked_mod_u64` emitter arms (miden-core-lib
/// `u64::div`/`u64::mod`).
#[test]
fn u64_udiv() {
    run_case("u64_udiv", include_str!("cases/case_u64_udiv.rs"));
}

/// Signed i32 comparisons (`< <= > >=`) over both-sign operands feeding
/// branches and selects — the `Type::I32` arms of the `binary.rs` compare
/// dispatchers (`::intrinsics::i32::is_lt/is_lte/is_gt/is_gte`).
#[test]
fn i32_scmp() {
    run_case("i32_scmp", include_str!("cases/case_i32_scmp.rs"));
}

/// Signed i64 comparisons (`< <= > >=`) over both-sign operands feeding
/// branches and selects — the `Type::I64` arms of the `binary.rs` compare
/// dispatchers and the `lt_i64`/`lte_i64`/`gt_i64`/`gte_i64` emitters
/// (`::intrinsics::i64::{lt,lte,gt,gte}`).
#[test]
fn i64_scmp() {
    run_case("i64_scmp", include_str!("cases/case_i64_scmp.rs"));
}

/// Signed i32 division/remainder in all four sign combinations with
/// by-construction-safe dynamic divisors — `checked_div`'s I32 arm ->
/// `checked_div_i32` and `wasm.I32RemS` -> `wrapping_mod` ->
/// `wrapping_mod_i32` (truncate-toward-zero remainder signs).
#[test]
fn i32_sdiv() {
    run_case("i32_sdiv", include_str!("cases/case_i32_sdiv.rs"));
}

/// Non-strict signed compares (`<=`/`>=`, both widths) materialized as
/// boolean VALUES — branches/selects always canonicalize to strict compares,
/// so this value form is the only producer of `i32.le_s/ge_s`/`i64.le_s/ge_s`
/// and the `lte`/`gte` I32 arms + `lte_i64`/`gte_i64` emitters.
#[test]
fn scmp_bool() {
    run_case("scmp_bool", include_str!("cases/case_scmp_bool.rs"));
}

/// Arithmetic shift right (i32/i64) with dynamic masked counts and constant
/// counts — the `Type::I32`/`Type::I64` arms of the `shr` dispatcher ->
/// `shr_i32`/`shr_i64` (`::intrinsics::{i32,i64}::checked_shr`); the
/// `shr_imm_*` variants have no non-test callers.
#[test]
fn i_ashr() {
    run_case("i_ashr", include_str!("cases/case_i_ashr.rs"));
}

/// Signed i64 division with by-construction-safe dynamic divisors (positive
/// and negative) — `checked_div`'s I64 arm -> `checked_div_i64`
/// (`::intrinsics::i64::checked_div`, which execs miden-core-lib `u64::div`).
#[test]
fn i64_sdiv() {
    run_case("i64_sdiv", include_str!("cases/case_i64_sdiv.rs"));
}

/// Reproducer for a compile-time spill-transform panic: each arm calls a
/// non-inlinable helper, spills the call result under wide-tree pressure,
/// then yields it, so the spilled value crosses the control-flow edge as the
/// arm's result. A second shape (nested wide diamonds) hits the same panic.
#[test]
#[ignore = "compile-time compiler panic: TransformSpills convert_reload_to_load unwraps None \
            (dialects/hir/src/transforms/spill.rs:157); gates the edge-split spill cluster"]
fn spill_edge() {
    run_case("spill_edge", include_str!("cases/case_spill_edge.rs"));
}

/// Reproducer for a compile-time gap: signed 64-bit `%` with a dynamic
/// divisor — `arith.Mod` on I64 reaches `checked_mod`, whose dispatch has no
/// I64 arm (and no wasm.I64RemS op or i64 mod intrinsic exists to back one).
#[test]
#[ignore = "compile-time compiler panic: 'not implemented: checked_mod for i64 is not supported' \
            (codegen/masm/src/emit/binary.rs:665); i64 % with a dynamic divisor cannot compile"]
fn i64_srem() {
    run_case("i64_srem", include_str!("cases/case_i64_srem.rs"));
}

/// u128 `/` with dynamic small (u64-range) and full-width non-zero divisors —
/// executes compiler-builtins `__udivti3`/`u128_div_rem` (u64 clz/shift/
/// subtract long-division loops compiled into the guest) on the VM.
#[test]
fn u128_udiv() {
    run_case("u128_udiv", include_str!("cases/case_u128_udiv.rs"));
}

/// Pinned edge grid for `u128_udiv`: divisor exactly 1 with a huge dividend
/// ((1, 0) makes b == 1), dividend 0 ((0, 0)), smallest divisor > dividend
/// ((0, x) makes a == 0 so q1 divides n by n+1), u64::MAX and high-bit-set
/// small divisors. Divisor == dividend and both-limbs-max are outside this
/// derivation's range — pinned by `u128_bounds_edges` instead.
#[test]
fn u128_udiv_edges() {
    run_case_with_inputs(
        "u128_udiv_edges",
        include_str!("cases/case_u128_udiv.rs"),
        &[
            (0, 0),
            (1, 0),
            (0, 1),
            (0, 0xffffffff),
            (0xffffffff, 0xffffffff),
            (0xffffffff, 0),
            (1, 0xffffffff),
            (2, 0),
            (0x80000000, 0),
            (3, 5),
        ],
    );
}

/// u128 `%` with dynamic small and full-width non-zero divisors — executes
/// compiler-builtins `__umodti3` remainder paths on the VM.
#[test]
fn u128_umod() {
    run_case("u128_umod", include_str!("cases/case_u128_umod.rs"));
}

/// Pinned edge grid for `u128_umod`: dividend 0 ((0, 0)), a full-width
/// divisor greater than the dividend ((0, 1): swapped-limb d2 has high limb
/// K > a), and high-bit-set small divisors ((0xFFFFFFFF, 0): a|1 ==
/// 0xFFFFFFFF00000001). Divisor 1 with a nonzero dividend and divisor ==
/// dividend are outside this derivation's range — pinned by
/// `u128_bounds_edges` instead.
#[test]
fn u128_umod_edges() {
    run_case_with_inputs(
        "u128_umod_edges",
        include_str!("cases/case_u128_umod.rs"),
        &[
            (0, 0),
            (0, 1),
            (1, 0),
            (0xffffffff, 0),
            (0xffffffff, 0xffffffff),
            (0, 0xffffffff),
            (0x80000000, 1),
            (5, 3),
            (2, 7),
            (123456789, 987654321),
        ],
    );
}

/// u128 `/` and `%` boundary relations unreachable from the u128_udiv/
/// u128_umod input derivations: divisor == dividend exactly (n | 1 on odd n),
/// smallest divisor > dividend (even n), both-limbs-max operands, and
/// divisor 1 with a nonzero dividend — `/` and `%` use limb-swapped operand
/// pairs so the same-pair div+rem mul-sub fusion cannot elide either builtin.
#[test]
fn u128_bounds() {
    run_case("u128_bounds", include_str!("cases/case_u128_bounds.rs"));
}

/// Pinned edge grid for `u128_bounds`: (MAX, MAX) makes both operands
/// u128::MAX (MAX/MAX == 1, MAX%MAX == 0); (1, 0)/(0, 1) pin odd/even n and
/// m in both orders (divisor == dividend vs == dividend+1, and divisor-1
/// legs on the opposite operation); (0, 0) pins 0/1 and 0%1.
#[test]
fn u128_bounds_edges() {
    run_case_with_inputs(
        "u128_bounds_edges",
        include_str!("cases/case_u128_bounds.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0, 0),
            (1, 0),
            (0, 1),
            (2, 0),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x80000000, 0x80000000),
            (3, 3),
            (7, 5),
        ],
    );
}

/// i128 `/` with an odd (never-MIN) both-sign numerator and dynamic positive/
/// negative divisors — executes `__divti3`'s sign-fixup around the unsigned
/// division core on the VM.
#[test]
fn i128_sdiv() {
    run_case("i128_sdiv", include_str!("cases/case_i128_sdiv.rs"));
}

/// i128 `%` with an odd (never-MIN) both-sign numerator and dynamic positive/
/// negative divisors — executes `__modti3` (truncate-toward-zero remainder
/// signs) on the VM.
#[test]
fn i128_srem() {
    run_case("i128_srem", include_str!("cases/case_i128_srem.rs"));
}

/// Dynamic u128 `<<`/`>>` with counts in [0, 128) — executes the
/// compiler-builtins `__ashlti3`/`__lshrti3` two-limb funnel shifts (both
/// count < 64 and >= 64 legs) on the VM.
#[test]
fn u128_shifts() {
    run_case("u128_shifts", include_str!("cases/case_u128_shifts.rs"));
}

/// Pinned edge grid for `u128_shifts`: both shift counts (left = input2 &
/// 127, right = (input1 ^ input2) & 127) pinned to 0/1/63/64/65/127 (plus a
/// 126 row) — the funnel-shift limb-crossing boundaries of `__ashlti3`/
/// `__lshrti3`; rows with input1 == 0xFF give a byte-splat all-ones high
/// limb.
#[test]
fn u128_shifts_edges() {
    run_case_with_inputs(
        "u128_shifts_edges",
        include_str!("cases/case_u128_shifts.rs"),
        &[
            (0, 0),
            (1, 0),
            (1, 1),
            (0xff, 0x3f),
            (0x41, 0x01),
            (0x40, 0x40),
            (0, 0x41),
            (0x3f, 0x40),
            (0, 0x7f),
            (0xff, 0x7f),
            (0x3e, 0x01),
            (0xffffffff, 0x40),
            (0xff, 0x01),
        ],
    );
}

/// Dynamic i128 arithmetic `>>` on both-sign values — executes `__ashrti3`
/// including the sign-propagating count >= 64 leg (`i64.shr_s` fills the high
/// limb) on the VM.
#[test]
fn i128_ashr() {
    run_case("i128_ashr", include_str!("cases/case_i128_ashr.rs"));
}

/// Pinned edge grid for `i128_ashr`: w1's sign is input1 bit 31 and its
/// count is input2 & 127; w2's count is (input1 >> 3) & 127 (bits 3..9,
/// independent of the sign bit). Rows pin counts 0/1/63/64/65/127 on
/// negative AND positive values — count 127 on negative w1 is the full
/// `__ashrti3` sign-fill (result -1).
#[test]
fn i128_ashr_edges() {
    run_case_with_inputs(
        "i128_ashr_edges",
        include_str!("cases/case_i128_ashr.rs"),
        &[
            (0x80000000, 0),
            (0x80000008, 1),
            (0x800001f8, 63),
            (0x80000200, 64),
            (0x80000208, 65),
            (0x800003f8, 127),
            (0x000003f8, 127),
            (0x00000200, 64),
            (0, 1),
            (0x7ffffff8, 63),
            (0xffffffff, 0xffffffff),
        ],
    );
}

/// u128 `count_ones`/`leading_zeros`/`trailing_zeros` on dynamic values —
/// executes the i64 popcnt limb sum and the clz/ctz limb selects (both legs,
/// via parity-zeroed limbs) on the VM.
#[test]
fn u128_bits() {
    run_case("u128_bits", include_str!("cases/case_u128_bits.rs"));
}

/// u128 comparisons: branch/select position (strict two-limb lt/gt chains)
/// plus `#[inline(never)]` bool-value `<=`/`==` — executes the 128-bit
/// carry/borrow compare legalization on the VM.
#[test]
fn u128_cmp() {
    run_case("u128_cmp", include_str!("cases/case_u128_cmp.rs"));
}

/// Dynamic-count logical shifts and rotates on u32/u64 (wrapping_shl/shr,
/// rotate_left/right) — asserts the VM masks the count (`% width`) exactly
/// like Rust.
#[test]
fn shift_counts() {
    run_case("shift_counts", include_str!("cases/case_shift_counts.rs"));
}

/// Pinned edge grid for `shift_counts`: counts 0, 1, width-1, width, width+1,
/// 2*width, and over-2*width (67/96/131) on values 0/1/0x7FFFFFFF/0x80000001/
/// u32::MAX — boundary count pairs proptest essentially never draws.
#[test]
fn shift_counts_edges() {
    run_case_with_inputs(
        "shift_counts_edges",
        include_str!("cases/case_shift_counts.rs"),
        &[
            (0x80000001, 0),
            (0x80000001, 1),
            (0x80000001, 31),
            (0x80000001, 32),
            (0x80000001, 33),
            (0x80000001, 63),
            (0x80000001, 64),
            (0x80000001, 67),
            (0x80000001, 131),
            (1, 63),
            (0xffffffff, 32),
            (0x7fffffff, 31),
            (0, 64),
            (0xdeadbeef, 96),
        ],
    );
}

/// Arithmetic shift right on negative values with dynamic unmasked counts —
/// the edge arms of `::intrinsics::i32/i64::checked_shr` plus the constant
/// `>> 31` / `>> 63` sign-mask idiom.
#[test]
fn ashr_neg() {
    run_case("ashr_neg", include_str!("cases/case_ashr_neg.rs"));
}

/// Pinned edge grid for `ashr_neg`: MIN >> 0 == MIN, MIN >> width-1 == -1,
/// count == width masks to 0, over-width counts mask (67 -> 3), -1 >> c == -1;
/// row (0x80000000, 0) makes the i64 operand exactly i64::MIN.
#[test]
fn ashr_neg_edges() {
    run_case_with_inputs(
        "ashr_neg_edges",
        include_str!("cases/case_ashr_neg.rs"),
        &[
            (0x80000000, 0),
            (0x80000000, 31),
            (0x80000000, 32),
            (0x80000000, 63),
            (0x80000000, 64),
            (0x80000000, 67),
            (0xffffffff, 0),
            (0xffffffff, 1),
            (0x7fffffff, 31),
            (0x7fffffff, 63),
            (0, 63),
            (1, 31),
        ],
    );
}

/// Unsigned u32/u64 division/remainder with `| 1`-guarded dynamic divisors —
/// the boundary relations (divisor 1 / equal / greater, dividend 0, high-bit
/// divisors) of `u32div`/`u32mod` and miden-core-lib `u64::div`/`u64::mod`.
#[test]
fn udiv_bounds() {
    run_case("udiv_bounds", include_str!("cases/case_udiv_bounds.rs"));
}

/// Pinned edge grid for `udiv_bounds`: divisor 1 ((0,0), (MAX,1)), divisor ==
/// dividend ((5,5), (MAX,MAX)), divisor > dividend ((3,7), (1,MAX)), dividend
/// 0, and u64 divisors with the high bit set (largest-divisor path).
#[test]
fn udiv_bounds_edges() {
    run_case_with_inputs(
        "udiv_bounds_edges",
        include_str!("cases/case_udiv_bounds.rs"),
        &[
            (0, 1),
            (5, 5),
            (3, 7),
            (0xffffffff, 1),
            (1, 0xffffffff),
            (0, 0),
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0, 0x80000000),
            (0x80000000, 0),
            (7, 2),
            (2, 4),
        ],
    );
}

/// Signed i32 `/`+`%` and i64 `/` at the MIN/magnitude-1 boundaries, MIN/-1
/// unconstructible by design (odd numerators over negative divisors) — the
/// sign-fixup edges of `::intrinsics::{i32,i64}::checked_div`/`wrapping_mod`.
#[test]
fn sdiv_bounds() {
    run_case("sdiv_bounds", include_str!("cases/case_sdiv_bounds.rs"));
}

/// Pinned edge grid for `sdiv_bounds`: row (0x80000000, 0) forces i32 MIN/1,
/// (MIN|1)/-1 == MAX, i64::MIN/1, and (i64::MIN|1)/-1 == i64::MAX all at
/// once; rows (0x00008000, 0) / (0x00800000, 0) pin the rotated remainder
/// numerators to MIN % 1 and (MIN|1) % -1; other rows pin -1/1, 1/-1, 0/1.
#[test]
fn sdiv_bounds_edges() {
    run_case_with_inputs(
        "sdiv_bounds_edges",
        include_str!("cases/case_sdiv_bounds.rs"),
        &[
            (0x80000000, 0),
            (0x00008000, 0),
            (0x00800000, 0),
            (0x80000001, 0),
            (0xffffffff, 0),
            (0x7fffffff, 0),
            (1, 0),
            (0, 0),
            (0x80000000, 999),
            (0x80000000, 0x00010000),
            (0xdeadbeef, 123456),
            (12345, 4),
        ],
    );
}

/// Wrapping arithmetic at MIN/MAX: wrapping_neg/abs(MIN), MAX+1, MAX*MAX,
/// MIN sign-extending casts, u32::MAX widening to u64, and checked_add/sub/neg
/// None arms (LLVM legalizes checked ops to wrapping + compare).
#[test]
fn wrap_minmax() {
    run_case("wrap_minmax", include_str!("cases/case_wrap_minmax.rs"));
}

/// Pinned edge grid for `wrap_minmax`: i32/i64 MIN rows (0x80000000, 0),
/// u32 MAX+1 and MAX*MAX rows, i32 MAX+1 -> MIN, i64 MAX+1 -> MIN
/// ((0x7FFFFFFF, 0xFFFFFFFF)), and the checked-op overflow/underflow rows.
#[test]
fn wrap_minmax_edges() {
    run_case_with_inputs(
        "wrap_minmax_edges",
        include_str!("cases/case_wrap_minmax.rs"),
        &[
            (0x80000000, 0),
            (0x80000000, 1),
            (0xffffffff, 1),
            (0xffffffff, 0xffffffff),
            (0x7fffffff, 1),
            (0x7fffffff, 0xffffffff),
            (0, 0),
            (0, 1),
            (1, 0xffffffff),
            (0xaaaaaaaa, 0x55555555),
        ],
    );
}

/// leading_zeros/trailing_zeros/count_ones of exactly 0 and MAX on u32, u64,
/// and u128 — the clz(0) == width / ctz(0) == width saturation arms of the
/// bit-count intrinsics, never differentially asserted before.
#[test]
fn bitcnt_zero() {
    run_case("bitcnt_zero", include_str!("cases/case_bitcnt_zero.rs"));
}

/// Pinned edge grid for `bitcnt_zero`: the all-zero row (0, 0) (clz/ctz
/// saturate at 32/64/128), the all-ones row, and limb-boundary single-bit
/// rows — ctz(1<<32) == 32 via (1, 0), clz == 32 via (0, 0x80000000).
#[test]
fn bitcnt_zero_edges() {
    run_case_with_inputs(
        "bitcnt_zero_edges",
        include_str!("cases/case_bitcnt_zero.rs"),
        &[
            (0, 0),
            (0xffffffff, 0xffffffff),
            (0, 1),
            (1, 0),
            (0x80000000, 0),
            (0, 0x80000000),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x00010000, 0),
            (0xffff0000, 0x0000ffff),
        ],
    );
}

/// Zero- and boundary-length memory ops with disjoint ranges: element copies
/// of length 0/1, byte copies of length 0..=5 across the memcpy `% 4`
/// fastpath boundary, and a byte fill of length 0 — with fixed-index reads
/// asserting length-0 ops wrote nothing.
#[test]
fn memlen_zero() {
    run_case("memlen_zero", include_str!("cases/case_memlen_zero.rs"));
}

/// Pinned edge grid for `memlen_zero`: (0,0) makes every copy/fill length 0;
/// (1,1) exactly 1; (4,4) puts the byte copy exactly on the element-fastpath
/// boundary (count 4); 2/3/5 pin the byte-tail fallback loop lengths.
#[test]
fn memlen_zero_edges() {
    run_case_with_inputs(
        "memlen_zero_edges",
        include_str!("cases/case_memlen_zero.rs"),
        &[
            (0, 0),
            (1, 1),
            (4, 4),
            (5, 2),
            (3, 3),
            (2, 0),
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (7, 9),
            (12, 10),
        ],
    );
}

/// DELIBERATE PROBE: zero-length copy at an identical src == dst position
/// (opaquely-zero length and dst offset survive to a runtime memory.copy) —
/// length-0 ranges cannot overlap, so any VM-side abort would be a real
/// divergence in the memcopy_elements overlap assert.
#[test]
fn memnoop_same() {
    run_case("memnoop_same", include_str!("cases/case_memnoop_same.rs"));
}

/// Pinned edge grid for `memnoop_same`: positions p = 0..=3 (including via
/// u32::MAX % 4) for the len-0 src == dst copy; every row must be a no-op on
/// both sides.
#[test]
fn memnoop_same_edges() {
    run_case_with_inputs(
        "memnoop_same_edges",
        include_str!("cases/case_memnoop_same.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 3),
            (3, 2),
            (0xffffffff, 0),
            (0x80000004, 4),
            (7, 11),
            (123456789, 987654321),
        ],
    );
}

/// While-loops with `% 97`-derived trip counts and loop-carried u32/u64
/// values — the zero-trip (guard skips the rotated body) and one-trip edge
/// behavior of lifted scf.while regions at runtime.
#[test]
fn trip_loops() {
    run_case("trip_loops", include_str!("cases/case_trip_loops.rs"));
}

/// Pinned edge grid for `trip_loops`: trip counts exactly 0 and 1 for both
/// loops in all combinations, including the modulus wrap rows 97 -> 0 and
/// 98 -> 1, plus a full-range row.
#[test]
fn trip_loops_edges() {
    run_case_with_inputs(
        "trip_loops_edges",
        include_str!("cases/case_trip_loops.rs"),
        &[
            (0, 0),
            (1, 1),
            (0, 1),
            (1, 0),
            (97, 97),
            (98, 98),
            (2, 1),
            (96, 2),
            (0x80000000, 1),
            (0xffffffff, 0xffffffff),
        ],
    );
}

/// ~400 non-reassociable mixed u32/u64 ops in one basic block (four
/// right-leaning sub/rotate/xor waves plus select combiners) — operand
/// scheduling and single-block spill/reload at ~15x the corpus scale.
#[test]
fn chain300() {
    run_case("chain300", include_str!("cases/case_chain300.rs"));
}

/// A single dense `match h & 63` with 64 structurally-varied arms — wasm
/// `br_table` with 64 targets and switch lowering at 8x the corpus's
/// previous width.
#[test]
fn match64() {
    run_case("match64", include_str!("cases/case_match64.rs"));
}

/// Twelve levels of mixed while-loop/conditional nesting with all state
/// threaded through every level — cfg-to-scf structural recursion and scf
/// region nesting at depth.
#[test]
fn deep_nest() {
    run_case("deep_nest", include_str!("cases/case_deep_nest.rs"));
}

/// Thirty #[inline(never)] helpers: a 20-deep non-recursive call chain with
/// per-level fan-out to ten leaves — 30 MAST procedure digests and a
/// 21+-frame VM call stack at runtime.
#[test]
fn call_web() {
    run_case("call_web", include_str!("cases/case_call_web.rs"));
}

/// 24 odd-size statics (u8/u16/u32/u64 tables), three restored AtomicU32
/// mutables, and a ~4KB const-generated table — data-segment layout and
/// runtime reads at segment counts the corpus never had.
#[test]
fn seg24() {
    run_case("seg24", include_str!("cases/case_seg24.rs"));
}

/// i8/i16 sign boundaries (0x7F/0x80/0xFF, 0x7FFF/0x8000/0xFFFF) through
/// sign-extending table loads (i32/i64.load8_s/16_s) and `as i8/i16 as
/// i32/i64` truncate-sign-extend chains.
#[test]
fn subword_sign() {
    run_case("subword_sign", include_str!("cases/case_subword_sign.rs"));
}

/// Pinned edge grid for `subword_sign`: exact boundary bytes/halfwords into
/// the extend chains (0x7F/0x80/0xFF, 0x7FFF/0x8000/0xFFFF), table indexes
/// hitting the MIN/MAX/-1 entries, and truncation-before-sext rows
/// (0x100 -> 0, 0xFF80 -> -128, 0xFFFF8000 -> -32768).
#[test]
fn subword_sign_edges() {
    run_case_with_inputs(
        "subword_sign_edges",
        include_str!("cases/case_subword_sign.rs"),
        &[
            (0x7f, 0x7fff),
            (0x80, 0x8000),
            (0xff, 0xffff),
            (0, 1),
            (1, 0),
            (2, 2),
            (0x100, 0x10000),
            (0x17f, 0x17fff),
            (0xff80, 0xffff8000),
            (5, 3),
        ],
    );
}
