# fuzza knowledge base

Accumulated, verified facts from past coverage campaigns: compiler
reachability facts, LLVM pre-cleaning traps, case-writing tricks, and
operational gotchas. This file is **committed** and is required reading for
every agent before it writes a single case; the per-run logs under `scratch/`
are gitignored and machine-local, so anything durable discovered there must be
promoted here or it is lost to the next clone.

Maintenance rules:

- Add a fact only once it is *verified* (probe, WAT/HIR dump, region-level
  coverage evidence, or a committed reproducer case) — cite the case or probe
  that established it.
- Refine or strike facts that a compiler change invalidates.
- Only durable facts belong here. Known bugs are documented at their
  `#[ignore]`d reproducer tests (see below), and per-run coverage state lives
  in the reports — neither is duplicated in this file.
- Do **not** re-derive anything recorded here.

## Toolchain & pipeline facts

- `cargo-miden` builds with RUSTFLAGS target features `+bulk-memory` and
  `+wide-arithmetic` only — **no `+multivalue`**. Consequences: multi-value
  wasm returns are impossible; tuple/struct/array/u128 returns all lower to
  sret pointers into the caller's frame; u128 *parameters* scalarize to i64
  pairs; multi-result `hir.exec` is unreachable from Rust.
- rustc `-O3` (LLVM) pre-cleans aggressively:
  - Constant-constant arithmetic is folded before wasm — a two-const HIR arith
    op is rare; single-constant operands are everywhere.
  - Known-bits-provable values fold: `(x | 1).trailing_zeros()` becomes `0`;
    known-bits-impossible guards get deleted. Dynamically-impossible-but-opaque
    guards need cross-modulus contradictions (`h % 6 == 5 && h % 3 == 0`).
  - Loop bounds like `x & 7` are fully peeled; `% 97`-style moduli survive.
  - Everything is inlined unless `#[inline(never)]`; obvious tail recursion
    becomes a loop (and any *surviving* recursion is a linker error anyway).
  - Multi-use defs are tee'd — the same SSA value never appears as both
    operands of one op (`v * v` gets distinct HIR values).
  - Identical trailing code in branch arms is merged (sink-common-code), which
    can silently drop your arm-local pressure below thresholds.
- **The locals argument** (kills many transform paths): LLVM's RegStackify
  keeps values on the wasm operand stack only *within one basic block*, and
  multivalue is off, so every cross-block or multi-use value travels through a
  wasm local → unpromoted `hir.load_local`/`store_local` (`Local2Reg` promotes
  only single-load locals). Consequences, all probe-verified:
  - HIR joins have no block parameters; `scf.while` always has **zero** iter
    args and zero yield operands (only cfg-to-scf's own synthesized
    discriminators thread as SSA).
  - The five scf while/switch arg-and-result canonicalization interiors and
    cfg-to-scf undef/latch threading are structurally unproducible.
  - Spill-analysis W at any block/region boundary holds at most one value →
    proactive block-arg spilling, `spill_trailing_until_fits`, loop-header
    `w_used >= K` arms, and region-branch spill arms are unproducible.
  - The >16-felt pressure differential cases trigger is largely
    *self-inflicted*: the frontend batches `load_local`s at block tops and
    `SinkOperandDefs` sinks arithmetic but not loads (original wasm operand
    depth was ~5). `align_branch_stack` realignment is defensive
    dead-in-practice code — dynamic branch joins always arrive aligned.
- LLVM's wasm backend never emits `if`/`else` (only `block` + `br_if`; no
  wasm-opt in the pipeline) and always rebases `br_table` to 0.
- Signed-compare canonicalization: `<=`/`>=` in *branch or select position* is
  always turned into a strict compare with inverted arms. `le_s`/`ge_s` (and
  the `lte`/`gte` emitter arms) are reachable **only** by materializing the
  boolean as a value inside a `#[inline(never)]` helper (`case_scmp_bool.rs`).
- The harness prepends a `loop {}` panic handler, so `panic!` **never** lowers
  to wasm `unreachable`. To get a genuine trap edge, plant
  `core::arch::wasm32::unreachable()` behind an impossible cross-modulus guard
  (`case_unreachable_exits.rs`).
- `Operator::CallIndirect` is `todo!()` in the frontend — function-pointer /
  dyn-dispatch cases panic the compiler. Recursion (self or mutual) is a clean
  "found a cycle in the call graph" linker error. Neither is testable.
