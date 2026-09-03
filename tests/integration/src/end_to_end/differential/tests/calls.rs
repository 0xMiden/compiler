//! Function-call boundaries: sret aggregates, at-limit signatures, call placement.

use super::super::harness::{run_case, run_case_with_inputs};

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

/// Mixed-width signatures at and near the 16-felt limit (campaign 14): seven
/// u64 plus two u32, 14 u32 + 1 u64, a u128-returning helper whose hidden return-area
/// pointer is the sixteenth felt (7 u64 + u32 + sret), and 15-felt helpers
/// taking three u128 parameters (scalarized to i64 pairs) with a u64 and a
/// `(u64, u64)` return area — four u64 values live across every call.
#[test]
fn call_sigs16() {
    run_case("call_sigs16", include_str!("../cases/case_call_sigs16.rs"));
}

/// Wide results returned by value through return-area pointers: a padded
/// `repr(C)` record with a word-aligned u128 field, a `repr(C, packed)`
/// record with its u128 at byte offset 1, a 13-byte array, a `(u64, u64)`
/// tuple, `Option<u128>` / `Result<u64, u32>`, a per-trip u128 helper result
/// in a loop (one return area reused per iteration) and a helper that
/// forwards its own return-area pointer to its callee.
#[test]
fn ret_area() {
    run_case("ret_area", include_str!("../cases/case_ret_area.rs"));
}

/// Calls inside loops with loop-carried values across them: four u64 and
/// two u32 carried across two pinned calls per trip of a zero-trip-capable
/// loop (call arguments reused after the call), a call in one `match` arm
/// only beside a `continue` arm, a call result deciding an early `return`,
/// a bottom-test inner loop calling a helper on the carried state, and a
/// call result deciding the outer break.
#[test]
fn loop_calls() {
    run_case("loop_calls", include_str!("../cases/case_loop_calls.rs"));
}

/// Pinned exit / 0-trip / 1-trip inputs for `loop_calls`: the outer loop
/// skipped (input2 % 41 == 0), single trips, the `return` from the `match`
/// arm and the call-decided `break`.
#[test]
fn loop_calls_edges() {
    run_case_with_inputs(
        "loop_calls_edges",
        include_str!("../cases/case_loop_calls.rs"),
        &[
            (0, 0),
            (41, 41),
            (1, 1),
            (3, 2),
            (0x7fff_ffff, 0xffff_ffff),
            (7, 40),
            (0, 29),
            (1, 10),
        ],
    );
}

/// Indirect dispatch under operand pressure: a runtime-indexed table of
/// 7-u64 fn pointers (15 of the 16 window felts with the table index)
/// dispatched inside a loop whose index is loop-carried while SIX u64
/// locals stay live across the dispatch (each is an argument and is used
/// again afterwards), then `dyn Trait` methods returning a u128 (return
/// area + receiver + four u64) under the same live state. Six is the
/// largest live-local count that compiles; seven is the `indirect_spill`
/// panic below.
#[test]
fn dispatch_pressure() {
    run_case("dispatch_pressure", include_str!("../cases/case_dispatch_pressure.rs"));
}

