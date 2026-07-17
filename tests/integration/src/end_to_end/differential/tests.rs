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

/// Exercises integer width conversions and per-width bit-counting/`bnot`
/// arms in `codegen/masm/src/emit/unary.rs`.
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

/// Exercises scf.while canonicalization: duplicated yielded results, results
/// unused after the loop, and loop-invariant carried values.
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