- Flat function signatures are capped at **16 stack felts** (16×u32 or 8×u64 is
  the at-limit case, `case_wide_calls.rs`); one felt more currently fails the
  build inside the spill analysis — treat wider signatures as unwritable, not
  as a novel finding.

## Frontend routing facts (Rust → wasm → HIR → emitter)

- Rust `as` casts become `trunc`/`zext`/`sext`/`bitcast` — **never** HIR
  `cast`. `OpEmitter::cast` and its ~500-region helper cluster are unreachable
  (hir.Cast is created only by canon-ABI glue, felt intrinsics, and local type
  mismatches; the only mismatches wasm typing permits take the bitcast special
  case in the translator).
- `_imm` binary emitter variants are called only from `#[cfg(test)]` code,
  except `eq/lt/gt/lte_imm`, which switch lowering calls with **U32 selectors
  only**. `shr_imm_*` is dead: `arith::Shr` lowering always calls `shr()`;
  constant shift counts are materialized as pushed operands.
- Memory-op immediate/typed arms: the `load_imm` family has only unit-test
  callers; `store_imm` non-u32 arms require GlobalVariables (only
  `__stack_pointer` exists); felt load/store is unobservable (no f32
  reinterpret support); `repr(packed)` / dynamically-unaligned access adds
  nothing (dynamic-pointer load/store delegates wholesale to intrinsics —
  alignment branching is imm-pointer-only); wasm `memory.copy` is always
  u8-typed (typed memcpy arms dead).
- Unsigned translators bitcast U32/U64 operands to I32/I64 around every op, so
  U-typed arithmetic emitter arms (`add_u64`, `mul_u64`, smallint
  add/sub/mul/div/mod, …) are dead; U8/U16/U32-*typed* HIR values arise only
  from widening loads (`case_loadwiden.rs`).
- Ops the wasm frontend **never builds** (their emitter arms and
  `schedule_operands` monomorphizations are unreachable regardless of Rust
  source): `min`/`max` (LLVM emits compare+select), `neg`/`not`/`incr`,
  `*_overflowing`/checked ops (LLVM legalizes to wrapping + compare),
  `clo`/`cto`, `ilog2`/`pow2`/`exp`/`is_odd`/`inv`, `ext2*`,
  `Sdiv`/`Smod`/`Sdivmod` (signed div/rem map to `Div`/`Mod` on signed
  *types*; the `Sdiv`/`Smod` lowerings are live `todo!()`s but unreachable),
  I1 `and`/`or`/`xor`.
- hir `Call`/`Syscall`/`ExecFpi` (cross-context / FPI) ops, their lowerings,
  and `process_call_signature`'s sret-assert and extension-marker arms are
  built only by SDK/component paths — no producer exists in a plain no_std
  core module.
- `i64.mul_wide_s`/`mul_wide_u` sign/zero-extend **both** operands to 128-bit
  at translation; a constant multiplicand is the **only** Rust-reachable
  constant-operand `sext`/`zext` (feeding `Sext::fold`/`Zext::fold`'s 128-bit
  arms). `ArithDialect::materialize_constant` coerces immediates via `as_u64`
  and **rejects negative i64 constants** — use positive constants when a fold
  matters.
- `wasm.SignExtend` (extend8/16/32_s) lowers to a `trunc(src)` + `sext(dst)`
  pair (`case_sext_widths.rs`).
- LLVM legalizes u128 compares/bitwise/popcounts to i64 limb ops — the i128
  emitter arms for those are unreachable; only add128/sub128/mul_wide reach
  int128.rs (`case_u128_mix.rs`). Runtime-confirmed (2026-07-23): compares are
  two-limb strict `lt_u/gt_u + eq + select` chains (an inline `i64.le_u` pair
  survives in *select* position; branch position is strict-only, and the
  bool-value form still needs the `#[inline(never)]` helper trick,
  `case_u128_cmp.rs`); clz/ctz are limb selects, popcount a limb sum
  (`case_u128_bits.rs`).