/// COMPILE-TIME COMPILER PANIC (safe Rust, campaign 14, 2026-09-02): the
/// LOOP form of the `hir.exec_indirect` argument-blindness class (loop-free
/// minimal form: `indirect_spill_line`; emitter-side signature:
/// `indirect_spill_args`). Building this case panics with `NoSolution` at
/// codegen/masm/src/lower/lowering.rs:109 while scheduling the loop body's
/// `arith.bxor` `[Move, Copy]` over a SEVENTEEN-felt operand stack (eight
/// u64 + the loaded fn pointer). Shape: seven loop-invariant u64 locals are
/// the arguments of a 7-u64 fn-pointer dispatch inside a loop with a
/// loop-carried table index, and a u64 accumulator is carried across the
/// dispatch (one local more than the passing `dispatch_pressure`). Root
/// cause (`MIDENC_TRACE='analysis:spills=trace'`): the spill analysis takes
/// an operation's inputs from operand group 0 only
/// (hir-analysis/src/analyses/spills.rs, `op.operands().group(0)` at the
/// generic scheduling site ~2291 and at ~993/~2470), but `hir.exec_indirect`
/// keeps only the u32 table index in group 0 and its ARGUMENTS in group 1
/// (dialects/hir/src/ops/invoke.rs, `#[operand] index` + `#[operands]
/// arguments`). In the loop body the seven argument loads (`hir.load_local`
/// of the DWARF-kept wasm locals) plus the accumulator and the table-index
/// math exceed the window, so the analysis spills four of the arguments
/// and, blind to their use at the dispatch, never reloads them ("required
/// by reloads = 0", "freed by op = 1", four arguments in S^entry at the
/// dispatch). Spills materialize as `store_local` copies, so the emitter
/// keeps the four values physically until the dispatch consumes them and
/// the body is scheduled over 17 felts. No "unused phi" warning (not F1),
/// no edge split needed (not F6), out-of-contract stack (not the arity-2 F2
/// gap). Panic-only: the emitter schedules the real values and every spill
/// slot holds the correct value, so no silent miscompile is possible.
/// Bounded by `dispatch_pressure` (six such locals, passes), `direct_loop`
/// (the SAME loop with eight locals and a DIRECT 7-u64 call: `hir.exec`
/// keeps its arguments in group 0, passes) and `indirect_args` (eight
/// locals across two straight-line dispatches, passes: the argument loads
/// happen right before each dispatch with nothing else live). Compile-time
/// — no inputs involved. Un-ignore when this case compiles (the spill
/// analysis counts every operand group of `hir.exec_indirect`).
#[test]
#[ignore = "compiler panic: 'with error: NoSolution' at codegen/masm/src/lower/lowering.rs:109 on \
            a 17-felt operand stack — the spill analysis reads only operand group 0 and so never \
            sees the arguments of hir.exec_indirect (group 1): spilled dispatch arguments are \
            never reloaded and the call is budgeted as one felt (compile-time, no inputs involved)"]
fn indirect_spill() {
    run_case("indirect_spill", include_str!("../cases/case_indirect_spill.rs"));
}

/// Straight-line twin of `indirect_spill`: eight u64 locals used by two
/// fn-pointer dispatches with no loop — every argument is loaded right
/// before its dispatch with nothing else live, so no spill is needed and
/// the group-0-only accounting is harmless here (twelve such locals pass
/// too; probe deleted).
#[test]
fn indirect_args() {
    run_case("indirect_args", include_str!("../cases/case_indirect_args.rs"));
}

/// Caller pressure meeting callee pressure: six u64 values live across
/// pinned calls whose callees spill internally (a 20-felt right-leaning
/// tree returning a `(u64, u64)` pair, and a 16-felt-signature helper using
/// every parameter twice) — frames and spill slots across the boundary.
#[test]
fn callee_pressure() {
    run_case("callee_pressure", include_str!("../cases/case_callee_pressure.rs"));
}

/// Campaign-12 lead check: u128/i128 `checked_add`/`overflowing_add`/
/// `saturating_sub`/`checked_sub`/`overflowing_sub` (`i64.add128` /
/// `i64.sub128` on this toolchain) in `#[inline(never)]` helpers and a loop
/// — the forms that expose the F9 guest-LLVM defect for `mul_wide_s` — with
/// both limbs at their boundaries; passes, so the defect does not extend to
/// the 128-bit add/sub instructions.
#[test]
fn add128_checked() {
    run_case("add128_checked", include_str!("../cases/case_add128_checked.rs"));
}

/// Helpers taking `&mut` stack arrays and runtime-bounded slices (fat
/// pointers): a 15-felt pointer+scalar signature writing through two
/// arrays, an in-place `swap` permutation, shared-reference reads, with u64
/// scalars live between the calls and elements read back at runtime indexes.
#[test]
fn mut_arrays() {
    run_case("mut_arrays", include_str!("../cases/case_mut_arrays.rs"));
}