- u128/i128 `/` and `%` compile to compiler-builtins functions
  (`__udivti3`/`__umodti3`/`__divti3`/`__modti3`, all delegating to
  `specialized_div_rem::u128_div_rem`) compiled into the guest wasm as
  ordinary functions — no new emitter arms, but deep u64 clz/shift/subtract
  loops that execute differentially and PASS on the VM
  (`case_u128_udiv/u128_umod/i128_sdiv/i128_srem.rs`, wat-verified 2026-07-23).
- **Dynamic**-count 128-bit shifts become compiler-builtins libcalls
  (`__ashlti3`/`__lshrti3`/`__ashrti3`), not inline select chains — constant
  counts (e.g. the `<< 64` limb-construction idiom) fold away at compile time
  instead. Both count legs (< 64 / >= 64) and the `i64.shr_s` sign-fill
  execute and pass (`case_u128_shifts.rs`, `case_i128_ashr.rs`).
- `memory.size` gets CSE'd even across stores; the rewrite pipeline order is
  Canonicalizer → CSE → SCCP (same `op.fold`) — SCCP cannot out-fold the
  canonicalizer, and the `Foldable::fold_with` family is dead API
  workspace-wide (SCCP computes `constant_operands` and then drops them).
- `CanonicalizeI64RotateBy32ToSwap` never fires on wasm-derived IR: the
  translator wraps every dynamic shift/rotate count in `arith.band`
  (`mask_movement_count`), which hides constant counts from the pattern.

## Block-emitter operand-drop facts (codegen/masm emitter.rs / emit/mod.rs / stack.rs)

Verified 2026-07-23 (three probed cases — fresh-valued multi-exit loops,
nested labeled blocks, three-level nested-loop exits — all +0 area; HIR dumps
corroborate each closure below):

- **The post-op drop never fires.** `drop_unused_operands_at`'s per-op call
  site (emit_inline closure#1) can never find a dead operand: between
  consecutive ops, stack-value liveness only changes by consumption or by a
  last use inside the op's regions — and region bodies only use their own
  load_locals plus cfg-to-scf columns, with the scf lowerings reconciling the
  parent stack themselves. Everything dead is dropped at block entry
  (closure#0); the post-op scan interior is unreachable.
- **Block-entry drops always see uniform liveness.** Inherited stacks at
  emit_inline entries hold at most one cfg-to-scf payload column (+ the
  selector, which emit_switch_region/emit_linear_search drop out-of-band via
  `drop_operand_at_position` at index 0). Dispatch arms forward ALL their
  level's columns or NONE — the levels chain, they never stack — so entries
  are all-live (no drop) or all-dead (whole-stack batch, warm). The
  solver path and the used/unused interleave arms of the pathological branch
  are unreachable from wasm-derived IR.
- **scf.while `after` regions are always trivial latches**: `^block(args…):
  scf.yield` with every arg dead and dropped at index 0 — the swap/movup arms
  of `drop_operand_at_position` have no producer (dead scf results are
  canonicalized away, selectors sit on top, argc>=2 rets are impossible).
- **`truncate_stack` only ever takes its `num_to_drop == 0` fast path**: Ret
  argc <= 1 (no multivalue) and the pre-terminator/dead-result drops leave
  exactly the ret operands on the stack; a leftover duplicate would need a
  multi-use stack-resident value that is also a ret operand (multi-use Rust
  values are locals).
- **Dead parameter space in `copy/move_operand_to_position`**: the only
  callers (solver `solve_and_apply`) pass `m=0, is_commutative=false`; every
  `m>0` and commutative arm is dead API.
- **No sub-word/felt immediates**: `push_immediate`'s I8/I16/U16/Felt arms
  (and `emit_push::<u16>/<Felt>`) are unreachable — wasm consts are i32/i64
  and U8/U16-typed values only arise from non-constant widening loads.
- **`arith.Bnot` has no wasm-frontend producer** (built only by the MASM
  lifter and the builder API; Rust `!x` stays `bxor x, -1`), so
  `OpEmitter::bnot` and its `emit_repeat`/`emit_template` 64/128-bit arms are
  unreachable; `emit_all::<[_;13]>/<[_;14]>` likewise (callers are the
  checked/overflowing `mul_u64` arms the frontend never builds).
- **`OperandStack::get` is SDK-only** (emit/events.rs, emit/merkle.rs);
  `IndexMut` remains closed with the same-value-operand-pair fact. Refinement:
  cfg-to-scf DOES synthesize repeated-operand lists (`scf.yield %v, %v, %v,
  %v, %d` observed), but yields are no-op lowerings — the "same-SSA-value
  operand pairs unreachable" fact still holds for solver-scheduled ops.

## Case-writing tricks that work

- Runtime-indexed local arrays defeat SROA → real loads/stores; `[0u32; N]`
  initializers become `memory.fill` (covers `memset` wholesale).
- Runtime-length `copy_from_slice`/`copy_within` → `memory.copy` → both the
  element fast path and the byte fallback of `memcpy` at compile time.
- Atomic statics are the safe mutable statics (`.data` segment beside
  `.rodata`) — **restore them before returning**: the native cdylib is loaded
  once and reused across all 16 proptest inputs.
- Deep right-leaning *non-reassociable* expression trees (sub/rotl/xor mix)
  stay stackified → >16 live felts → single-block spills. Ten u64s live across
  a branch/loop → the CFG-form spill transform (`case_spill_branch.rs`,
  `case_spill_loop.rs`).
- Exactly-16-felt call signatures with u64s live across the call sites
  activate the `linear_stack_window` scheduling tactic
  (`case_wide_calls.rs`). Tactic note: with default fuel, every applicable
  tactic runs on every arity≥3 problem — tactic coverage is about *interior
  arms*, not selection order.
- Sub-word loads widened straight to 64-bit (`SBYTES[i] as u64` shapes via
  `i64.load8/16/32_u/_s`) reach the zext/sext smallint arms and warm entire
  never-run wasm-op translation chains (`case_loadwiden.rs`).
- Probe before paying a coverage step (`cargo make fuzza-probe`, see
  AGENT-PROMPT.md) — most wasted steps are shapes LLVM pre-cleaned away.
- To make a "limb is zero" leg of a legalization select (u128 clz/ctz, etc.)
  *dynamically* taken without LLVM folding it, zero the limb with a parity
  multiply — `limb.wrapping_mul((input & 1) as u64)` — which is opaque to
  known-bits, unlike `& mask` shapes (`case_u128_bits.rs`).

## Known bugs live with the tests

Every `#[ignore]`d differential test is deliberate, and the tests are the
single source of truth for known bugs: each one's doc comment and ignore
reason carry the failure, the exact inputs, what passing sibling cases have
*bounded*, and what would allow un-ignoring. Runtime divergences additionally
carry a pinned `<case>_repro` twin (`run_case_with_inputs` with the exact
failing pair). Read the `#[ignore]`d tests in `tests.rs` before writing
cases — some otherwise-reasonable shapes are currently blocked by bugs
documented there — and never re-report one as a new finding. This file
deliberately records no bug specifics, so fixing a bug means cleaning up only
at the test site.

## Operational gotchas

- Agent Bash tools usually start a fresh shell per command — `export
  FUZZA_AREA=...` is lost. Prefix every invocation:
  `FUZZA_AREA='...' cargo make fuzza-cov-step`.
- The report's `Area delta` line inflates by a constant when duplicate
  monomorphized `(file, name)` rows exist — judge productivity by the
  difference of the area *headline* between steps.
- A `fuzza-cov-step` launched immediately after a backgrounded `fuzza-cov` can
  produce an empty report (0 tests, 0 regions) — rerun the step; note the
  `report.prev.json` delta chain is polluted for that step.
- `report.md` can be re-rendered with different `--area` scoping without
  rerunning tests: `python3 tools/fuzza-agent/cov.py
  target/fuzza-coverage/report.json . --prev
  target/fuzza-coverage/report.prev.json --area '<paths>' >
  target/fuzza-coverage/report.md`.
- Ignored cases contribute no coverage on a clean rebaseline (they don't run),
  so an area's headline can *drop* after `fuzza-cov-clean` relative to the
  session that created the ignored case. Expected.
- `MIDENC_EMIT` paths must be ABSOLUTE `kind=DIR` specs: bare kinds dump into
  the test process CWD (that is how stray `.masm`/`.hir` files end up in the
  source tree), and *relative* dirs silently vanish into the ephemeral
  cargo-build workspace. midenc runs in-process on every test invocation
  (cargo caching only affects the Rust→wasm step), so no cache-busting is
  needed to re-probe an unchanged case. `cargo make fuzza-probe` handles all
  of this and writes to `target/fuzza-probe/<case>/`.