/// Bounded recursion THROUGH A FUNCTION-POINTER TABLE (depth `input1 % 6`,
/// non-tail, per-frame state): no direct call edge closes the cycle, so the
/// linker's call-graph cycle check does not fire, each frame dispatches its
/// callee with `call_indirect` (`dynexec`), and the recursion executes
/// correctly on the VM — direct or mutual recursion is still a clean "found
/// a cycle in the call graph" linker error (campaign-14 probe, deleted).
#[test]
fn recursion_indirect() {
    run_case("recursion_indirect", include_str!("../cases/case_recursion_indirect.rs"));
}

/// Pinned depths 0..5 and every table index for `recursion_indirect`.
#[test]
fn recursion_indirect_edges() {
    run_case_with_inputs(
        "recursion_indirect_edges",
        include_str!("../cases/case_recursion_indirect.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 2),
            (3, 0),
            (4, 1),
            (5, 2),
            (11, 4),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// Direct-call loop twin of `indirect_spill`: the same bottom-test loop with
/// EIGHT loop-invariant u64 locals as the arguments of a pinned direct 7-u64
/// call and a u64 accumulator carried across it — `hir.exec` keeps its
/// arguments in operand group 0, so the spill analysis reloads them and the
/// shape compiles and passes (seven pass as well; probe deleted).
#[test]
fn direct_loop() {
    run_case("direct_loop", include_str!("../cases/case_direct_loop.rs"));
}

/// COMPILE-TIME COMPILER PANIC — the loop-free minimal form of the
/// `indirect_spill` class (campaign 14 attempt 2, 2026-09-03): a
/// straight-line 7-u64 fn-pointer dispatch with two single-use u64 helper
/// results computed before it and consumed after it (LLVM stackifies them
/// UNDER the dispatch, so they are SSA values live across
/// `hir.exec_indirect` in one block: 14 argument felts + the table index +
/// 4 felts live-through = 19). The spill analysis, reading operand group 0
/// only, spills two of the arguments ("required by reloads = 0", "freed by
/// op = 1", two argument limbs in S^entry) and never reloads them; the
/// emitter still holds them and the dispatch ITSELF is scheduled over a
/// 17-felt stack: `NoSolution` at codegen/masm/src/lower/lowering.rs:109
/// `for inst 'hir.exec_indirect'`, constraints all `Move`. Bounded by
/// `direct_line` (the same shape with a pinned direct call, passes) and
/// `indirect_wide` (the same 7-u64 dispatch with nothing live across it,
/// passes); one live-through u64 still fits (probe deleted). Compile-time —
/// no inputs involved. Un-ignore together with `indirect_spill`.
#[test]
#[ignore = "compiler panic: 'with error: NoSolution' at codegen/masm/src/lower/lowering.rs:109 \
            scheduling hir.exec_indirect itself over a 17-felt stack — the spill analysis is blind \
            to the dispatch arguments (operand group 1); compile-time, no inputs involved"]
fn indirect_spill_line() {
    run_case("indirect_spill_line", include_str!("../cases/case_indirect_spill_line.rs"));
}

/// Direct-call twin of `indirect_spill_line`: the same two single-use helper
/// results stackified under a pinned direct 7-u64 call — passes.
#[test]
fn direct_line() {
    run_case("direct_line", include_str!("../cases/case_direct_line.rs"));
}

/// COMPILE-TIME COMPILER PANIC — third signature of the `indirect_spill`
/// class (campaign 14 attempt 2, 2026-09-03): a loop-free 7-u64 fn-pointer
/// dispatch whose fourth and sixth arguments are rotated IN PLACE from two
/// more u64 locals by runtime counts, so the argument setup alone loads
/// nine u64 (18 felts) before the dispatch. The spill analysis (blind to the
/// group-1 arguments) spills four of them and never reloads them; the
/// emitter keeps them physically and the first arity-1 `arith.trunc` of a
/// Copy-constrained deep u64 aborts in the EMITTER rather than the solver:
/// `invalid operand stack index (10): requires access to more than 16
/// elements` at codegen/masm/src/emit/mod.rs:623 (`copy_operand_to_position`
/// → `dup`). The same class also surfaces as `invalid stack offset for
/// movup: 17 is out of range` (emit/mod.rs:758) for a `dyn Trait` method
/// dispatch with two in-place-computed u64 arguments (probe deleted) and as
/// a 17-felt `arith.shl` `NoSolution` for fn pointers taking three u128
/// parameters in a loop (see `indirect_u128`). Bounded by `direct_args`
/// (the same in-place arguments through a pinned direct call, passes) and
/// `bands_calls` (tests/compose.rs: the dispatch takes plain locals only,
/// passes). Compile-time — no inputs involved. Un-ignore together with
/// `indirect_spill`.
#[test]
#[ignore = "compiler panic: 'invalid operand stack index (10): requires access to more than 16 \
            elements' at codegen/masm/src/emit/mod.rs:623 — the spill analysis is blind to the \
            dispatch arguments (operand group 1); compile-time, no inputs involved"]
fn indirect_spill_args() {
    run_case("indirect_spill_args", include_str!("../cases/case_indirect_spill_args.rs"));
}

/// Direct-call twin of `indirect_spill_args`: the same seven u64 locals, two
/// of them rotated in place by runtime counts, passed to a pinned direct
/// 7-u64 call — passes.
#[test]
fn direct_args() {
    run_case("direct_args", include_str!("../cases/case_direct_args.rs"));
}

/// recursion_indirect x indirect_wide: bounded recursion THROUGH a
/// fn-pointer table with a five-u64 signature (ten argument felts + the
/// table index), depth `input1 % 6`, every frame keeping two u64 of state
/// live across its dispatch (non-tail), the callee chosen per frame from
/// the frame's own state, and two independent recursions whose depths come
/// from both inputs.
#[test]
fn recursion_wide() {
    run_case("recursion_wide", include_str!("../cases/case_recursion_wide.rs"));
}

/// narrow64 x ret_area x lane_bytes: helpers return records with u8 / u16 /
/// u32 / u64 fields by value (i64.store8 / store16 / store32 of truncated
/// values into the return area), a padded `repr(C)` record and a `repr(C,
/// packed)` one with its u64 at byte offset 1, collected into stack arrays
/// filled in a loop, read back at runtime indexes and folded by a helper
/// taking slices of both arrays.
#[test]
fn narrow_ret() {
    run_case("narrow_ret", include_str!("../cases/case_narrow_ret.rs"));
}

/// ret_area x call_indirect: fn pointers whose signatures RETURN wide values
/// by value — `fn(u64, u64) -> u128`, `fn(u32, u64) -> (u64, u64)`,
/// `fn(u64) -> Option<u128>` — dispatched from runtime-indexed tables, so
/// the hidden return-area pointer is the first argument of
/// `hir.exec_indirect` and the callee writes through it; dispatched in a
/// loop with the u128 result feeding the next trip's index and arguments.
#[test]
fn sret_dispatch() {
    run_case("sret_dispatch", include_str!("../cases/case_sret_dispatch.rs"));
}

/// recursion_indirect x mut_arrays: bounded recursion THROUGH a fn-pointer
/// table (depth `input1 % 6`) where every frame owns a `[u64; 6]` that
/// escapes by `&mut` into the callee frame (address-taken locals in
/// recursive frames: each dynexec level pushes its own shadow-stack frame
/// while the caller's array stays live), the callee writes it and the frame
/// reads it back after the call together with its own state.
#[test]
fn recursion_frames() {
    run_case("recursion_frames", include_str!("../cases/case_recursion_frames.rs"));
}

/// div128_guards x indirect_wide: fn pointers taking TWO u128 parameters
/// (four i64 limbs, eight felts) and returning a u128 through a return area,
/// dispatched in a loop whose carried state is three u128s, with u128 / i128
/// `checked_div` / `checked_rem` against divisors reaching 0 / -1 / MIN and
/// 128-bit shifts by runtime counts in the callees. Two plain u128 locals is
/// the boundary: three u128 parameters, or two with one argument computed
/// in place, hit the `indirect_spill` class (17-felt `arith.shl`
/// `NoSolution` while the limbs are loaded for the dispatch; probes deleted).
#[test]
fn indirect_u128() {
    run_case("indirect_u128", include_str!("../cases/case_indirect_u128.rs"));
}
