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

## Out-of-scope surfaces (standing decisions — do not target)

- **Linker-stub / `export_name` cases are out of scope** (decision
  2026-07-27): stubs (`#[unsafe(export_name = "ns::path")]` functions with
  `unreachable` bodies) and the routing machinery behind them
  (`frontend/wasm/src/module/linker_stubs.rs`, `frontend/wasm/src/
  intrinsics/`, the miden-ABI transform + signature tables, `emit/felt.rs`)
  are implementation details of the current linking scheme — a future
  linker could drop stubs entirely, and tests must not couple to that.
  Never propose stub-based cases or an `export_name` override; treat this
  entire surface as permanently closed rather than cold.
- **Float bit-transport cases are out of scope** (same decision): f32
  values silently traveling as felt bit patterns is implementation-
  dependent behavior, not a contract — no case may assert it. The durable
  float facts, so the surface is never re-probed (verified 2026-07-23; no
  committed cases): every float COMPUTE operator (f32/f64 arithmetic,
  compares, converts, truncs, promote/demote) fails with the translator's
  clean catch-all diagnostic `Wasm op <Name> is not supported`; `f64` in a
  signature or local fails type conversion (`unsupported type 'f64'`);
  only bit TRANSPORT (f32 consts/reinterprets/params/load/store, mapped
  onto Felt carrying the IEEE bits < 2^32 < p) is silently accepted; and
  felt ARITHMETIC has no float-operator producer, so no IEEE-vs-field
  arithmetic leak is constructible from safe Rust.

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
    operands of one op (`v * v` gets distinct HIR values). BUT midenc-CSE
    merges the repeated `hir.load_local`s of one wasm local within a block
    (no intervening store) into ONE multi-use SSA value (verified
    2026-09-02: the `rotl_window` panic dump's post-loop `acc` is one
    `load_local` with eleven uses), so a user value read several times in
    a block IS a Copy-constrained operand at every use but the last — the
    strongest plain-Rust lever for Copy constraints, beside the shared
    count bands (`case_chain_window.rs`).
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
    cfg-to-scf undef/latch threading are structurally unproducible AT O2.
    Refined 2026-09-02 (`case_loop_keep_oz`, `--optimize=size-min`): the
    small early-`break` scan and `continue` loops -Oz keeps lift to
    `scf.while` ops with exit-dispatch `scf.index_switch` continuations
    whose result columns include UNUSED results, and those drive the full
    interiors of `WhileUnusedResult` (61→168/171) and
    `IndexSwitchRemoveUnusedResults` (18→104/106, incl. `transfer_body` and
    the rewriter-instantiated scf `while`/`index_switch`/`condition`
    builders) — the O2 corpus only ever hit their bail paths.
    `WhileRemoveDuplicatedResults` and `IfRemoveUnusedResults` stay closed
    at -Oz too (no duplicated condition operands, no unused if results).
    `RemoveLoopInvariantArgsFromBeforeBlock` IS producible at O2 (campaign
    14, 2026-09-03): a `continue 'outer` from an inner loop that contains a
    `#[inline(never)]` call lifts to an outer `scf.while` with a
    loop-invariant before-block argument — and the pattern's rewrite
    panics on every match (`AliasingViolationError` at rewriter.rs:335,
    pinned `nest_continue` in tests/compose.rs); with the call inlined
    LLVM restructures the nest and the pattern stays cold.
  - Spill-analysis W at a block/region boundary carries no USER values
    (locals are reloaded per block; Local2Reg promotes only same-block
    store/load pairs with no control flow between them — local2reg.rs). The
    2026-07 corollary that W <= 1 at every boundary was WRONG (struck
    2026-08-27): midenc-CSE'd masked shift/rotate-count bands cross edges
    as SSA and can carry 16+ felts — see "Spill analysis & the edge-split
    cluster". Proactive block-arg spilling remains unproducible (>16-felt
    block params have no producer), but `spill_trailing_until_fits` IS
    producible since 2026-09-02 (campaign 11): cfg-to-scf threads escaping
    values and exit discriminators outward as REGION-OP RESULT columns, and
    a nine-deep loop nest gives an `scf.if` more than 16 felts of results
    (see "Control-flow lifting shapes"); the loop-header `w_used >= K` arm
    and the CFG edge-split machinery are reachable as before.
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
  The same holds for the UNSIGNED non-strict compares: `i64.ge_u` (the
  `gte_u64` emitter arm) has no producer besides a materialized
  `(a >= b) as u32` helper (`case_ucmp_ge.rs`, 2026-08-27) — the u128
  compare legalization only ever materializes an inline `i64.le_u` pair
  (which is what keeps `lte_u64` warm), never `ge_u`.
- Every `panic!` (bounds check, `unwrap`, division guard, `assert!`) lowers
  to a wasm `unreachable` in EVERY case, strict corpus included: the guest is
  built with `-Cpanic=immediate-abort` (`MANDATORY_RUST_FLAGS`,
  midenc-compile/src/pipeline/frontends/rust.rs), so the header's
  `#[panic_handler]` body is never reached on wasm — the harness-built
  `trap_branch` wasm has no handler function at all, just the `unreachable`
  (verified 2026-09-17; the earlier "the `loop {}` handler means `panic!`
  never becomes `unreachable`" fact was wrong). The handler body matters
  only for the native build. An explicit `core::arch::wasm32::unreachable()`
  behind an impossible cross-modulus guard (`case_unreachable_exits.rs`)
  is still the way to plant a trap edge with no panic machinery around it.
  Trap-parity cases (`run_case_traps`,
  `run_case_traps_with_inputs`, module `tests/traps.rs`) are built with a
  header whose panic handler DOES trap on both targets — `unreachable` on
  wasm (what the SDK's handler does; the compiler lowers it to
  `push.0 assert` "entered unreachable code") and `_exit(101)` on the host,
  where the entrypoint runs in a forked child — and the two sides must agree
  per input on value-or-trap. A VM execution error is an outcome there, not a
  failure; the mismatch message reads `native value 7, masm trap (vm: …)`.
- `Operator::CallIndirect` is fully supported (PR #1251 + signature-tag-check
  follow-up, 2026-08): function-pointer and dyn-dispatch cases compile and run
  — see the "Indirect calls / funcref tables" section for the verified facts.
  Recursion (self or mutual) remains a clean "found a cycle in the call graph"
  linker error and is untestable.
- Flat function signatures are capped at **16 stack felts** (16×u32 or 8×u64 is
  the at-limit case, `case_wide_calls.rs`); one felt more currently fails the
  build inside the spill analysis — treat wider signatures as unwritable, not
  as a novel finding.
- **The differential reference is native code, so a guest-TOOLCHAIN
  miscompile shows up as a divergence too.** Before blaming midenc, run
  the dumped WAT through an independent engine (`wasm-tools parse x.wat -o
  x.wasm && wasmtime run -W wide-arithmetic=y --invoke entrypoint x.wasm a
  b`): if wasmtime agrees with the MASM value, the wasm itself is wrong.
  One such class is known (F9; campaign 12 + the campaign-15 blast-radius
  enumeration, 2026-09-03): with `+wide-arithmetic`, rustc
  1.97-nightly/LLVM 22.1.4 can emit the `local.get` of a multi-result op's
  result local BEFORE the op that defines it — RegStackify sinks
  `i64.mul_wide_s`/`mul_wide_u`/`i64.add128`/`i64.sub128` into the SECOND
  operand subtree of a binary instruction whose FIRST operand is the high
  word read through a local, so the read sees a zero-initialised local, the
  register previously colored to that local, or the previous loop
  iteration's value. WAT signature: `local.get N` textually preceding
  `<wide op> ... local.set N` with that value still on the operand stack
  when the `local.set N` executes (a stack-simulating detector over the WAT,
  campaign-15 tooling kept at `scratch/c15-tools/probe.py` on the campaign
  machine, agreed with wasmtime-vs-native on every one of ~260
  probes × up to 16 configs; wasmtime 48 `-W wide-arithmetic=y` always
  returns the MASM value). Classify by that signature. Blast radius (all
  four ops, every opt-level 1/2/3/s/z, LTO irrelevant):
  - Harness-exposed at every `-C debuginfo` level (pinned as ignored
    reproducers): i64 `checked_mul` (non-inlined helper returning `Option`,
    and loops at O2/O3), `saturating_mul` (every form), `checked_pow` (every
    form), `overflowing_mul` whose flag feeds a `break` (`checked_mul_i64` /
    `sat_mul_i64` / `pow_i64`); u128 `saturating_add` / `saturating_sub`
    (every form, `sat_add_u128` / `sat_sub_u128`); the straight-line
    fixed-point idiom `((a as u128 * b as u128) >> 32) as u64`
    (`fixmul_u64`); u128 `checked_add` accumulated in a loop at guest
    opt-level 1 only (`chk_add_u128_o1`, `--optimize=basic`).
  - Masked by `-C debuginfo=2` ONLY (the harness's `debug = 2` guest
    profile; `debuginfo=1` still miscompiles): every "both words used as
    values" shape — `hi ^ lo.rotate_left(k)` of a u64×u64/i64×i64 product
    or of a u128 sum/difference, in straight-line, helper and loop forms,
    with dynamic, constant or x·x operands (`wide_words`, `sext_shapes`),
    bignum add-with-carry chains (`s = a + b + c` as u128, carry = `s >>
    64`), and `hi != lo >> k` loop-break compares on mul_wide_u (all
    levels) / add128 / sub128 (O2, O3 only) (`wide_loop_cmp`). The
    variable-location records pin the op's definitions; a case that passes
    only with DWARF is not proof of correctness — check the standalone
    no-DWARF build before un-ignoring anything in this family.
  - Safe (valid stackification at every level and debuginfo): high word
    only, low word only, `hi != 0` / `hi < 0` / `hi < lo` branches,
    `hi != dyn` with the low word unused, selects, the mum fold `lo ^ hi`,
    `(product) % m` and `>> dynamic` libcall forms, products stored to
    memory or compared as u128, 4-limb multiply-accumulate and schoolbook
    chains, sub-with-borrow chains; u64 checked/overflowing/saturating_mul,
    checked_pow, widening_mul, carrying_mul in every form (`hi != 0` is a
    unary `i64.eqz`); i128 saturating_add/sub (sign-xor test) and every u128/
    i128 checked/overflowing add/sub except the O1 loop above; i128
    checked/overflowing/saturating_mul and i64/u64 wrapping products emit no
    wide op at all. Guards: `mul_hi_only`, `u64_sat_forms`, `sat_i128`,
    `add128_checked`, `ovf_mul`, `wide_mul_edges`, `mulwide_dyn`.
  Keep every shape in the first two groups out of passing guards until the
  toolchain is bumped past the LLVM fix or cargo-miden drops
  `+wide-arithmetic`; the standalone scan of the whole corpus (213 cases, O2
  with and without DWARF) found no other affected case.
- LLVM on wasm keeps constant-divisor division and remainder as
  `div_s/div_u/rem_s/rem_u` with an immediate operand (`isIntDivCheap`):
  only UNSIGNED power-of-two divisors become `shr_u`/`and`; signed `x / 8`
  stays `div_s`, and no multiply-high magic (`mul_wide`) division form
  exists on this target (wat-verified 2026-09-02, `case_div_const_forms.rs`).
  Corollary: i64 `%` by a constant hits the same `checked_mod for i64`
  compile panic as the dynamic form.

## Frontend routing facts (Rust → wasm → HIR → emitter)

- Rust `as` casts become `trunc`/`zext`/`sext`/`bitcast` — **never** HIR
  `cast`. `OpEmitter::cast` and its ~500-region helper cluster are unreachable
  (hir.Cast is created only by canon-ABI glue, felt intrinsics, and local type
  mismatches; the only mismatches wasm typing permits take the bitcast special
  case in the translator).
- `_imm` binary emitter variants are called only from `#[cfg(test)]` code,
  except `eq/lt/gt/lte_imm`, which switch lowering calls with **U32 selectors
  only**. `shr_imm_*` is dead: `arith::Shr` lowering always calls `shr()`;
  constant shift counts are materialized as pushed operands. (`push_u32`/
  `push_i32` in int32.rs are cold corollaries: their only callers are the
  closed `shl/shr/rotl/rotr_imm` interiors.)
- Division/modulo routing (verified 2026-09-02): the MASM lowering maps
  `arith.Div` → `checked_div`, `arith.Mod` → `checked_mod`, `arith.Divmod`
  → `checked_divmod`, and `wasm.I32RemS` → `wrapping_mod` (its ONLY
  producer; `I64RemS` goes through `arith.Mod` into the known i64
  checked_mod panic). No pipeline path emits an unchecked division:
  `unchecked_div`/`unchecked_mod`/`unchecked_divmod` (+ `_imm` and
  smallint-uint twins) are dead API. `arith.Divmod`'s only producer is the
  byte→element address split in `dialects/wasm/src/mem.rs` `prepare_addr`,
  always **U32**-typed (wasm32 byte addresses) — the U64/other
  `checked_divmod` arms and `checked_divmod_u64` are unreachable (user
  `/`+`%` pairs never form a Divmod: div and rem translate separately, and
  LLVM's fusion is mul-sub strength reduction, not a divmod op).
- `(try_)int32_to_uint` / `(try_)int32_to_int` (int32.rs N-bit
  normalization) are compile-time-unreachable from ANY frontend input
  (verified 2026-09-02, full caller audit): callers are the U8/U16 arms of
  binary arith/div (need sub-word-typed HIR arith — no producer, LLVM
  pre-masks), `handle_uint_overflow`'s Checked/Overflowing arms (doubly
  dead: checked/overflowing legalized away), `cast` arms (hir.Cast never
  built), `exp`/`exp_imm`/`pow2` arms (ops never built), `felt_to_uint/
  int` (SDK-only felt surface), and `is_valid_uint`/`is_valid_int` (zero
  callers workspace-wide — dead API). No caller can pass n=32 (arms pass
  8/16, cast arms {16,8,1}, felt guards n<32), so the 2026-08 upstream
  mask-overflow fix point and the `signed/unsigned_reserved_mask` helpers
  are reachable only from unit tests. The 2026-08 upstream
  Dup1→Dup0/mask fixes in these functions therefore fixed
  corpus-unreachable code — do not hunt for their divergences.
- Sub-word premask + checked/saturating/overflowing legalization
  re-verified on the 2026-09 toolchain (subword_sign HIR probe + deleted
  `sat_ovf` wat probe): i8/i16 appear only as pointer pointee types and
  `wasm.sign_extend` src types; saturating u32/i32 add/sub become
  add/sub + compare + select (a saturating u64 add over provably-small
  operands is known-bits-folded away entirely), overflowing add/mul
  become add+carry-compare / mul_wide+hi-word-select, checked_div becomes
  an explicit rhs==0 branch. Zero checked/saturating/overflowing
  constructs survive to wasm.
- Memory-op immediate/typed arms (re-verified 2026-08-27 on the
  element-address-space rewrite of emit/mem.rs): the `load_imm` family has
  only unit-test callers; `store_imm`'s sole producer is the
  GlobalVariable-initializer lowering (lower/component.rs), and the only
  global in a plain no_std module is the element-aligned I32
  `__stack_pointer` — so every non-I32/unaligned `Some(imm)` arm
  (`store_small_imm`, `store/load_double/quad_word_imm`, the felt `_imm`s,
  `store_word_imm`'s unaligned else, `push_native_ptr`) is unreachable.
  Constant-address user stores do NOT reach `store_imm`: no HIR
  constant-address store/load canonicalization exists — the frontend always
  materializes a pointer value through `prepare_addr`. Felt load/store has
  no in-scope producer (f32 bit transport is out of scope by decision — see
  "Out-of-scope surfaces" — and LLVM int-ifies plain from_bits/to_bits
  memory traffic anyway); `repr(packed)` / dynamically-unaligned access
  adds nothing (dynamic-pointer load/store delegates wholesale to
  intrinsics — alignment branching is imm-pointer-only); wasm `memory.copy`
  is always u8-typed, so the byte-`memcpy` runtime element-alignment split
  (memcopy_elements fast path vs fallback loop) is the ONLY reachable
  memcpy fork (both arms warm), and the word-sized `memcopy_words` fast
  paths (pointee size 16 / multiple of 16), the other-size fallback call,
  and `emit_word_aligned_element_addr_from_byte_ptr` (called only from
  those paths) are dead; `realign_double_word`/`realign_quad_word` remain
  zero-caller dead API; `OpEmitter::mem_stream` is dead in this pipeline
  (HIR MemStream is built only by the MASM-frontend lifter);
  `store_array`/`store_struct` are todo!() stubs with no producer (wasm
  stores are scalar-only). `prepare_addr`/`enforce_alignment` are WARM via
  the frontend `FunctionBuilderExt` monomorph (their cold remainder is the
  assert message + `?` error edges); their two fully-cold report rows are
  the `FunctionBuilder<OpBuilder>` instantiation (used only by the
  aligned_memory.rs unit tests) plus a `<_, _>` phantom row — do not
  re-read them as a coverage regression.
- Wasm has no 128-bit memory ops: `[u128; N]` array (runtime-indexed,
  loads AND stores) and u128-static traffic all legalize to `i64.load`/
  `i64.store` PAIRS (wat+masm probe-verified 2026-07-23, deleted `u128_arr`
  probe — the masm shows only `load_dw`/`store_dw` execs). I128-typed HIR
  loads/stores arise only from compiler-internal spill/sret slots, always
  with dynamic pointers (the warm `load/store_quad_word` None arms →
  `load_qw`/`store_qw` execs). The imm quad-word arms are therefore closed:
  their only callers are `load_imm` (unit tests) and `store_imm`
  (GlobalVariable initializers; sole global = I32 `__stack_pointer`), so
  `load_quad_word_imm`/`store_quad_word_imm` are unreachable and
  `realign_double_word`/`realign_quad_word` are zero-caller dead API.
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
- Two config/consumer-gated global cold clusters (2026-07-23, source-audited
  — not case-producible): the advice-taint analysis monomorphs
  (hir-analysis sparse/dense over `AdviceTaint*`) run only under
  `session.options.lint`, which the harness never sets; and postdominance
  (`SemiNCA<true>`, incl. `find_roots`) has no computing consumer in the
  wasm pipeline (CSE only marks `PostDominanceInfo` preserved).
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
- Dead-code translation (`translate_unreachable_operator`) only ever sees
  structural `end`s: LLVM deletes unreachable MBBs before CFGStackify, so no
  `block`/`loop`/`if`/`else` operator and no plain operator is ever emitted in
  dead state (those arms + the catch-all are unproducible; the End-of-Loop arm
  IS warm — loops whose only exits are mid-loop returns/traps keep an
  unconditional latch `br`, wat-verified). LLVM's end-of-function fixup gives
  such dead-fallthrough loops/blocks a `(result ..)` type, but those frames
  are never branch targets and `br` never carries values (locals argument) —
  so a dead `end` never resumes at a following block WITH arguments (the
  next_block_args closure is unproducible).

## Module-structure payload closure (verified 2026-08-27, global mop-up)

The cold remainder of `module_env.rs::parse_payload` and its section handlers
is toolchain-gated for cargo-miden no_std cdylib builds — not case-producible:

- `import_section`/`declare_import` (0-cov): harness modules are import-less;
  an undefined import is a clean link error, and intrinsic/stub imports are
  the out-of-scope linker-stub surface.
- `start_section` (0-cov): rustc/wasm-ld never emit a wasm start section for
  a no_std cdylib (no life-before-main in Rust).
- `dwarf_section`: WARM since the campaign-7 DWARF flip (974f0757e) — every
  differential guest now builds with full debug info (see the debug-info
  cluster section).
- `TagSection` is `unreachable!()` (exceptions feature disabled).
- Partials: `global_section` (the I32 `__stack_pointer` is the only wasm
  global this toolchain emits), `data_section` (no passive-segment producer
  without shared-memory init), `element_section`/`table_section` (multi-table
  / passive / null-hole / PIC-base shapes — see the indirect-calls section),
  `name_section` (subsections beyond function names are not emitted).
  The remaining error arms (Encoding::Component, duplicate custom sections)
  are diagnostics backstops.

## Local2Reg & data-segment layout (verified 2026-08-27, memory gap-check)

The pass lives at `dialects/hir/src/transforms/local2reg.rs` (NOT
hir-transform/) — scope FUZZA_AREA accordingly.

- **Every function parameter gets an unconditional `hir.store_local` at
  entry** (frontend/wasm func_translator.rs `declare_parameters`); wasm
  local.get/set/tee are the only other load/store_local producers.
  Consequences (`case_local_shapes.rs`): an UNUSED parameter of a kept
  function (`#[no_mangle]` defeats dead-arg elimination, `#[inline(never)]`
  keeps the call) is a stored-but-never-loaded local and reaches the pass's
  dead-store-erasure arm; a zero-param/zero-local helper reaches the
  no-locals early return; a by-value aggregate param (passed indirectly)
  gives a promotable single-use pointer local.
- **Harness guests carry FULL DWARF since the campaign-7 flip** (974f0757e:
  `debug = 2` in the generated release profile + the package retention key),
  so `di.debug_declare` and location schedules are pipeline-live — but
  rustc/LLVM emit every wasm-local VALUE location as the two-op
  `[DW_OP_WASM_local(N), DW_OP_stack_value]` (probe-verified across the
  `dbg_*` corpus; a bare one-op `[WasmLocal]` memory location never
  appears — even by-value aggregate pointer params get either no named
  DWARF entry at all, falling back to `argN`, or the two-op form,
  `case_dbg_byval`). Consequences: `declares_are_safe`'s exact
  `[WasmLocal(idx)]` match never succeeds, so the declare-conversion loop
  of `convert_debug_references_for_local` is unproducible from rustc DWARF
  (unit-test-only), and an unsafe declare referencing a promotable or
  dead-store local takes the return-false path — Local2Reg then PRESERVES
  the stores (DWARF-on builds promote fewer locals; codegen-only,
  differentially semantics-neutral). The frontend still synthesizes plain
  `[DW_OP_WASM_local(N)]` `di.debug_value` records at local.set/tee and for
  params, which keeps the safe-values rewrite loop warm. Quantified in
  campaign 24 (see "The release configuration" below): the candidate set is
  the same at every debug level and only the conversion check differs — 4
  slots promoted with full DWARF vs 27 without, over the same seven guests.
- Other closed Local2Reg arms: ExecFpi prefix-local pinning (SDK-only
  producer); the loaded-but-never-stored "poison" arm (no safe-Rust
  producer of a read-before-any-write wasm local — LLVM materializes
  constants for known-zero and deletes unreachable-path merges); the
  neither-loaded-nor-stored else (structurally dead — candidates come from
  the load/store maps); `is_declaration` (import-less harness modules have
  no function declarations); log bodies.
- **Data-segment layout arms are toolchain-bounded**: wasm-ld emits active
  segments sorted by offset, unique, non-overlapping, so
  `DataSegmentLayout::insert` always takes the push_back path
  (middle-insert / same-offset-dedup / Mismatch / Overlapping arms
  unproducible) and `validate_no_overlaps`' error interior is a backstop
  behind it. The end-of-address-space edges (insert's OutOfBounds,
  `next_available_offset`'s overflow Nones) need a segment ending at/past
  2^32 — covered by the linker.rs unit test
  `link_fails_when_data_segments_fill_the_address_space`, and inherently a
  link error, not differential material. `DataSegmentLayout::len`/
  `pop_front`/`Segment::alloc_default` are dead API (no pipeline callers).

## Memory & data-layout ladders (verified 2026-09-02, campaign 13)

Bug-directed sweep of the element-addressed memory lowering (`prepare_addr`
+ `emit/mem.rs` + the `intrinsics/mem.masm` cross-element procs + data
segments + frames) with plain-Rust layout ladders, every kept case native-
grid-checked (1225 boundary pairs) and 256-pair swept (`tests/memory.rs`,
campaign-13 cases):

- **No silent load/store miscompile was found.** Agreeing with native:
  byte lanes at all four offsets (volatile stores keep them as
  `i32.store8`; whole-word reads of a byte buffer become element-space
  `i32.load`s), halfwords at byte offsets 0..3 (3 = element-straddling
  `load_u16`/`store_u16`), `#[repr(C, packed)]` u16/u32/u64/i16 fields at
  every offset (21-byte records in a runtime-indexed array cycle each
  field through 0..3), `packed(2)` u32/u64/u128 fields at 2 mod 4
  (`align=1` memargs -> the byte-space `mod 2` assert), u64 at 4 mod 8
  (`i64.load/store align=4`: `load_dw`/`store_dw` at odd element
  addresses), u128 at 4/8/12 (i64 halves straddling Miden words), i64 at
  odd byte offsets (three-element `realign_dw`), `i64.store8/16/32` +
  `i64.load8/16/32_u/_s` at offsets 0..3 (`trunc_int64` into the narrow
  stores — the corpus had never emitted these before campaign 13), enum
  layouts (`Option<u8/u16/u32>`, `Result<u32, u8>`, u8/u32/u64-payload
  enums), fat pointers in `.rodata` and in the frame, odd-size by-value
  aggregates and sret returns ([u8; 7], [u16; 5], 13-byte packed, [u8;
  13], (u8, u32, u16)), `[bool; N]` lanes, runtime-length copies/fills of
  0..33 bytes at src/dst offsets 0..3 and 200..2092-byte copies through
  both memcpy arms, u16/u64/i8 element copies and fills, a 96 KiB
  `.rodata` beside a 40 KiB `.bss` and a funcref table (globals land at
  the wasm memory end, 0x130000, table on the next page), and frames of
  2.5 KiB / 68000 B / 1,000,000 B whose addresses escape to helpers.
- **The only memory finding is a MASM-only trap on identical-range
  copies**: a `copy_within` whose runtime destination equals its source
  is blocked when the ranges are 4-aligned (see the ignored
  `copy_same_pos` in `tests/memory.rs`; the byte-loop arm is fine,
  `copy_same_bytes`). Keep runtime shifts non-zero in passing cases.
- **memset has no element fast path**: `OpEmitter::memset` is a per-byte
  load/mask/or/store loop (~25 cycles per byte), so a `[0u8; N]`/`[0u32;
  N]` local costs ~25·N cycles per execution (a 68000-byte zero-init is
  ~1.7M cycles, ~17 s in the step-mode executor). memcpy's element fast
  path needs `src%4 == dst%4 == count%4 == 0`. For large-frame cases use
  `[MaybeUninit<u32>; N]` (no fill; `assume_init` only on written slots),
  which is how `frame_64k`/`frame_1m` stay cheap. A director-level
  improvement: splat the byte to a u32 and store whole elements when
  `dst%4 == count%4 == 0`.
- **Frames up to ~1 MB pass** (`frame_1m`, SP down to ~0x0B000): the
  shadow stack is 1 MiB (`--stack-first`, data at 0x100000), so larger
  frames are UB natively too — not differential material. The recursive-
  frame rung is unwritable (recursion is a linker error).
- **WAT probe reading**: the text format prints `align=N` in BYTES (2, 4,
  8; absent = natural alignment), not the log2 memarg value.
- `&STATIC[a..b]` in a static initializer is a rustc E0658 (const `Index`
  is not stable) — build interior slices of statics at runtime.

## Call boundaries & pairwise compositions (verified 2026-09-02, campaign 14)

Bug-directed sweep of internal call boundaries and of pairwise compositions
of the campaign-10..13 boundary guards (`tests/calls.rs` campaign-14 cases,
`tests/compose.rs`), every kept case native-grid-checked and 256-pair swept:

- **Signature felt counting**: a hidden return-area pointer (sret) is an
  ordinary i32 parameter and counts against the 16-felt flat-signature
  cap (7 u64 + u32 + u128 result = 16 felts compiles, `case_call_sigs16`);
  u128 parameters scalarize to two i64 (4 felts); mixed 14 u32 + u64 and
  7 u64 + 2 u32 are at-limit shapes that pass with four u64 live across
  every call.
- **Wide by-value results agree with native** in every layout tried
  (`case_ret_area`): `repr(C)` records with a word-aligned u128 field,
  `repr(C, packed)` records with the u128 at byte offset 1 (unaligned i64
  store pairs into the return area), 13-byte arrays, `(u64, u64)`,
  `Option<u128>` / `Result<u64, u32>`, a u128 rebuilt by a helper on every
  loop trip, and sret forwarding (a helper passing its own return-area
  pointer to its callee).
- **Callee-side pressure is independent of caller-side pressure**: a
  callee spilling a 20-felt tree and a 16-felt-signature callee using
  every parameter twice, called under six live caller u64s, pass
  (`case_callee_pressure`); `&mut` array / slice fat-pointer parameters
  (15-felt pointer+scalar signatures, runtime-bounded sub-slices, in-place
  `swap`) pass (`case_mut_arrays`); calls inside zero-trip-capable loops
  with carried u64s, in single `match` arms and deciding exits pass
  (`case_loop_calls`).
- **Composition boundaries** (each guard is the largest passing rung; the
  rung above hits only a KNOWN class): count bands x sixteen-state machine
  — 6 shared counts pass, 8 = F2 arity-2 gap (`case_chain_sm`); bands x
  five-exit zero-trip nest with an in-loop `match` — 6 pass, 7 = F2, 8 =
  F6 over-full stack (`case_bands_exits`); 24-felt tree in the innermost
  body of a six-level nest with three escape depths passes
  (`case_tree_nest`); twenty pinned felts across misaligned runtime-length
  copies/fills pass (`case_spills_copies`); nine rotating u32 + three u64
  carried across two pinned calls per trip pass (`case_calls_carried`);
  six selects feeding two `br_table`s in a loop pass
  (`case_selects_switch`); C12 value ladders deciding five exits pass
  (`case_ladder_exits`); packed 35-byte records feeding mul_wide / 128-bit
  libcall shifts / i128 checked_div / sext chains and written back
  unaligned pass (`case_lanes_wide`); a four-state machine mixing a
  16-felt sret call, a packed frame store, a fn-pointer dispatch and a
  runtime `copy_within` passes once its arithmetic lives in helpers
  (`case_calls_all`). No runtime divergence in any composition.
- **The F9 guest-LLVM class DOES extend to `i64.add128`/`sub128`** (campaign
  15 refuted the campaign-14 reading): u128/i128 checked/overflowing add and
  sub in helpers and a loop agree with native at the default level
  (`case_add128_checked`, 256 pairs), but u128 `saturating_add`/
  `saturating_sub` miscompile in every form, u128 `checked_add` loops
  miscompile at guest opt-level 1, and both-limb value uses of a u128 sum/
  difference are DWARF-masked miscompiles (see the F9 entry under "Toolchain
  & pipeline facts").
- **Second sweep (2026-09-03, all 256-pair swept, no runtime divergence)**:
  wide results carried OUT of loop nests by the exit dispatch (u128 /
  `(u64, u64)` / `Option<u128>` / `Result` helper results deciding and
  carrying five exits, `case_sret_exits`); fn pointers RETURNING u128 /
  tuples / `Option<u128>` (return-area pointer as the first
  `exec_indirect` argument, `case_sret_dispatch`); packed records returned
  by value into runtime-indexed array elements (computed unaligned sret
  address, `case_sret_indexed`); u8/u16/u32 record fields returned by
  value (i64.store8/16/32 into the sret area, `case_narrow_ret`); narrow
  signed/unsigned/bool parameters and results across calls and a
  narrow-typed fn pointer (`case_narrow_sigs`); three u128 carried across
  a call per trip (`case_carried_wide`); a 2 KiB frame escaping as a
  runtime `&mut [u32]` sub-slice through fn-pointer dispatch
  (`case_frame_dispatch`); side-effect order of `&&`/`||` lattices over
  direct and dispatched calls (`case_shortcircuit_calls`); six count bands
  across a direct call, a 6-u32 helper and a plain-locals dispatch
  (`case_bands_calls`); recursion through the table with a 5-u64
  signature (`case_recursion_wide`) and with per-frame `&mut [u64; 6]`
  arrays escaping into the callee frame (`case_recursion_frames`); a
  24-arm `br_table` whose arms call every arity shape (`case_switch_calls`);
  calls at every level of a five-level nest deciding each level's exit
  (`case_nest_calls`). Only the two panic classes above (`indirect_spill`
  family, `nest_continue`) were found.
- **`for`/`while` nests with a labeled `continue` of an outer level from
  an inner loop that contains a call do not compile** (the
  `nest_continue` panic above; the same nest with a same-level `continue`,
  labeled `break`s or an inlined helper compiles). Keep compositions to
  same-level `continue`s when the inner loop calls anything.
- **Native `usize` is 64-bit, wasm `usize` is 32-bit**: `(x as usize) % N`
  on a u64 `x` is a FALSE divergence (the wasm side truncates first) —
  index with `(x % N) as usize` (re-learned by `recursion_frames`; the
  C13 `rodata_big` gotcha above is the same trap).

## Indirect calls / funcref tables (verified 2026-08-27)

Corpus cases: `case_call_indirect`, `case_indirect_sigs`,
`case_indirect_collision`, `case_dyn_trait`, `case_fnptr_value`,
`case_indirect_chain`, `case_indirect_wide`; all probe- and/or
region-verified.

- **Pipeline**: wasm funcref table → `builtin.function_table` (two words of
  linear memory per slot: MAST-root digest word + signature-tag word) →
  linker allocates the table word-aligned in the page after the globals;
  component `init` fills initialized slots via `procref`; each
  `call_indirect` becomes `hir.exec_indirect` → bounds check + signature-tag
  check + `dynexec`. Tables are lowered lazily on the first dispatching
  `call_indirect`. The runtime failure modes (OOB index, null slot,
  tag-mismatched slot) are UB natively and are asserted NON-differentially in
  `end_to_end/indirect_call_traps.rs` — differential cases must stay on safe
  dispatches and never duplicate them.
- **Tag interning**: tag = structurally-interned wasm signature index + 1
  (`signature_type_tag`; 0 reserved for null slots). Structurally-equal fn
  types share one tag; distinct fn-ptr types in one program produce distinct
  tags inside the ONE shared table (`case_indirect_sigs`, entries tag 1 + 2).
  A multi-tag table is also what reaches the tag-mismatch skip arms of
  `ExecIndirect::verify` and `possible_callees`.
- **Toolchain shape**: rustc + wasm-ld emit exactly one funcref table
  (`__indirect_function_table`), slot 0 = reserved null pointer, all live
  address-taken functions contiguous from slot 1, initialized by a single
  active element segment at offset 1 with no `ref.null` holes. Hence
  `collect_table_image`'s FuncRef-whole-table-initializer, `precomputed`
  Null-image, global-relative(PIC)-base, and null-hole arms, plus every
  multi-table shape, are toolchain-unproducible.
- **Devirtualization** (the enemy): a provably-single-target fn ptr or a
  constant table index is devirtualized to a direct call. What survives as
  `call_indirect`: runtime-indexed loads from a `static` fn-ptr array
  (`OPS[(x & 3) as usize]`), runtime-indexed `[&dyn Trait; N]` selection, and
  fn-ptr values crossing `#[inline(never)]` boundaries (returned from or
  passed to noinline helpers, incl. loop-carried fn-ptr state machines) —
  LLVM does not do indirect-call promotion without PGO.
- **dyn Trait**: vtables are `.rodata` arrays of funcref-table indices; each
  method dispatch loads its vtable slot and `call_indirect`s with the
  method's own wasm signature (receiver pointer + args → its own tag)
  (`case_dyn_trait`, 3 dispatch sites, tags 1/2).
- **Non-capturing closures** coerced to `fn` become anonymous
  `FnOnce::call_once` shim entries in the table; **fn-ptr `==`** compiles to
  `i32.eq` on table indices, agreeing with native address comparison for
  distinct-bodied functions (`case_fnptr_value`; wasm-ld does no ICF, which
  also closes `possible_callees`' duplicate-callee dedup arm).
- **Table symbol collisions**: the generated table symbol is
  `__indirect_function_table_<idx>`, probed against the module symbol table
  with a counter bump — a user `#[no_mangle]` fn named exactly that forces
  the rename path (`case_indirect_collision`, table becomes `..._0_1`).
- **Width cap**: the lowering schedules the arguments plus the table index in
  Miden's 16-felt operand-stack window ⇒ at most 15 argument felts. 7×u64
  (14 felts) dispatches end-to-end (`case_indirect_wide`); one felt more is a
  clean translation-time diagnostic (code_translator/mod.rs `unsupported
  call_indirect ... operand stack window`), not a panic.
- **Recursion THROUGH a fn-pointer table compiles and runs** (campaign 14,
  2026-09-02, `case_recursion_indirect.rs` + `_edges`): the linker's
  "found a cycle in the call graph" check sees only direct `exec` edges, so
  a helper that loads its callee from a runtime-indexed `static [fn; N]`
  and calls it (`dynexec`) may recurse — bounded depth `input1 % 6` with
  non-tail per-frame state matches native. Direct and mutual recursion
  through plain calls remain the clean linker diagnostic (re-verified with
  a deleted probe). This is the only plain-Rust recursion lever.
- **Wide indirect dispatch is spill-analysis-blind** (campaign 14,
  trace-verified 2026-09-03): the spill analysis reads an op's inputs from
  operand group 0 only, and `hir.exec_indirect` keeps its ARGUMENTS in
  group 1 (group 0 = the table index), so whenever the argument setup of a
  dispatch needs more than 16 felts the analysis spills some ARGUMENTS,
  never reloads them ("required by reloads = 0", "freed by op = 1") and
  budgets the call as one felt, while the emitter keeps the spilled values
  physically (spills are `store_local` copies; the dispatch use is real).
  The over-full physical stack then aborts at the first deep access — the
  site varies: `NoSolution` at lowering.rs:109 for an arity-2 op in the
  same block or for the dispatch itself, `invalid operand stack index`
  (emit/mod.rs:623, arity-1 Copy) or `invalid stack offset for movup`
  (emit/mod.rs:758). `hir.exec` is unaffected (arguments in group 0);
  `dyn Trait` method dispatch (vtable `call_indirect`) is affected the
  same way. Plain-Rust producers: (1) 7-u64 dispatch in a loop with >= 7
  loop-invariant u64 locals as arguments (the DWARF-kept wasm locals are
  reloaded per block, so the seven `load_local`s + the carried accumulator
  + index math overflow; N <= 6 passes: `case_dispatch_pressure.rs`);
  (2) loop-free 7-u64 dispatch with two single-use values stackified
  UNDER it (one still fits); (3) loop-free dispatch with arguments computed
  IN PLACE from extra locals (`v3.rotate_left(c)` as an argument); (4) fn
  pointers taking three u128 parameters (twelve limb felts) in a loop, or
  two u128 with one argument computed in place (`case_indirect_u128.rs`
  = the passing two-plain-locals guard). Pinned: `indirect_spill` (loop),
  `indirect_spill_line` (loop-free), `indirect_spill_args` (emitter
  signature), each with a passing direct-call twin (`direct_loop`,
  `direct_line`, `direct_args`) in `tests/calls.rs`. Straight-line
  dispatches whose arguments are plain locals loaded right before the
  call pass with twelve such locals (`case_indirect_args.rs`). Rule for
  compositions: a fn-pointer / `dyn` dispatch under pressure must take
  PLAIN LOCALS only, with nothing single-use stackified across it.
- **Verified dead ends**: `add_table_entry`'s Intrinsic arm
  (`CallableFunction::Intrinsic` is unconstructible today — the only
  `register_linker_stub` caller pre-filters on `is_operation()`) and
  Instruction arm (an intrinsic in a table = linker-stub surface, out of
  scope); `live_entries`' empty-entries arm (a lazily-built table always
  holds ≥1 entry — safe Rust cannot dispatch without an address-taken
  function); `exec_indirect`'s argument-extension assert (indirect
  signatures come from `sig_from_func_type`/`AbiParam::new`, which never set
  extension attrs — extension attrs exist only on `Signature::new` canon-ABI
  component paths); the legalization illegal arms for
  FunctionTable/FunctionTableEntry/ExecIndirect (invalid-IR backstops; the
  frontend pre-diagnoses the producible ones at translation).
- `OpEmitter::assert`/`assert_eq` have only felt-intrinsic (SDK) HIR
  producers (`frontend/wasm/src/intrinsics/felt.rs`); `assert_eq_imm` has
  only `#[cfg(test)]` callers; `assertz`'s harness producer is only the
  `prepare_addr` align-hint — their cold arms are closed for plain-Rust
  cases.

## Rewrite-pass scope closures (CSE / SCCP / DCE / folder / scf patterns)

Verified 2026-07-23 (region-level coverage + source audit of the pass
pipeline; the pass list now lives in midenc-compile/src/pipeline/backend.rs,
where it is built unconditionally at every optimization level — it used to be
in midenc-compile/src/stages/rewrite.rs):

- The rewrite pipeline is Canonicalizer → CSE → SCCP → SinkOperandDefs →
  Local2Reg → TransformSpills → LiftControlFlowToSCF → Canonicalizer →
  SinkOperandDefs → TransformSpills, all on a FUNCTION pass manager. CSE and
  SCCP therefore run only on pre-lift cf-form function bodies — which contain
  **zero region-bearing ops** — and never on the module body (a graph region).
- CSE consequences: the entire non-SSA-dominance universe
  (`replace_uses_and_delete`'s visited-set arm, `has_visited_owner_or_
  ancestor`, the `ScopedCseCandidates` linear fallbacks) and BOTH
  nested-region branches of `simplify_block` are unreachable. Extra "CSE
  food" (e.g. duplicate unsigned-op bitcasts) only re-runs the already-warm
  SSA merge arm. The span-propagation tail is dead too: the only unknown-span
  ops are folder-materialized constants, already unique per (value, ty) when
  CSE runs; and CSE never sees trivially-dead ops (the canonicalizer's driver
  erases them immediately beforehand).
- SCCP cannot out-prove LLVM + the canonicalizer on wasm-derived IR: its only
  extra powers need dead CFG edges (constant branch conditions — already
  folded) or an all-preds-same-constant block arg (an identical-incoming phi,
  which LLVM InstSimplify folds to the value itself). Its warm
  `replace_with_constant` activity is just re-uniquing each function's
  existing `arith.constant`s — one per (value, ty), because the
  canonicalizer's folder dedups constants function-wide first. Hence
  `OperationFolder::try_get_or_create_constant`'s cache-hit and cross-dialect
  arms are unreachable (`arith.constant` is the only ConstantLike in cf-form
  wasm-derived HIR; `ub.poison` exists only post-lift, after SCCP).
- `OperationFolder::try_fold` / `process_fold_results` / `clear` have NO
  callers workspace-wide (dead API): the greedy driver folds internally and
  calls only `insert_known_constant`; SCCP calls only
  `get_or_create_constant`. `insert_known_constant`'s folder-owned-rehoist
  arms are dead (fresh folder per driver iteration, each op visited once),
  and `notify_removal`'s main body is dead (nothing erases a folder-owned
  constant while its folder lives).
- Greedy-driver region simplification runs at `RegionSimplificationLevel::
  Normal` everywhere (the driver default; the one pipeline config-setter,
  midenc-compile backend.rs, also sets Normal). `merge_identical_blocks` and
  `drop_redundant_arguments`/`drop_redundant_block_arguments` run only under
  `Aggressive` — config-gated, no case producer (2026-08-27).
- The MASM legalization pass (codegen/masm/src/legalization.rs) runs
  `apply_full_conversion` on every compile, but wasm-derived HIR arrives
  already-legal, so `FullConversionDriver::legalize_operation` only verifies
  legality: its pattern-rewrite/materialization interiors and
  `reconcile_unrealized_conversion_casts` are invalid-IR backstops
  (2026-08-27).
- `DeadCodeAnalysis` has exactly two pipeline loaders — SCCP's solver
  (pre-lift) and `LivenessAnalysis` inside TransformSpills (pre- AND
  post-lift; the latter is what warms the scf region-branch/terminator arms)
  — both FUNCTION-scoped. Hence: `walk_symbol_tables` finds no symbol tables
  under a `builtin.function` (the `initialize_callable_symbols` closure never
  runs), every resolved callee is "external" (outside the analysis scope), and
  known-callsite/callable-terminator resolution (incl. the
  `join_with_inputs::<ValueRange>` monomorphization and
  `mark_entry_blocks_live`) never executes. Module-scoped SCCP/liveness would
  be required, and the pipeline never schedules it.
- `WhileRemoveDuplicatedResults` DOES fire (REFUTED 2026-09-09, campaign 19;
  the 2026-07-23 argument below said its interior was unreachable from
  wasm-derived IR). It keys on duplicated **scf.condition forwarded
  operands** (the before-region terminator) — not on yields and not on
  results directly — and cfg-to-scf's exit-dispatch chain does synthesize a
  duplicate pair in a three-level TRIANGLE nest (`control_flow::triangle`,
  one rewrite at the default level). Removing either of that case's exits
  (the labeled `continue`, the inner `break`) still fires it, so the nest
  itself is the producer; a four-level triangle and every -Oz variant fire
  zero. The original argument — conditions forward either nothing
  (locals-only loops) or distinct `index_switch` results, and the repeated
  operand lists cfg-to-scf synthesizes (`scf.yield %v, %v, %v, %v, %d`) live
  only in switch/yield ARM terminators, which the pattern ignores — holds for
  everything except that shape.

## cf/scf canonicalization & cfg-to-scf closures (verified 2026-08-27)

Region-level audit of everything still cold under
`dialects/cf/,dialects/scf/,hir-transform/src/cfg_to_scf` (control-flow
gap-check pass; wat probes `tail_funnel` (deleted), `spin_guard`):

- **Returns are always per-site.** The LLVM wasm backend emits an explicit
  (tail-duplicated) `return` at every return site and never branches to the
  outermost frame; probe- and corpus-wat-verified, it also never emits
  result-typed `block`/`loop` frames — no `br`/`br_if` in our pipeline ever
  carries a value on the wasm stack (div-bearing two/three-arm tail merges
  and early-return+loop shapes all come out as per-site `return`s).
  Value-carrying HIR successor args therefore exist only in cfg-to-scf's own
  synthesized dispatch (e.g. the residual `cf.cond_br .. ^ret(%v)` exit).
- Consequences, all structurally closed for wasm-derived IR: the function's
  ret-only exit block (`^exit(%v): builtin.ret %v`, built by the final
  reachable `End`) always has exactly ONE unconditional-br predecessor, and
  `SimplifyBrToBlockWithSinglePred` (registered before `SimplifyBrToReturn`
  on cf.br, equal MAX benefit) always claims it — `SimplifyBrToReturn`'s
  interior, `collapse_branch`'s block-arg check/remap paths (a passthrough
  block with arguments needs back-to-back result-typed frame ends), and the
  branch-region entry-argument replacement in
  `transform_to_structured_cf_branches` (transform.rs ~725) are all
  unproducible.
- **`SimplifyPassthroughCondBr` DOES rewrite** (REFUTED 2026-09-09,
  campaign 18; the 2026-08-27 closure below was an argument about the IR as
  the frontend emits it, and missed that the greedy driver runs
  `SplitCriticalEdges` in the same fixpoint). Original argument: collapsing
  an arm of a multi-successor predecessor requires the passthrough's target
  to have a UNIQUE predecessor (critical-edge guard in `collapse_branch`),
  but a wasm frame is emitted only because something branches to it — a
  target whose only pred is the passthrough block would be a frame nothing
  branches to. What actually happens: `SplitCriticalEdges` inserts fresh
  single-predecessor blocks, and the guard becomes satisfiable on the
  rewritten CFG, so the two patterns alternate to a fixpoint. Producer: an
  inner loop whose only exits are several in-loop `return`s plus a `break`,
  nested in an outer loop — twelve `SplitCriticalEdges` rewrites interleaved
  with ten `SimplifyPassthroughCondBr` rewrites (`deadfall_oz`). Ladder
  2026-09-09 (campaign 19, `canon::passthru_frame`): the pattern fires if and
  only if at least one in-loop `return` comes AFTER the `break` — with the
  `break` last it never fires, at any number of return sites — and it fires at
  the DEFAULT opt-level exactly as at -Oz (the -Oz attribution was an artifact
  of only tracing -Oz cases). The count is ten on every firing rung regardless
  of the number of return sites (2..5) or the break's position, so it is a
  property of the two-level frame; six return sites stops it entirely (LLVM
  restructures the body).
  The plain-br variant (`SimplifyPassthroughBr`) skips the guard for
  1-successor preds and DOES fire (REFUTED 2026-09-09, campaign 19): a
  corpus-wide trace found it in `do_while` (four rewrites) and `cf_shapes`
  (one). The producer inside `do_while` is its `carried_bool` helper alone — a
  `while go` loop whose condition is a loop-carried bool updated in several
  match arms; `do_while_cont` alone fires zero, `nested_do_while` alone fires
  zero at O2 and one at -Oz. Everywhere else `SimplifyBrToBlockWithSinglePred`
  (same MAX benefit, registered first) claims the shapes.
  A frame-end passthrough to a *self-loop* is producible: a bare
  `loop {}` behind an impossible guard leaves a header block containing only
  its own back-edge `cf.br`, taking `collapse_branch`'s
  collapse-into-self-loop bail (`case_spin_guard.rs`; `unreachable_exits`
  deliberately keeps its infinite loop body non-empty, which hides this
  shape).
- **`CanonicalizeI64RotateBy32ToSwap` (dialects/arith) is structurally
  unreachable from wasm-derived IR** (closed by source + probe,
  2026-09-09): the pattern resolves its shift operand to a constant through
  `hir.cast`/`hir.bitcast`/`arith.trunc`/`arith.sext`/`arith.zext` only,
  but the frontend wraps EVERY rotate/shift count in
  `arith.band(trunc(count), width - 1)` (`mask_movement_count`,
  frontend/wasm/src/code_translator/mod.rs) and `arith.Band` has no `fold`
  impl (the arith dialect defines folders only for constants and
  coercions), so the walk bails on the band and never sees the 32. Probed
  111 attempts on a rotate-heavy -Oz case and 24 on a case that does
  nothing but `rotate_left(32)`/`rotate_right(32)`/`(x << 32) | (x >> 32)`
  on u64 — zero rewrites at every opt-level. This is the same `arith.band`
  that makes count bands the corpus's cross-block spill freight.
- **`SimplifyCondBrLikeSwitch` DOES fire** (REFUTED 2026-09-09, campaign 19;
  the campaign-18 "no producer" claim came from probing switch shapes with no
  trapping arm). It needs a `cf.switch` with exactly two successors, and TRAP
  or impossible-guard arms are what get a switch that low: six producers among
  the committed control-flow cases (`unreachable_exits` twice;
  `switch_loop_mix`, `switch_trap_arm`, `trap_branch`, `spin_guard`,
  `ret_args` once each), and it is deliberately reproducible with a sixteen-arm
  `match` carrying impossible `panic!()` arms or a wasm `unreachable` arm
  (`canon::trap_dispatch`, one rewrite at every opt-level). A plain many-armed
  `match` with duplicate arms still does not reach it — the 64-arm `sm16`
  fires `SimplifySwitchFallbackOverlap` fourteen times and this pattern zero.
- **`IfRemoveUnusedResults` DOES fire** (REFUTED 2026-09-09, campaign 20; the
  2026-09-09 campaign-19 closure below held only for freight-free shapes).
  Producer: a three-level diamond nest crossed by CSE-merged count bands whose
  only consumer is the deepest arm — three rewrites at both opt levels with 4, 8
  or 12 bands, ZERO with the same nest and no bands (`interact::sink_spill`), so
  the band traffic is what leaves an `scf.if` result with no real use. Original
  argument, still correct for the shapes it was derived from, and
  **`WhileConditionTruth` still has no producer** (2026-09-09): cfg-to-scf
  builds a payload column only for a
  value that HAS a use outside the region, so an `scf.if` result with no
  real uses cannot arise from partly-used join columns nor from a diamond
  inside a multi-continuation kept loop; and `WhileConditionTruth` needs
  `scf.condition` to forward its own condition value into the after region
  AND the corresponding block argument to be used, which LLVM forecloses by
  folding any in-body read of the loop condition to `true` before lifting.
- **The `WhileUnusedResult` -> `IndexSwitchRemoveUnusedResults` column-removal
  cascade has exactly one producer shape: a loop whose only in-body branch is
  an EMPTY `continue` arm** (2026-09-09, campaign 19, bisect of
  `loop_keep_oz`). cfg-to-scf gives such a loop an exit-dispatch payload
  column with no consumer; `WhileUnusedResult` drops the loop result, its
  yield operand dies and `IndexSwitchRemoveUnusedResults` rebuilds the
  `scf.index_switch` without that column. The cascade is LINEAR in the number
  of such loops (K copies -> K rewrites of each pattern, verified to K = 8 in
  `canon::col_cascade`) and fires at the DEFAULT opt-level as well as at -Oz.
  Emptiness is load-bearing: a `continue` arm that updates any carried
  variable fires neither pattern (verified over K x C = {1,2,4} x {1,2,3}
  loops x continue-edges), and neither does an early-`break` scan loop or a
  nested counted-loop pair.
- **`SimplifySwitchFallbackOverlap` rewrites ONCE per switch, however many
  arms it merges**: the pattern rebuilds the `cf.switch` without ALL of the
  overlapping cases in a single rewrite, so its fire count measures the number
  of distinct switches, not merged arms (`sm16`'s fourteen fires are fourteen
  switches). Arm count (8/16/32), duplicate count (3/5/9) and duplicate
  placement (contiguous/scattered/tail) leave the count at one
  (`canon::arms_merge`).
- **Corpus-wide firing table** (2026-09-09, campaign 19: the whole
  `control_flow` module traced in one run): `FoldRedundantYields`
  (`sm16`/`wide_exits`, sixteen each), `SimplifySwitchFallbackOverlap`
  (`sm16`, fourteen), `ConvertTrivialIfToSelect`
  (44 producers, max sixteen in `diamond_nest`), `SplitCriticalEdges` (44
  producers, max forty in `wide_exits`), `WhileRemoveUnusedArgs` (33
  producers, max six in `nest8`; a chain of K loops with an early `break`
  gives exactly K rewrites at -Oz, and K at the DEFAULT level too once the
  loop bodies carry count-band traffic that stops LLVM unrolling them —
  campaign 20, `interact::scan_spill`). `SimplifyBrToReturn` fires ZERO times
  corpus-wide — `SimplifyBrToBlockWithSinglePred` (same MAX benefit,
  registered first) claims its shapes.
- **`RemoveUnusedSinglePredBlockArgs` DOES fire**, but only in trap/br_table
  shapes (`switch_loop_mix`, `switch_trap_arm`, once each; 2026-09-09,
  campaign 19 — the campaign-18 "0 fires" claim was based on ~12 cases, none
  of them from the control-flow module). Four purpose-built scaled-up
  variants failed to reproduce it, so it stays rare. Source note: the
  pattern's loop reads `br_op.successors()[0]` for BOTH the then- and the
  else-destination (dialects/cf/src/canonicalization/simplify_successor_
  arguments.rs), so the else successor's arguments are never removed.
- **All cfg-to-scf payload columns are 1-felt `u32`** (2026-09-09, HIR dumps
  of `nest8`, `wide_exits`, `exit_values`, `sm_wide`, `loop_keep_oz` and
  campaign-19 shapes, with guest DWARF on AND off): every `scf.yield` /
  `scf.condition` / `scf.index_switch` / `scf.if` signature carries only
  `u32` discriminators and `ub.poison` placeholders, plus the `i32` function
  result and `i1` conditions. User state crosses region boundaries in wasm
  LOCALS (`hir.load_local` inside the consuming region), so multi-felt
  (u64 = 2-felt, u128 = 4-felt) columns are NOT producible from plain Rust —
  column-remap and if-to-select rungs that need wide columns are unproducible
  at every opt-level.
- **Cheapest pattern-firing evidence**:
  `MIDENC_TRACE='pattern-rewrite-driver=trace'` prints
  `trying to match '<pattern>'` (debug) immediately before each attempt and
  `pattern matched successfully` (trace) after a successful rewrite, so
  pairing each success with the preceding attempt line gives an exact
  fired/not-fired table per case. Useful sibling target for the emitter:
  `codegen:operand-scheduling=trace` ("there are N used operands out of M",
  "dropping dead instruction result %v at index i", "dropping N operands").
- **`cf.Switch`/`cf.CondBr::get_successor_for_operands` interiors are
  closed**: the only callers workspace-wide are DCA's
  `visit_branch_operation` (dce.rs) and the spill analysis'
  single-successor resolution (spills.rs), both passing SCCP-lattice
  constants — a cf selector/condition is never a lattice constant (SCCP
  cannot out-prove LLVM pre-lift; the post-lift residual dispatch selects on
  scf results, which are runtime).
- **`cf.Select::fold` interior is closed**: it folds only on a constant
  BoolAttr condition; wasm `select` conditions are LLVM-pre-folded and
  `ConvertTrivialIfToSelect`-created discriminator selects have runtime
  compare conditions; nothing post-lift constant-ifies an i1 (SCCP is
  pre-lift only, and no cf constant-condition pattern exists).
- **`FoldConstantIndexSwitch` is closed**: an `scf.index_switch` selector is
  either a user `br_table` selector (LLVM deletes constant-selector
  br_tables) or a cfg-to-scf discriminator (multiplexer block-arg/op-result
  by construction). `FoldRedundantYields` (the only use-replacer that could
  constant-ify one) needs ALL regions to yield the same SSA value in the
  selector column, but discriminator columns carry distinct per-continuation
  constants by construction — an all-same column would mean a single
  continuation, for which no dispatch switch is synthesized. Note
  `builtin.ret_imm` has NO pipeline producer (frontend always emits
  `builtin.ret`; ret_imm appears only in unit tests and global-variable
  initializers), so exit kinds are exactly {ret, unreachable}, the combined
  exit dispatch is at most one `cf.cond_br`, and a 3+-way residual exit
  switch cannot exist.
- **cfg-to-scf transform cold interiors are logs/errors or recorded
  closures**: `combine_exit` and `EdgeMultiplexer::redirect_edge` are fully
  warm except `log::trace!` bodies and `?` error edges; `check_value`'s
  nested-region grandparent walk needs an SSA value crossing sibling loops
  (locals argument); the undef-threading arm and `loop_block_dominates`
  cache need an escaping value not defined in the latch (latch-multiplexer
  construction, recorded); the latch→header carried-values loop
  (transform.rs ~891) and prior latch/header-arg checks need loop-header
  block args (irreducible/multi-entry CFG, unproducible from wasm); the
  reduce-time successor-swap arm is dead by the
  `create_single_exiting_latch` invariant (recorded). The
  `<_ as CFGToSCFInterface>` builder rows are duplicate unresolved-receiver
  monomorphs of warm concrete impls. `LiftControlFlowToSCF`'s
  World/Component/Module recursion arms never run (FUNCTION pass manager).
- The remaining cold cf/scf rows are OpParser/OpPrinter impls (textual HIR),
  SwitchCase KeyedSuccessor rewrite-API, rewriter-instantiated scf builder
  monomorphs of the closed canonicalization interiors, and the
  `get_region_invocation_bounds`/`get_entry_successor_regions`/
  `get_successor_regions` region-analysis arms (liveness/DCA-adjacent —
  candidates for a spill-focused area, not for CF cases).

## Control-flow lifting shapes (verified 2026-09-02, campaign 11)

Bug-directed sweep of cfg-to-scf / scf canonicalization / cf lowering with
input-routed plain-Rust shapes (state machines, multi-exit loops, carried
sets, deep nests, many-block functions, switch forms, short-circuit
lattices, jump-threading sources), every kept case with a native-verified
pinned exit grid (`tests/control_flow.rs`, campaign 11 cases):

- **No runtime divergence was found in any lifted shape** — 18 shape
  families x 16 random pairs at O2, x48 pairs at -Oz and O3, plus pinned
  grids covering every exit site, 0-/1-/n-trip of every zero-trip-capable
  loop, and the signed/unsigned compare boundaries of `Ordering` dispatch.
  Exit-dispatch selectors, poison-threaded discriminators (`sm_bits` lifts
  to 2 `scf.while`, 7 `index_switch`, 5 `scf.if`, 23 `cf.select`, 42
  `ub.poison`) and loop-carried locals all agree with native.
- **LLVM jump-threads a bit-driven `loop { match state {..} }` machine into
  NESTED loops at O2** (wat-verified, `case_sm_bits.rs`: 8 states with two
  `continue` arms become two `loop` frames, one `br_table`). State machines
  therefore exercise nested-loop exit dispatch, not a single flat switch.
- **Region-op result columns grow with nesting depth, ~2 per level**:
  every value that escapes a loop level and every level's exit
  discriminator becomes a result of the enclosing `scf.while`/`scf.if`
  (the widest `scf.if` of the eight-deep `case_nest8.rs` has fifteen u32
  results). This is the only plain-Rust producer of >16-felt region
  results — the `nest8` / `deep_nest_overflow` pair pins the depth
  boundary (8 pass / 9 fail at O2; 6 / 7 at -Oz, where LLVM keeps every
  level as a loop: wat-verified 8 `loop` frames at -Oz vs 6 at O2 for the
  same eight-level source, whose `% 3 + 1` levels O2 peels). Exit
  MULTIPLICITY does not drive it: ten escape sites at depth five
  (`case_wide_exits.rs`) stay within budget.
- A `match` on a u64 against 64-bit constants is NOT a compare chain:
  LLVM emits a `br_table` on a wrapped half plus `i64.eq`/`i64.ne`
  compares (`case_sm_wide.rs`, wat-verified), so u64 selectors still reach
  `translate_br_table`.
- **`br_table` width**: the wasm frontend builds one `cf.switch` successor
  group per target; a 255-target table builds (`case_switch255.rs`), 256
  does not (`switch256`, u8 group index) — sizing fact for dense-match
  cases, the bug itself lives with the test.
- Scale is otherwise free: ~800 blocks (200-arm match + depth-8 tree +
  128-step guarded chain, `case_blocks_max.rs`), a 16-state x 4-way
  machine with 64 arms (`case_sm16.rs`), sixteen loop-carried variables
  with a full rotation per trip all compile and pass at O2 — block count
  and arm count are not limits below the switch-width cap.
- Short-circuit lattices with side-effecting `#[inline(never)]` probes
  (`case_shortcircuit.rs`) keep their evaluation ORDER end-to-end: an
  atomic counter of probe weights, reset with `swap(0)` before returning,
  is a cheap way to assert which operands ran.

## Block-emitter operand-drop facts (codegen/masm emitter.rs / emit/mod.rs / stack.rs)

Verified 2026-07-23 (three probed cases — fresh-valued multi-exit loops,
nested labeled blocks, three-level nested-loop exits — all +0 area; HIR dumps
corroborate each closure below):

- **The post-op drop DOES fire** (refined 2026-09-02; the 2026-07-23
  "never fires" closure predates DWARF and the count-band lever).
  `drop_unused_operands_at`'s per-op call site (emit_inline closure#1)
  finds dead operands when a stack value's last use sits inside the
  regions of a region-bearing op: CSE-merged masked count bands live
  across an `scf.while` die inside its body, so the drop fires at the op
  right after the while (`case_band_guard_oz` at -Oz: one post-op site,
  nine dead bands under two live operands, "2 used operands out of 11" →
  the pathological branch's used/unused INTERLEAVE arms — movdn+dropn,
  swap+drop, movup+drop — went 25→105/177 at -Oz; the DWARF-on default
  corpus already sits at 76/177 through its unused-batch/whole-stack
  arms). Block-entry drops (closure#0) remain the common path.
- **Block-entry drops always see uniform liveness.** Inherited stacks at
  emit_inline entries hold at most one cfg-to-scf payload column (+ the
  selector, which emit_switch_region/emit_linear_search drop out-of-band via
  `drop_operand_at_position` at index 0). Dispatch arms forward ALL their
  level's columns or NONE — the levels chain, they never stack — so entries
  are all-live (no drop) or all-dead (whole-stack batch, warm). The
  solver path and the used/unused interleave arms of the pathological branch
  are unreachable AT BLOCK ENTRY; both are reachable at the post-op site
  (previous bullet: dead count bands under live operands after an
  `scf.while`).
- **scf.while `after` regions are always trivial latches**: `^block(args…):
  scf.yield` with every arg dead and dropped at index 0 — the swap/movup arms
  of `drop_operand_at_position` have no producer (dead scf results are
  canonicalized away, selectors sit on top, argc>=2 rets are impossible).
- **All three arms of the post-op drop now have committed producers**
  (verified 2026-09-09, campaign 18, `codegen:operand-scheduling=trace`).
  Which arm `drop_unused_operands_at` takes is decided purely by
  `unused.len()` vs `num_used` at that program point, and with count bands
  both counts are dialable: N bands used before a loop and on its
  loop-carried accumulator die inside the loop, M bands also used after it
  stay live. `N > M` gives the pathological manual interleave
  (`band_guard_oz`, "2 used operands out of 11"); `M == 0` gives the
  all-unused batch arm, whose `assert_eq!(batch_size, self.stack.len())`
  then holds (`drop_batch_oz`, "0 used operands out of 11" → one `dropn`);
  and `N < M` gives the SOLVER arm — `schedule_operands` with an all-`Move`
  constraint list followed by `dropn` (`drop_solver_oz`, "6 used operands
  out of 9"), which nothing in the corpus reached before. A wrong schedule
  in any of them is a silent miscompile, so these are value-checked
  differentially, not just compiled.
- **The dead-instruction-result drop only ever fires at index 0**, and its
  single plain-Rust producer is the unused LOW half of `i64.mul_wide_u/_s`
  — a Rust `mulhi`, `((a as u128) * (b as u128)) >> 64` (`mulhi_dead_oz`,
  four two-felt drops). The mirror shape (dead HIGH half) has no producer:
  whenever the high half is dead LLVM emits the narrow `i64.mul` instead of
  the wide op, and a u128 accumulator loop that reads only the low half
  afterwards keeps both halves live as loop-carried values. Hence the
  `swap+drop` / `movup+drop` arms of `drop_operand_at_position` are
  unreachable *from the dead-result site* (they are still reached from the
  block-entry unused-parameter loop).
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
- **Dead emit-helper API** (zero callers workspace-wide, 2026-08-27):
  `dup_select_int32`/`mov_select_int32` (int32.rs); `zext_int64` and
  `move_int64_up` (int64.rs) are called only from the dead cast/felt/i128
  paths. `LoopForest::verify`/`compare_loops`/`verify_loop` (hir ir/loops.rs)
  are self-check API with no pipeline caller.
- **`OperandStack::get` is SDK-only** (emit/events.rs, emit/merkle.rs);
  `IndexMut` remains closed with the same-value-operand-pair fact. Refinement:
  cfg-to-scf DOES synthesize repeated-operand lists (`scf.yield %v, %v, %v,
  %v, %d` observed), but yields are no-op lowerings — the "same-SSA-value
  operand pairs unreachable" fact still holds for solver-scheduled ops.

## Operand-scheduler solver & scale facts (2026-07-23 scale iteration)

Region-verified during the scale-stress iteration (codegen/masm/src/opt/,
linker.rs, cfg_to_scf); the corpus's scale cases are `case_chain300` (~400-op
single-block chain), `case_match64`, `case_deep_nest`, `case_call_web`,
`case_seg24`.

- **The solver has no fallback scheduler.** Production fuel is always the
  default 40, charged once per tactic tried (cost 1 for the four pattern
  tactics, `max(num_copies,1)` for CopyAll/Linear/LinearStackWindow; chains
  are ≤5 tactics). Exhausted fuel only stops the search for a *better*
  solution — with no solution yet, the remaining tactics run regardless
  (regression-test-pinned). Reaching the exhaustion break needs ≥14
  Copy-constrained operands in one problem (still no known producer). The
  2026-07-23 claim that all-tactics-fail (`NoSolution` → compile panic) is
  closed from wasm-derived IR was WRONG (struck 2026-08-27): LLVM
  runtime-unrolls a `% 97`-bounded `acc = acc.wrapping_mul(33) ^ i` loop 8x
  into a single block interleaving eight mul/xor rounds with eight distinct
  `i+k` operands, and scheduling that block panics with `NoSolution` on safe
  Rust (`unroll_chain`, kept `#[ignore]`d; specifics at the test). The
  mul-only and xor-only bodies of the same loop collapse when unrolled and
  pass, so the trigger is the unroll-produced *interleaved* chain — plain
  chain length is fine (`case_chain300`). ROOT-CAUSED 2026-08-27: the
  unroll-family panics (`unroll_chain`, `unroll_rotmix`) are NOT solver
  limitations — the unroll forces spilling, and TransformSpills hands the
  solver SSA-invalid IR (see the spill section's phi-insertion fact); the
  scheduling problems are *unsatisfiable* (an expected operand absent from
  the operand stack), not hard. Panic site depends only on arity: arity-2 →
  `TwoArgs` NotApplicable → `NoSolution` at lowering.rs:109; arity≥3 →
  `MoveDownAndSwap` walks the model past its end → subtract-with-overflow
  in `Stack::movdn` (stack.rs:80). The solver never validates that expected
  Move operands exist on the stack, so out-of-contract input surfaces as
  these arbitrary panics.
- **Arity-2 problems are TwoArgs-only** (`solver.rs` `is_binary` branch):
  no other tactic is pushed for binary ops, so when TwoArgs' fixed
  dup/movup pattern needs a stack access past the 16-felt MASM window (a
  Copy-constrained operand near the bottom of a full 15-felt window — copy
  materialization adds transient depth the K=16 spill cap does not model),
  the window check rejects the solution and there is no fallback →
  `NoSolution` on an *in-contract, solvable* problem
  (`LinearStackWindow`+`Linear` produce a valid in-window schedule for the
  same shape at other arities). Reproducer: `rotl_window` (ten shared
  count bands + u64 rotl; the six-count `spill_switch` passes) — a
  root-cause distinct from the unroll-family panics above. The class needs
  no loop: a single-block chain of N shared counts on a multi-use u64
  (LLVM hoists the rotates ahead of the xor/add chain, so their results
  are the freight) reproduces it at N = 20 and passes at N <= 18
  (`case_chain_window.rs`, campaign 10); `[Copy, Copy]` (counts reused a
  third time) fails from N = 16, `[Copy, Move]` and `[Move, Move]` pass at
  every N tried (the failing TwoArgs patterns are the ones that `dup` the
  Copy operand BEFORE moving/duping the deeper one).
- **Arity-1 and arity >= 3 problems cannot hit the window gap on an
  in-contract (<= 16-felt) stack** (campaign 10, source + ladder-verified):
  arity-1 never enters the solver (`solve_and_apply` emits one dup/movup;
  a bottom u64 of a 16-felt stack is `dup.15 dup.15`, `case_unary_window.rs`
  passes with thirteen counts + accumulator above the operand), and
  `LinearStackWindow` materializes copies deepest-first, which keeps every
  later copy source within the window (the already-copied sources lie
  between it and the bottom), then moves only within the top. A
  `NoSolution` at arity 1/>= 3 therefore implies an OVER-FULL stack (> 16
  felts, see the spill section) or SSA-invalid IR (F1), never a solver
  limitation.
- **No size-gated compiler path exists at single-block scale**: a ~400-op
  non-reassociable chain (139 spill locals, 267 stack-motion ops in MASM)
  compiles in about a second and passes differentially — no cliff, no
  fuel/scale arm. Scale DID warm `TwoArgs::move_copy`'s commutative
  sub-arms (non-strict scheduling of commutative binops under reload
  interleavings) — the only tactic interior that responded to scale.
- **MoveDownAndSwap's FIRST evict arm and MoveUpAndSwap's final
  NotApplicable arm remain unproducible**: they need a live non-operand
  value on top of the stack at an arity≥3 no-copy problem, but RegStackify
  moves every single-use def to its use and SinkOperandDefs sinks the whole
  operand cluster together, so operands stay adjacent to their op; the
  400-op storm never produced the shape. Refinement (2026-08-27):
  MoveDownAndSwap's SECOND evict arm (the post-move eviction) IS warm in
  the current corpus, so the old "evict arms unproducible" plural was too
  strong. CopyAll's success loop and SwapAndMoveUp's real arms still have
  no plain-Rust producer — the four tactics' *precondition* arms are
  structurally dead (each tactic is only pushed when its precondition
  already holds). The unroll-interleave lever (2026-08-27) DOES reopen the
  solver interiors, but every u64 trigger found so far panics before
  contributing coverage (NoSolution at lowering.rs:109 — also producible
  WITHOUT unrolling by an arity-2 rotl with a copy-constrained shared
  count band under ~10 felts of crossing-band freight — and a second
  unroll-family panic in `Stack::movdn`; both live as ignored reproducers
  in the spills test module). The schedulable u32 twin (`case_unroll_u32`)
  adds no new interior regions. `preemptively_move_endangered_operands_to_
  top`'s interior is closed-in-practice: it needs missing-copy felts plus
  a deep move operand in one problem, but exec args are always fresh
  single-use loads (no aliases) and alias-bearing small ops have
  SinkOperandDefs-adjacent operands.
- **Switch lowering is width-insensitive past 8 arms**: a 64-arm dense
  `match` survives as one 65-target `br_table` (structurally-varied arm
  bodies defeat LLVM's lookup-table and arm-merging transforms), and adds
  zero new compile-side regions over the 8-arm case. Likewise cfg-to-scf is
  depth-insensitive (12-level nesting = +0) and the codegen linker is
  count-insensitive (24 statics = +0; wasm-ld merges statics into few
  segments regardless).
- **cfg-to-scf reduce-loop interiors are closed**: every value escaping a
  loop is a latch-multiplexer block argument *by construction* (all exit
  traffic is multiplexed through the single latch, so `check_value`'s
  defined-outside-latch guard never passes → the escape undef-threading and
  its dominance cache are unproducible). In-loop value joins never carry
  block args — even a div-heavy two-arm `let t = if c {..} else {..}` join
  travels through a wasm local (wat-verified; the only br-with-value LLVM
  emits is the function-result tail merge). Loop-header-args arms need an
  irreducible CFG (wasm is reducible by construction). The latch's
  successor 0 is always the loop header (create_single_exiting_latch
  construction invariant), so the reduce-time successor-swap arm is dead.
- **codegen/masm/src/linker.rs is data-layout only** (segments + globals +
  function-table bases — call-graph/MAST ordering lives in the assembler,
  not here) and closed: its cold surface is error paths, disabled log
  bodies, the multi-module `__stack_pointer` dedup (the harness always links
  exactly one HIR module), the page_size=0 arm, and dead accessors —
  including `FunctionTableLayout::is_empty` (sole caller
  `has_function_tables` sits behind `requires_init`'s `has_globals()`
  short-circuit, and `__stack_pointer` makes has_globals always true) and
  `element_addr_of`'s None edge (2026-08-27; the table layout loop itself is
  warm from the call_indirect cases).

## Spill analysis & the edge-split cluster (verified 2026-08-27)

Corpus cases: `case_spill_split` (asymmetric diamond, both split flavors),
`case_spill_loop_mix` (loop-header over-capacity + backedge splits),
`case_spill_switch` (dispatch under crossing freight), plus the revived
`case_spill_edge`. All trace-verified with `MIDENC_TRACE=
'analysis:spills=trace,pass:spills=trace'` — the spill pass/analysis logs
(edge splits, W^entry sets, loop pressure) are the cheapest way to check a
spill shape BEFORE paying a coverage step.

- **The only plain-Rust producer of cross-block W traffic is the
  masked-count band**: the translator wraps every shift/rotate count in
  `arith.band(count, mask)`; the canonicalizer's folder dedups the constant
  operands function-wide and CSE merges the structurally-identical bands
  into the dominating occurrence — so a count CONSTANT reused in two blocks
  becomes ONE u32 SSA value (one felt) live across the edges between them.
  User values never cross in W (locals are reloaded per block; Local2Reg is
  same-block-only). N shared counts = N felts of freight across any chosen
  edge; CSE needs the first use to dominate the later ones (e.g. a do-while
  body dominates the post-loop code, a `while` body does not dominate its
  exit).
- **Edge splits (`SpillAnalysis::split`, the transform's split loop,
  `Placement::Split`) fire on ASYMMETRIC pressure**: a value in W^entry(B)
  that is missing from one predecessor's W^exit gets a reload split on that
  edge, and the compensating spill lands as a split on the other edge —
  produced by a diamond whose arms differ in pressure while shared bands
  cross both (`case_spill_split`); symmetric-pressure shapes (spill_branch/
  twin/edge) spill the value in BOTH arms and never trigger reconciliation
  (that is why the cluster stayed cold until now). Loop preheader and
  BACKEDGE splits come from over-capacity loop headers the same way.
  CAVEAT (2026-09-02, `pass:spills=trace` over spill_split /
  spill_loop_mix / spill_switch): split-edge reloads are materialized and
  then ERASED by the SSA reconstruction ("erase unused reload" for every
  split-block reload — the walk uses a dominator tree cached before the
  transform's own splits, so split blocks are never visited); the spilled
  values stay live on the operand stack past their spills and the passing
  cases pass only because the unrelieved pressure still fits the window.
  Never design a case whose window fit depends on split-edge relief
  (specifics at `zero_trip_frontier`/`zero_trip_overflow` in pressure.rs).
  Campaign-14 refinements (trace-verified, 2026-09-02): a BACKEDGE split
  of a bottom-test loop loses its reloads the same way (an over-capacity
  header with eight live-through u64 locals and three bands shared
  before/after the loop: "edges to split = 1", then "erase unused reload"
  for exactly those bands); and the `frontier.rs:123` unwrap needs only
  ONE crossing band plus a join with three or more predecessors reached
  through a split edge — a four-arm `match` inside a zero-trip-capable
  loop with a single u32 count shared before and after the loop is
  enough. Count bands are not only rotates: EVERY constant shift count is
  one — the `<< 32` of a u64 assembly and the `>> 32` of the final fold
  form a band that crosses everything in between, and LLVM synthesizes
  constant u32 shifts on its own (`x * 7` -> `x << 3`, `x * 19` -> shifts
  by 4 and 1, `(a >> 4) & 3` address math), so a composition that must
  stay free of crossing bands should keep its arithmetic in
  `#[inline(never)]` helpers and leave the composing function with calls,
  locals and `& mask` operations only (`case_calls_all.rs`).
- **Zero-trip-capable loops are the plain-Rust loop-bypass lever**: a
  `while i < input2 % 97` bound keeps LLVM's loop guard and a bypass edge
  around the loop, whereas the corpus's `% 97 + 3` bounds become
  bottom-test loops with no bypass (wat/HIR-verified 2026-09-02; the
  lifted form wraps the while in an `scf.if`). With spilled bands crossing
  such a loop the shape reaches the split-edge defects above from about
  eleven counts (two loops) / twelve counts (one loop) / six counts with an
  in-loop `match`; below those, zero-trip and one-trip inputs execute
  correctly (pinned twin `zero_trip_guard_repro`). Bottom-test loops with
  an opaque `#[inline(never)]` trip count are unaffected (the campaign-10
  `helper_bound` rung passed on 0/1-trip inputs).
- **`scf.while` result columns fit the loop-header budget**: twelve
  in-loop counts plus two live-through counts across a bottom-test while
  that carries a dispatch discriminator + payload column (in-loop
  `return`) compile and pass at every (in-loop, live-through) split from
  (2, 12) to (12, 2) (`case_while_results.rs`); the "results are not in
  the header budget" overflow hypothesis is closed for bottom-test loops.
- **Loop-header `w_used >= K`** (the over-capacity arm incl. its sort and
  take_while closures) is reachable with 16+ shared counts used both before
  the loop and on the loop-carried accumulator inside it (LICM cannot hoist
  rotates of a loop-carried value; rotates of loop-invariant operands DO
  get hoisted and defeat the shape).
- **Pre-lift spilling bounds the post-lift pass**: the first TransformSpills
  caps SSA values crossing any CFG edge at <= K felts and rewrites spilled
  values' downstream uses to reloads placed at those uses, so after
  cfg-to-scf no scf op can have >16 felts of results and no post-lift
  block boundary exceeds K. Consequently `spill_trailing_until_fits`, the
  w_exit>K result-spill arm of `compute_w_exit_region_branch_op`, the
  region-branch entry-spill arm of `visit_region_branch_operation` (min()
  also caps W right before every op, and scf operands — if conditions,
  switch selectors — are always freshly computed there), and the loop-LIKE
  over-capacity closures are all unproducible.
- **Terminator operands are always fresh**: yield/condition/ret operands
  are constants, local loads, or tail-computed values, never
  spilled-and-unreloaded — MIN's terminator-reload interiors are closed.
  Splits carrying successor ARGUMENTS are likewise unproducible (arg
  sources are computed immediately before the terminator).
- **Pre-lift "live through loop" is always empty** (three shapes): the
  loop-exit +LOOP_EXIT_DISTANCE increment never survives into the header's
  next-use set, so post-loop-used values arrive classified as in-loop
  candidates; the pre-lift live-through sort closure is out of reach (the
  post-lift loop-LIKE counterpart does fire).
- **`max_block_pressure`'s region-branch arm is empirically unproducible**:
  the loop-pressure walk only visits the scf.while's own region-graph
  entries, and top-test, light-header, and bottom-test diamond-in-loop
  variants never place the nested scf.if in a walked block.
- **`get_region_invocation_bounds` (and the entry-successor arms it feeds)
  is pass-config-gated**: its sole caller is ControlFlowSink
  (hir-transform/src/sink.rs), which is registered but never scheduled in
  the pipeline. (This refutes the 2026-08 CF-iteration lead that
  liveness/DCA under TransformSpills reach it.)
- **Test-only API**: `is_spilled_at`/`is_reloaded_at`/`is_spilled_in_split`/
  `is_reloaded_in_split`/`set_materialized_split`/`get_split` are called
  only from the analysis' own unit tests.
- The spill freight has a scheduler ceiling: crossing-band freight around
  10 felts combined with an in-loop multi-arm dispatch currently fails to
  schedule (see the ignored reproducers in the spills test module); keep
  deliberate freight around 6-8 felts in cases that must pass.
- **`insert_required_phis` seeds every predecessor edge with the spilled
  value itself** (hir-transform/src/spill.rs, phi-insertion for DF+ of the
  reload blocks): for a join reachable via a path the definition does not
  dominate (e.g. a loop-bypass edge when the spill lives in the loop body),
  no reaching definition exists on that edge, so the seeded successor
  argument is never rewritten and the function leaves the pass SSA-invalid;
  the phi is provably dead on such edges (a real use would have been
  invalid pre-pass), the pass itself warns "unused phi ... encountered
  during rewrite phase" (removal is an open TODO in `rewrite_inserted_phi_
  uses`), and nothing downstream verifies dominance (the per-op verifier
  has no SSA-dominance check). cfg-to-scf then threads the dead phi args
  into sibling-region scf.yield operands, and codegen panics scheduling an
  operand that is not on the operand stack — the mechanism behind the
  ignored unroll-family reproducers (specifics at the test sites). Because
  the poisoned phi can never feed a live use, this defect cannot silently
  miscompile; it always surfaces as a compile-time panic. Two source facts
  bound the phi machinery (2026-09-02): `rewrite_cfg_spills` rebuilds SSA
  form from the `DominanceInfo` the spill ANALYSIS computed and cached
  before the transform split any edge, and `DominanceFrontier::new`
  populates frontiers only for blocks with THREE or more predecessors
  (`enumerate().any(|(i, _)| i > 1)`) — so phi insertion never happens at
  two-predecessor joins, and the unroll-family shapes (epilogue joins with
  3+ predecessors) are the ones that reach it.

## Spill freight x canonicalization: pass interactions (verified 2026-09-09, campaign 20)

Composing the canonicalization producers of campaign 19 with the spill freight
of campaign 18, every rung value-checked at the default level and at
`--optimize=size-min` (corpus cases in `tests/interact.rs`).

- **Count bands alone rarely request a spill; a u64 CLUSTER does.** Up to about
  nine shared masked rotate counts the spills trace logs zero spills (the
  analysis counts LIVE felts). The lever that makes the analysis actually spill
  is the `case_spill_split` recipe generalized: H u64 values defined before the
  region, consumed inside it in ONE wide expression (so they are all live at a
  single program point) and used again after it. Eight bands plus an eight-value
  cluster across two loops gives 57 spills / 84 reloads / two "edges to split" /
  six split edges. The two axes are not independent: with four bands the cluster
  caps at seven values, with eight bands an eight-value cluster still compiles.
- **`TransformSpills` leaves no spill/reload ops behind.** Each reload is
  rewritten into a `hir.load_local` of a spill slot — the pass trace logs
  "convert reload to load" once per reload — so post-lift passes and print-IR
  dumps show spill freight as `load_local`, never as `hir.reload`. Searching a
  post-lift IR dump for reload ops will always find zero.
- **Freight does not change how often a canonicalization pattern fires.** The
  column-removal cascade stays linear in the number of empty-`continue` loops
  (1/2/4 loops -> 1/2/4 of each pattern) for every band count from 2 to 12;
  `SimplifyPassthroughCondBr` still fires exactly ten times (with 13 / 12
  `SplitCriticalEdges` at O2 / -Oz) with twelve bands crossing both loops;
  `SimplifySwitchFallbackOverlap` still fires once per switch. What freight
  changes is the operand-scheduling outcome, not the rewrite.
- **Two exceptions, both new producers:** (a) `IfRemoveUnusedResults`, recorded
  as having NO producer, fires three times on a three-level diamond nest whose
  deepest arm is the only consumer of the crossing bands — bisected against the
  same nest with zero bands, which fires it zero times, so the BAND TRAFFIC is
  the producer; (b) a chain of K early-`break` scan loops fires
  `WhileRemoveUnusedArgs` K times at the DEFAULT level once its bodies carry
  band traffic (campaign 19 measured K at -Oz and zero at O2, because LLVM
  unrolls freight-free scans).
- **The band-count boundary is not monotone.** The empty-`continue` cascade
  shape compiles at 2, 4, 6, 8, 9, 12, 13, 14 bands and panics at 10, 11, 15,
  16, identically at O2 and -Oz and identically for one, two and four loops.
  Do not infer "N-1 passes" from "N fails" on this axis.
- **Freight tolerance ranking of the canonicalization shapes** (bands crossing,
  at both opt levels unless noted): a chain of early-`break` scan loops and a
  three-level diamond take 12+; the fallback-overlap dispatch takes 10 (12 at
  O2 only); the cond-br-like-switch dispatch takes 8; the asymmetric if-to-select
  diamond takes 6 (8 at O2 only) and its symmetric twin two rungs fewer; the
  passthrough frame takes 16 at O2 but only 12 at -Oz. On the cluster axis the
  passthrough frame is the weakest (four values break it) and the scan chain the
  strongest (ten). The PREDICATE of a scan loop's `break` moves that number: an
  accumulator-derived condition tolerates ten cluster values where an
  input-bit-derived one fails at six.
- **The `frontier.rs:123` unwrap does not need a zero-trip-capable loop.** It
  needs a join with three or more predecessors reached through one of the spill
  transform's own split edges; a sixteen-arm `match` inside a bottom-tested loop
  and a pair of sequential loops whose bands are used only in the second both
  reach it with `(input % k) + 2` bounds. Both are DEFAULT-level-only failures
  that compile at `--optimize=size-min` — the opposite direction from the
  documented -Oz-earlier rule.
- **Erased split-edge reloads and dead "unused phi" block arguments are present
  in programs that compute the right answer.** Four of the six committed
  interaction guards carry erased split reloads (2 to 32 of them) and two carry
  "unused phi" warnings, and every one of them agrees with the native build on
  its pinned grid. Neither marker on its own predicts a miscompile; they predict
  how close the shape is to the pressure cliff.
- **Print-IR between passes:** `-Z print-ir-after-pass=<pass>` is an unstable
  option whose printer emits through `log::trace!` with target `pass:<pass>`
  (hir/src/pass.rs), so it needs BOTH the flag and
  `MIDENC_TRACE='pass:<pass>=trace'`; `print-ir-after-all` and
  `print-ir-after-pass` are mutually exclusive. Pass names: `canonicalizer`,
  `cse`, `sparse-conditional-constant-propagation`, `sink-operand-defs`,
  `local2reg`, `transform-spills`, `lift-control-flow`. Both canonicalizer runs
  and both spill runs share one name, so the dumps are told apart by order.

## Compiler-configuration axes (verified 2026-09-02, campaign 8)

- **midenc has no middle-end optimization knob.** The HIR pass pipeline in
  `midenc-compile/src/pipeline/backend.rs` is a fixed list; per-pass CLI
  flags are dead (the registration loop is commented out), `--pass-pipeline`
  exists only in the standalone `hir-opt` tool, `ControlFlowSink` and
  `DeadCodeElimination` are commented out of the schedule on purpose
  (`24b3c936b`), and `RegionSimplificationLevel::Aggressive` is never
  constructed (its `merge_identical_blocks` is a stub). Reaching those needs a
  compiler-source change, not a harness flag — do not re-explore.
- **`--optimize=<level>` only sets the guest's LLVM opt-level** (via
  `--config profile.release.opt-level=N` on the nested cargo build). Cargo
  config profiles override manifest profiles, so the harness's manifest
  `opt-level = 3` is inert: the default corpus runs at **opt-level 2**,
  `--optimize=max` gives 3, `--optimize=size-min` gives z. Verified by guest
  wasm hashes changing under the flag.
- **Sweep plumbing:** `MIDENC_DIFF_FLAGS='<flags>'` (whitespace-split env,
  read by the harness) re-runs the whole corpus under another configuration;
  `run_case_with_flags(name, src, &["--optimize=size-min"])` pins a
  configuration-dependent finding in-repo. The native reference build is
  never affected. Prefix `fuzza-cov`/`fuzza-cov-step` with the same env when
  measuring a configuration, or the profile mixes configurations.
- **Corpus sweep results (117 cases):** `--optimize=max` clean;
  `--debug=none` clean (with campaign 7's debug-on arm, both debug
  configurations agree with native everywhere); `--optimize=size-min` hits
  one known compile-time panic class (see the ignored `spill_loop_mix_oz`
  in `tests/spills.rs`) — `-Oz` keeps count bands un-hoisted, so
  copy-constrained operands sit deeper in the window than at O2/O3.
- Closures in this file that argue "LLVM pre-cleans X" were established at
  opt-level 2; `-Oz` keeps loop structure, avoids unrolling, and prefers
  calls over inlining, so those closures may not hold under
  `--optimize=size-min` unless marked as re-verified there. Re-verified at
  -Oz over the whole corpus (wat sweep, 2026-09-02): no wasm `if`/`else`,
  no result-typed `block` frames, per-site `return`s, loop state through
  locals (the locals argument). REOPENED at -Oz: "everything is inlined
  unless `#[inline(never)]`", "small constant-trip loops are peeled/
  unrolled", and "constant-size copies are inline load/store pairs" — see
  the -Oz shape facts below.
- **-Oz opens no new compiler function** (iteration 8.1, 2026-09-02:
  per-function set diff of the -Oz and default corpus baselines, 117 cases
  each). The only functions warm exclusively under `--optimize=size-min`
  are the NoSolution panic-dump path (OperandStack/OperandType Debug,
  ValueId Display, the `schedule_operands` error closure). Region-level
  novelty is limited to `BlockType::from_wasm`'s single-result `Type` arm
  and the `create_block_with_params::<Vec<Type>>` monomorph it feeds: 17
  corpus cases carry a dead-fallthrough `loop (result i32)` at -Oz (the
  frame LLVM's end-of-function fixup types when a loop's only exits are
  in-loop returns; the O2 corpus has none), whose `end` is translated in
  dead state and hits exactly the translate_unreachable_operator regions
  the O2 corpus already warms. The `FuncType` arm (block params /
  multi-value results) needs `+multivalue` and the `ir_type` error edge
  needs an f64 block result (out of scope), so `from_wasm` is closed at
  every opt-level. The apparent -Oz gains in `emit_if`/`emit_branch_block`
  are unwinding phantoms of the known NoSolution panic (see the gotcha in
  "Operational gotchas"). Verdict: the axis is coverage-poor; its value is
  differential (new guest programs over warm paths).
- **-Oz shape facts** (wat-probed 2026-09-02; cases `case_loop_keep_oz`,
  `case_helper_calls_oz`, `case_mem_libcalls_oz`, `case_switch_loop_oz`,
  `case_band_guard_oz` in `tests/opt_levels.rs`, all pinned via
  `run_case_with_flags`, all passing): at `--optimize=size-min` LLVM keeps
  4-8-trip constant-bound loops that O2 unrolls (nested counted loops,
  early-`break` scans, `continue` loops — 6 kept loops vs 2 at O2); keeps
  ~8-op u32 helpers called from 3-4 sites as real calls (3 functions / 7
  calls vs one fully-inlined function at O2), but a 2-site u64 helper is
  still inlined — it is per-callee size arithmetic, so `#[inline(never)]`
  stays the reliable lever; lowers constant-length copies of >= 48 bytes
  and a 64-byte zero-init to `memory.copy`/`memory.fill` with immediate
  lengths (O2: inline i64 pairs), while 12-24-byte copies stay inline at
  -Oz too and a fill that later stores fully overwrite is DSE'd at both
  levels (immediate lengths reach no new emitter arm — memcpy/memset treat
  the count as a runtime operand); keeps a dense 8-arm `match` in a 5-trip
  loop as ONE in-loop `br_table` (O2: nine unrolled br_tables). A 6-trip
  loop with a heavy u64 checked/overflowing body already stays a loop at
  O2 — no wasm shape difference, yet its -Oz scheduling still warms
  post-op drop and `OpEmitter::swap` arms (`case_u64_checks_oz`).
- **The count-band window boundary moves at -Oz**: the spill_loop_mix
  shape (K masked rotate counts shared between pre-loop code and rotates
  of the loop-carried accumulator, plus the 28/30 live-through pair and a
  light second loop) compiles and passes with K <= 9 at -Oz and panics
  (`NoSolution`, the rotl_window arity-2 class) from K = 10 up, whereas
  K = 16 compiles at O2 and O3 — -Oz keeps the bands un-hoisted, so the
  Copy-constrained count sits deeper in the window. `case_band_guard_oz`
  (K = 9) is the passing guard beside the ignored `spill_loop_mix_oz`.
  Ladder mapped 2026-09-09 (campaign 18, every rung value-checked): with N
  bands dying inside the loop and M bands also used after it, the boundary
  tracks N + M, not the shape of the post-loop code — `N + M <= 11`
  compiles for every M in 1..4 and `N + M >= 12` panics. The KIND of the
  first post-loop op (an arith op, a call to a kept helper, a
  runtime-indexed store, a `select`, a second loop, a bare `return`) does
  not move it by a single rung. The pre-loop DEFINITION ORDER of the bands
  does: dying-bands-first reaches 11, live-first and alternating cap at 10,
  a live/dead/live sandwich at 9. With M = 0 there is no boundary at all —
  twenty bands compile and pass (`drop_batch_oz`), because nothing after
  the loop needs a Copy-constrained count from the bottom of the window.
- **The arity-2 `TwoArgs` gap has four arms, not two**: the tactic
  dispatches on `(a.is_alias(), b.is_alias())` into `copy_copy` /
  `copy_move` / `move_copy` / `move_move`, and `copy_copy` fails
  identically — `[Copy, Copy]` on an `arith.shl` at -Oz, where
  `dup(b_index)` pushes the copied word and the following
  `dup(a_index + 1)` then needs felt index 16 (campaign 18, count-band
  ladder in live-first or alternating band order). Bug specifics live at
  `rotl_window` in tests/spills.rs.
- **The "a NoSolution over more than 16 felts is a spill defect" rule of
  thumb does NOT discriminate at -Oz** (campaign 18): the documented -Oz
  reference `spill_loop_mix_oz` itself dumps a 16-operand / 18-felt stack
  and its ladder siblings dump 15 operands / 17 felts. Use the spills
  trace instead — `MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`
  over the whole count-band ladder logs zero spills, zero edge splits and
  zero unused-phi warnings, because `max_block_pressure` counts LIVE felts
  and operands that are dead-but-not-yet-dropped are invisible to it. No
  spill was requested, so the scheduling problem is in-contract.
- `--test-harness` (codegen emits extra VM test-harness code) is a distinct
  codegen arm that was NOT swept: its executor-side semantics are unclear,
  so a divergence there could be a false finding — investigate before use.
- **`--optimize=max` can overflow the test-runner THREAD stack on a large
  guest** (campaign 13, `copy_mixed`): at max LLVM unrolls loops into a
  program many times larger (an 8467-line MASM here), and the assembler's
  MAST build / debug-engine block execution recurse deeper than the 2 MiB
  proptest test-runner thread allows ("thread ... has overflowed its
  stack; fatal runtime error: stack overflow"). NOT a compiler finding —
  `RUST_MIN_STACK=8388608` makes it pass, and the real midenc/VM binaries
  run on the 8 MiB main-thread stack. If a max sweep aborts a whole batch
  this way, drop the offending large-program case from the max set (it
  still passes at default and -Oz) or re-run that case with a bigger
  `RUST_MIN_STACK`; do not pin it.

- **Configuration dependence of the known panic classes (campaign 16
  sweeps, 2026-09-03; whole corpus at `--optimize=max`, `size-min`,
  `basic` × 256 pairs and a `FUZZA_GUEST_DEBUG=0` sweep):** the guest
  opt-level moves every pressure boundary — the arity-2 gap fires one to
  four count-band rungs earlier at `-Oz`/O1 (bands stay un-hoisted), the
  region-exit budget (deep nests) is hit at depth 7 instead of 9 below O2,
  the labeled-continue-plus-call aliasing panic fires wherever LLVM stops
  inlining the helper (below O2), and O3's full unrolling of constant-trip
  inner loops inside rolled outer loops creates backedge splits whose
  reloads the spill transform erases (stale dominator tree) — the first
  REALISTIC program to panic did so only at O3. Without guest DWARF,
  Local2Reg promotion (blocked under DWARF) creates loop-invariant
  before-block arguments, so the scf loop-invariant-args pattern matches
  programs with no labeled continue at all. Practical rules: sweep every
  kept case at max and size-min (and basic) before calling it a guard;
  `RUST_MIN_STACK=8388608` for max sweeps; a standalone `-Oz` rustc build
  can stackify differently from the harness's build-std/LTO/cgu=1 build,
  so confirm toolchain-class divergences on the harness-built wasm
  (wasmtime on target/miden_test_shared/.../differential_<case>.wasm).
- **Block-end sweep of the 2026-09-09 corpus (359 tests; campaign 23):**
  256 random pairs per case at the default configuration are value-clean
  (zero divergences); at max / size-min / basic / no-DWARF the only
  movements beyond the campaign-16 lists are realistic programs entering
  KNOWN classes (`prog_fixedpoint` at basic → the O3 emitter site;
  `prog_varint_guard` at size-min and `prog_rkscan_guard` at basic → their
  pinned twins; without DWARF `prog_sorts`, `prog_rkscan_guard` and
  `prog_rkscan_wa` → the loop-invariant-args aliasing panic). Rule of
  thumb confirmed: a no-DWARF sweep is the cheapest way to find the
  loop-invariant-args producers hiding behind Local2Reg, and `black_box`
  rescues that depend on constant bands can fall into it.
- **Configuration knobs summary:** `MIDENC_DIFF_FLAGS` (midenc flags,
  per-case flags win per option), `FUZZA_INPUT_PAIRS=N` (random pairs
  per case), `FUZZA_GUEST_DEBUG=0|1|2` (guest debug-info level; default
  2), `run_case_with_flags` (pin a configuration in-repo; the harness
  pseudo-flag `--guest-debug=0|1|2` pins the guest debug level per case),
  `run_case_with_flags_and_inputs` (pin a configuration together with the
  exact input pairs), `run_case_traps` / `run_case_traps_with_inputs` (trap
  parity: a trap on both sides is a match, a trap on one side a finding;
  the env knobs apply to those cases too).

## Debug-info (DWARF) cluster facts (verified 2026-09-02, campaign 7)

The harness flip alone (974f0757e) warmed the decode/schedule/lowering
pipeline wholesale (+2090 regions); marginal case shapes are almost all +0.
Corpus cases: `case_dbg_rebind/negconst/salvage/loop/byval/manylive/
spillmix/match` (differential tests module `debug_info.rs`).

- **rustc -O2 wasm DWARF shapes** (HIR-dump + `MIDENC_TRACE=dwarf=trace`
  probes): value locations `[WasmLocal|WasmStack, StackValue]`; memory
  locations `DW_OP_fbreg(local FP, offset)` for stack aggregates; constant
  location RANGES for named mutable vars before their first mutation —
  the only plain-Rust `DW_OP_consts` producer is a NEGATIVE i64 initializer
  (`case_dbg_negconst`; positive values always come as `DW_OP_constu`, even
  for i64, so the signed-positive Const chain in
  `debug_var_location_from_expression` (lowering.rs:1678) has no producer);
  salvaged dead named defs → arithmetic expressions (`DW_OP_mul/and/shl`
  after the location op) which the decoder catch-all DROPS wholesale — only
  the `DW_OP_plus_uconst` form survives decode (`case_dbg_salvage`).
- **Never emitted by this toolchain** (probe-verified dead ends):
  DW_AT_decl_column on variables; DW_OP_addr for locals (`&STATIC` locals
  constant-fold away, and global-variable DIEs live at CU scope which
  `collect_dwarf_local_data` never walks — it only descends subprogram
  subtrees); DW_OP_WASM_global variable locations (globals appear only as
  frame bases); DW_OP_reg/bregN; low_pc-as-Udata / high_pc-as-Addr forms.
  Const-folded tuples do not produce const-piece expressions (no producer
  for the unsupported-no-index Exprloc edge in `decode_variable_entry`).
- **Serde/print/parse closures**: `DebugVarLocation::Expression` — the only
  path into `ExpressionOp` Serializable/Deserializable — is produced ONLY
  by single-op `FrameBase::Local` declares (plus the theoretical Address
  arm); compound expressions always contain a Wasm-location op and fail
  `debugger_can_safely_evaluate_expression`. Hence the serde arms for
  Plus/Minus/Deref/Piece/BitPiece/ResolvedFrameBase/Unsupported and most
  `read_from` tags are pipeline-unreachable (the salvage API that could
  build such expressions is dead, below). All di `AttrParser::parse` impls
  (Subprogram/Variable/CompileUnit/Expression) are textual-HIR-parse-gated;
  `Subprogram::with_param_names`/`with_function_type` are component-export
  path only (`semantic_debug_signature`).
- **debuginfo/transform.rs**: `salvage_debug_info`/`apply_salvage_action`/
  `collect_debug_ops`/`debug_value_users`/`is_debug_info_op` have zero
  callers outside the dialect (dead API). `erase_debug_info`'s only callers
  — the DeadCodeElimination pass and ControlFlowSink — are commented out of
  the pipeline (midenc-compile backend.rs), so the LIVE debug-erasure path
  is region-DCE's `erase_debug_value` (ir/region/transforms/dce.rs), driven
  by dead named computations whose debug uses survive to HIR
  (`case_dbg_salvage`).
- Debug decorators lower to a DebugVar decorator + a REAL Nop each;
  differentially verified neutral under operand-scheduling pressure, spill
  edge splits, and br_table dispatch (`case_dbg_manylive`,
  `case_dbg_spillmix`, `case_dbg_match`) — the full suite is green with
  DWARF on.

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
- **Pinned edge grids** (`run_case_with_inputs` `<case>_edges` companions) are
  how boundary *semantics* get differentially asserted: a grid guarantees its
  exact pairs on every run, while random draws hit any given boundary only
  probabilistically — even after the harness switched from uniform pairs
  (which essentially never drew 0/1/MIN/MAX/width-boundary values) to a
  boundary-biased mixture (2026-07-31: half-uniform / half boundary-table per
  component, 1-in-8 forced-equal pairs). Grid pairs are also immune to LLVM
  folding because inputs are runtime values
  (`case_shift_counts.rs` … `case_subword_sign.rs`, 2026-07-23).
- **div/rem pair fusion**: `x / d` together with `x % d` on the *same* operand
  pair → LLVM strength-reduces the rem to mul-sub and the VM-side mod ops
  (`u32mod`, `u64::mod`, `i32::wrapping_mod`) never execute. Give remainders a
  mirrored/rotated operand pair with no matching div (masm-verified both ways,
  `case_udiv_bounds.rs`, `case_sdiv_bounds.rs`).
- **Pure defs (and pure `#[inline(never)]` calls!) sink to their use**:
  LLVM infers readnone on internal helpers and moves the computation into
  the use's block, destroying any "defined before the branch, used after
  the join" liveness you were counting on. Pin a call in place by giving
  the helper an opaque atomic side effect that never changes state:
  `PIN.fetch_add(0, Ordering::Relaxed)` folded into the result
  (`case_spill_split`) — deterministic across the 16 reused native
  invocations, unfoldable, and unsinkable.
- An **opaquely-zero value** (impossible cross-modulus guard `as usize`, times
  an input-derived factor) keeps a copy alive that LLVM would elide when it
  can prove `len == 0` or `src == dst` — how a len-0 same-position
  `memory.copy` reaches the VM at all (`case_memnoop_same.rs`).
- **Pressure ladders are cheap**: a warm differential case takes ~2 s and
  the harness runs cases in parallel, so a parametric family (a small
  generator script under `scratch/`, one case file per rung) classifies a
  whole ladder in one `cargo test <module> -- --test-threads=8` run; read
  the panic dumps' felt totals first — a `NoSolution` over MORE than 16
  felts is an over-full stack (spill defect), one within 16 felts is the
  in-contract arity-2 gap (campaign 10).
- **Exit tags in the top nibble + a native scan** (campaign 11): return
  `(tag << 28) | (acc & 0x0fff_ffff)` from a multi-exit shape, then build
  the case as a host binary (`scratch/c11run.sh <case> scan`, rustc only,
  no harness) to list inputs per exit tag — that is how every `_edges`
  grid in `tests/control_flow.rs` is guaranteed to cover each exit and
  each 0-/1-/n-trip path before it is committed. Composite cases pack two
  tags (`a_tag << 30 | b_tag << 28`).
- Widening multiplies do NOT give a multi-use i128: each
  `(a as u128) * (b as u128)` gets its own `zext` pair (no CSE merge even
  when `a` is shared), so no 4-felt Copy-constrained operand is
  constructible from plain Rust (HIR-probed 2026-09-02).

## Runtime edge-semantics facts (2026-07-23 edge-value sweep)

All asserted by pinned-grid differential cases (`case_shift_counts`,
`case_ashr_neg`, `case_udiv_bounds`, `case_sdiv_bounds`, `case_wrap_minmax`,
`case_bitcnt_zero`, `case_memlen_zero`, `case_memnoop_same`,
`case_trip_loops`, `case_subword_sign`), all passing:

- VM lowerings agree with Rust at every probed boundary: shift/rotate counts
  mask `% width` (including counts ≥ 2×width) on u32/u64; `checked_shr` at
  count 0 / width−1 on MIN and −1; unsigned div/mod at divisor 1, divisor ==
  dividend, divisor > dividend, dividend 0, and high-bit-set u64 divisors;
  signed div at MIN/1, MIN % 1, (MIN|1)/−1; wrapping_neg/abs(MIN) == MIN;
  clz/ctz of exactly 0 saturate at 32/64/128; len-0 `memory.copy`/
  `memory.fill` write nothing; 0-trip/1-trip loops; i8/i16 sign boundaries
  through both sext loads and extend8/16_s chains.
- miden-core-lib `memcopy_elements`' overlap assert (`wp >= rp + n OR
  rp >= wp + n`) **accepts n == 0 even at rp == wp** — a zero-length copy at
  an identical position is safe end-to-end (`case_memnoop_same.rs` executes
  exactly that against the always-taken element fast path). The nonzero-length
  overlap abort (`mem_overlap`) is a separate, still-open bug.
- Campaign 12 value ladders (2026-09-02, pinned grids + 512-pair sweeps,
  all passing — `case_sdiv_guards`, `case_div_const_forms`,
  `case_shift_shapes`, `case_ext_chains`, `case_wide_mul_edges`,
  `case_cmp_chains`, `case_bit_shapes`, `case_width_trees`, `case_ovf_mul`,
  `case_int_logs`, `case_div128_guards`, `case_shift128_shapes`): the
  LLVM guard arms for `/0` and `MIN / -1` (checked/wrapping/overflowing/
  euclid forms) on i32/i64/i128/u128 agree with Rust; checked/overflowing
  shifts at counts width/2·width/u32::MAX, sub-word (u8/u16/i8/i16) shifts
  and rotates, u128/i128 rotates and checked shifts across the 64-bit limb;
  sext/zext chains in every cast order incl. i8→u64, sext-then-logical-
  shift, and `i64::from(i32)` products at MIN·MIN; `mul_wide_u/s` hi and lo
  words at MAX·MAX, 2^63·2^63, MIN·MIN, MIN·−1, MIN·MAX; 4-limb u128
  products/carries ((2^64−1)(2^64+1) == MAX, MAX+1 == 0); signed-vs-unsigned
  compares of one bit pattern, `cmp`/min/max/clamp on i64 and i128 with
  high limbs equal or differing by one; swap_bytes/reverse_bits/
  is_power_of_two/next_power_of_two/leading_ones/abs family at MIN on
  32/64/128 bits; ilog2/ilog10/ilog(3)/isqrt/checked_pow at exact powers;
  and mixed-width limb reassembly trees. The only divergences were the
  guest-toolchain class above.
- u128 boundary relations agree with Rust end-to-end (2026-07-23 mop-up
  grids, all passing): `__udivti3`/`__umodti3` at divisor exactly 1,
  divisor == dividend, smallest divisor > dividend, dividend 0, and
  both-limbs-max (u128::MAX) operands (`case_u128_bounds.rs` + `_edges`,
  with `/` and `%` on limb-swapped pairs to defeat div/rem fusion; the
  `u128_udiv`/`u128_umod` derivations cannot reach the ==/MAX relations —
  their `_edges` grids pin the reachable ones); `__ashlti3`/`__lshrti3`/
  `__ashrti3` at counts 0/1/63/64/65/127 in both directions and both signs,
  including the count >= 64 sign-fill (`u128_shifts_edges`,
  `i128_ashr_edges`).

## Known bugs live with the tests

Every `#[ignore]`d differential test is deliberate, and the tests are the
single source of truth for known bugs: each one's doc comment and ignore
reason carry the failure, the exact inputs, what passing sibling cases have
*bounded*, and what would allow un-ignoring. Runtime divergences additionally
carry a pinned `<case>_repro` twin (`run_case_with_inputs` with the exact
failing pair). Read the `#[ignore]`d tests in the differential test modules
before writing cases — some otherwise-reasonable shapes are currently blocked by bugs
documented there — and never re-report one as a new finding. This file
deliberately records no bug specifics, so fixing a bug means cleaning up only
at the test site.

## Operational gotchas

- Case files cannot use `//!` inner doc comments: the harness prepends
  `#![no_std]` + the panic handler ABOVE the case source, so inner doc
  comments land after items and fail with E0753. Use plain `//` comments.
- Agent Bash tools usually start a fresh shell per command — `export
  FUZZA_AREA=...` is lost. Prefix every invocation:
  `FUZZA_AREA='...' cargo make fuzza-cov-step`.
- The report's `Area delta` line inflates by a constant when duplicate
  monomorphized `(file, name)` rows exist — judge productivity by the
  difference of the area *headline* between steps.
- Generic compiler functions can appear as SEVERAL rows: the pipeline's
  live instantiation, unit-test-only instantiations, and `<_, _>`
  unresolved-receiver phantom rows. A "fully untouched" row does not mean
  the function is cold — check the partially-covered table (and
  report.json) for a warm sibling monomorph before treating it as a target
  (2026-08-27: prepare_addr/enforce_alignment read as untouched this way).
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
- `MIDENC_DIFF_FLAGS` is appended AFTER a case's `run_case_with_flags`
  flags, and the harness drops any env flag whose option the case already
  pins (per-case wins, keyed on the text before `=`), so an env-prefixed
  sweep runs pinned `_oz` / `_o3` / `_nodwarf` tests in THEIR configuration
  rather than failing on clap's "cannot be used multiple times" (verified
  2026-09-10: `fir_cordic_guard_o3` passes under a `size-min` prefix). A
  sweep therefore never measures a pinned case in the sweep's
  configuration — add a plain `run_case` sibling if that is what you need.
- **Panic unwinding leaves phantom warm regions.** llvm-cov derives many
  region counts as expressions (entry minus error edge, entry minus
  sibling arm) that do not model unwinding, so a frame a panic unwinds
  through reads `entry N / exit N-1` and its never-taken sibling arms (an
  `assert!` message, an impossible `if let` arm) read count 1 (2026-09-02:
  the spill_loop_mix -Oz NoSolution panic unwinds through `emit_if`'s
  then-branch closure, which is why `emit_if`/`emit_branch_block` read
  +5/+4 at -Oz). Before treating a compile-panicking case's "gains" as
  reachable arms, check the arm-tail counters and the panic backtrace.
- The report's per-function cold-line lists merge regions into line ranges: a
  warm match ARM whose `match` line hosts a small cold sub-region can look
  cold (that is how translate_unreachable_operator's warm End-of-Loop arm
  read as a target). Before betting a case on a specific arm, verify at
  region level — `report.json` carries exact `line:col` spans per region.
  Even REGION-level cold on a dispatch arm can be attribution noise: the
  `OpEmitter::shr` U64 arm's region reads count-0 while its unique callee
  `shr_u64` is 6/6 warm and the corpus HIR provably contains u64 `arith.shr`
  (2026-08-27). When a cold arm has a dedicated callee, check the callee's
  coverage before treating the arm as a gap.
- `MIDENC_EMIT` paths must be ABSOLUTE `kind=DIR` specs: bare kinds dump into
  the test process CWD (that is how stray `.masm`/`.hir` files end up in the
  source tree), and *relative* dirs silently vanish into the ephemeral
  cargo-build workspace. midenc runs in-process on every test invocation
  (cargo caching only affects the Rust→wasm step), so no cache-busting is
  needed to re-probe an unchanged case. `cargo make fuzza-probe` handles all
  of this and writes to `target/fuzza-probe/<case>/`.
- **The printed HIR shows a constant's RESULT type, not its immediate's
  variant.** `arith.constant 10 : i64` prints identically whether the
  attribute is `Immediate::I64(10)` or `Immediate::I128(10)`, and the
  emitter pushes (and models) from the IMMEDIATE (`arith::Constant::emit` →
  `literal(value)`), so an attribute/type mismatch is invisible in every IR
  dump and shows up only in the MASM as a wrong-width push (`push.0 push.0
  push.0 push.10` or `dup.N` ×4 feeding a two-felt `intrinsics::i64::*`
  call) followed by a stack misalignment. When a divergence smells like
  "an operand was replaced by zeros", read the MASM around the consumer;
  the constant-folding coercions (`Sext`/`Zext`/`Trunc::fold`) are the
  known producer of such a mismatch (specifics at `wide::parse_i64_hand`).
- **Tracing a whole module in ONE `cargo test` run needs care with test
  attribution**: with `--test-threads=1 --nocapture`, libtest prints
  `test <name> ... ` BEFORE the test runs, so log lines that follow a name
  belong to THAT test and everything before the first name belongs to nothing.
  A parser that flushes counters when it sees a name attributes every test's
  output to its successor (campaign 19; `scratch/c19pat.sh` + `c19pat.py` get
  it right, and single-test `--exact` runs have no such hazard). The
  `pattern-rewrite-driver` trace lines also carry `dialect=`/`op=` fields, so
  a fired count can be split by the op kind it rewrote.
- **`pass:local2reg=trace` logs "found promotable local X" BEFORE the debug
  check.** The pass may then log "ignoring X: debug declarations cannot all be
  converted safely" and skip it, so the number ACTUALLY promoted is
  (found promotable) - (declare-blocked). Counting only the first line reports
  identical promotion at every debug level, which is exactly wrong.
- `cargo test <filter> -- --exact` needs the FULL test path
  (`end_to_end::differential::tests::<module>::<name>`); a partial path
  with `--exact` silently runs zero tests. Also, extra names after `--`
  are OR-ed with the pre-`--` filter, so `cargo test control_flow:: --
  sm_bits` runs the whole module, not one test.
- A guest that fails to COMPILE (a rustc error such as an ambiguous integer
  literal, E0689) aborts the whole `cargo test` process, not just its test:
  every other test in the batch is lost and the failing tests print no
  dump. Type generated literals (`let k: u32 = if c { 3 } else { 11 }`)
  and check the log tail for `could not compile` before reading results.
  Cheap pre-check: `scratch/c13run.sh <case> grid` builds a case natively
  and evaluates the 35x35 boundary grid (compile errors, native panics,
  non-determinism) in seconds before any harness run.
- **`usize` is 64 bits natively and 32 bits on wasm** — a FALSE divergence
  source (campaign 13, `rodata_big`): `(j.wrapping_mul(31)) % 98304` with
  `j: usize` wraps at 2^32 only on the wasm side, and 98304 is not a
  power of two, so the two sides index different bytes. Keep index
  arithmetic that can exceed 2^32 in `u32` (wrapping ops) BEFORE the
  `as usize`, unless a power-of-two mask/modulus follows. Before blaming
  the compiler, model the case in Python with 32-bit wraps.

## Program-scale composites (verified 2026-09-03, campaign 17)

Realistic `no_std` programs (`tests/programs.rs`: SHA-256, Keccak-f[800],
Murmur3/xxHash32/FNV, a stack-VM interpreter in `match` and fn-pointer-
table forms, 256-bit bignum incl. Montgomery reduction, a Huffman + LZ
decoder, three sorts, Q16.16 DSP + CORDIC, a shunting-yard parser with an
arena AST, Dijkstra, Levenshtein/LCS/Needleman-Wunsch, CRC/Adler/LFSRs,
`core::fmt` into a stack buffer, iterator pipelines) all compile and match
native at the default configuration. Facts they established:

- **Slice / array `==` is unlinkable in guests** (REFINED by campaign 27's
  link-reach section below: a constant-size array `==` links at every level
  except `-Oz`, and the `str::split(char)` result is opt-level dependent):
  `[u32; 64] == …` and any runtime-length slice equality lower to a `memcmp`/`bcmp`
  libcall and the guest link fails with `rust-lld: undefined symbol:
  memcmp` (no wasi-libc, compiler-builtins' `mem` symbols absent).
  This includes core code that compares slices internally — `str::split`
  with a `char` pattern (`CharSearcher` compares the encoded char with
  `==`) — bisected standalone in `prog_fmt`. Compare element-wise / scan
  bytes by hand. A user program hitting this gets a link error, not a
  midenc diagnostic.
- **`core`'s unstable sorts are recursive** — SUPERSEDED 2026-09-10 by
  campaign 27: only `select_nth_unstable` still is (`median_of_medians`);
  `sort_unstable` / `_by` / `_by_key` link at every level because build-std
  runs with `optimize_for_size` (non-recursive `heapsort`). The assembler's
  `found a cycle in the call graph` error remains the symptom for any
  recursion; `binary_search`, `rotate_left`, `reverse`, `split_at_mut`,
  `fill`, `copy_from_slice`, `swap` and the iterator adapters (`zip`,
  `windows`, `chunks_exact`, `rev().enumerate()`, `max_by_key`,
  `position`/`rposition`, `step_by`, `take_while`, `skip`, `cycle().take`,
  `nth`, `find`, `min`, `any`/`all`, `filter().count()`) are loops and
  pass (`prog_iters`; the sort probe was deleted).
- **`core::fmt` runs on the VM**: `write!` into a `fmt::Write` stack
  buffer with Display / LowerHex / UpperHex / Binary / Octal / signed /
  width / alignment / `{:?}` of slices and tuples, plus `str::parse` and
  `from_str_radix` round-trips, agree with native (`prog_fmt`) — the
  `dyn Write` vtable dispatch, `Formatter::pad_integral` and `DebugList`
  paths execute correctly (previously linked only as dead panic paths).
- Every program that keeps an unprovable bounds check links
  `core::panicking` + `core::fmt` (~600 KB wasm, never executed) — a
  compile-time / package-size cost only.
- Cost calibration: Keccak-f[800] (three 22-round permutations on 25 u32
  lanes) is ~4 s per input pair in the step-mode executor — the most
  expensive realistic shape in the corpus; the other programs stay under
  ~0.5 s per pair. Budget deep sweeps accordingly.
- `gen` is a reserved keyword in edition 2024 — name generators
  `gen_str` etc. (a guest that fails to compile aborts the whole test
  batch).
- The u128 multiply-accumulate schoolbook chain (`t = a*b + r + carry` as
  u128, `r = t as u64`, `carry = t >> 64`) is valid at every opt level
  and debuginfo level (`prog_bignum`: 16 `mul_wide_u` + 24 `add128`, no
  stale-read signature standalone at o2/o2d2/o3/oz) — unlike the C15
  add-with-carry idiom, so it is safe in passing guards.
- **`--optimize=max` is a pressure lever realistic programs pull by
  themselves**: LLVM fully unrolls constant-trip inner loops (a 16-tap
  i64 multiply-accumulate over a stack array becomes ONE block of 16
  products) while the enclosing data-length loop stays rolled, so values
  shared by the code before the loop, the loop and the code after it are
  spilled at the header with reloads on the split backedge — the shape
  behind `prog_fixedpoint_o3` / `fir_cordic_o3` (tests/programs.rs; the
  same programs pass at opt-level 2 and `-Oz`, where the tap loop stays
  rolled). Sweep every kept program at max and size-min before calling it
  clean; pin configuration findings with `run_case_with_flags` twins.
- libtest's `--skip NAME` is a SUBSTRING filter: `--skip probe_m1` also
  skips `probe_m10`..`probe_m19` (a ladder batch that silently runs zero
  tests). Use `-- --exact <full path> <full path> …` to select rungs.

## Realistic programs at the freight cliff (verified 2026-09-09, campaign 21)

Twelve `no_std` kernels written the way a user writes them, but each built
around one of the structured-control-flow shapes campaign 20 measured the
spill-freight cliff on (corpus cases in `tests/programs.rs`, `prog_*` /
`prog_*_guard`). Seven of the twelve fail to COMPILE at the default
configuration, so the cliff is not a synthetic-ladder artifact.

- **Realistic programs produce crossing count bands by construction.** The
  producers are ordinary idioms: a mixer's rotation constants reused in the
  seed, in the loop and in the finalization; a two-pass algorithm reusing the
  bucket-selection shifts in both passes; a sponge sharing the permutation's
  rotation offsets between absorb and squeeze; a decoder's error paths each
  folding the same running statistics. No `#[inline(never)]` pinning, no
  artificial cluster expression is needed.
- **The number of DISTINCT rotate/shift constants is the lever, not the number
  of u64 state values.** An eight-lane sponge compiles with four distinct
  rotation offsets and panics with eight; a Rabin-Karp scanner still panics
  with two fingerprint words instead of six but compiles once five rotate
  constants become three. Reducing the u64 state alone moved the boundary only
  where the state values were themselves the operands of distinct constants
  (Feistel round keys, TLV digest words).
- **In-loop wide dispatch over u64 bookkeeping is the SURVIVING shape.** A
  20-opcode stack VM and a 16-state framer, each with six to eight u64
  accumulators updated by every arm and combined in one per-step expression,
  compile and match native at all four optimization levels; the shapes that
  break are return-heavy search loops nested in an outer loop, asymmetric
  diamonds in hot loops, two passes sharing constants, and three-level nests
  whose deepest arm is the only consumer. A chain of early-`break` scans
  remains the most tolerant (campaign 20's ranking holds at program scale).
- **Opt level is not a safety ladder.** Among the seven panicking programs,
  `--optimize=size-min` rescues four, `--optimize=max` rescues one (a different
  one), `--optimize=basic` rescues two, and two programs panic at all four
  levels. A user who hits this cannot rely on "try another `-O`".
- **F12 (`remove-loop-invariant-args-from-before-block` →
  `AliasingViolationError` at hir/src/patterns/rewriter.rs:335) has plain-Rust
  producers at the DEFAULT level** with no labeled `continue` and no call in
  the loop: a decoder loop with several early error returns carrying five or
  six u64 statistics. Confirm the class by the driver's last line under
  `MIDENC_TRACE='pattern-rewrite-driver=trace'`. Reducing the carried state
  moves the default-level boundary but not the `--optimize=size-min` one (one
  running value still panics there).
- **The `frontier.rs:123` unwrap needs only FOUR crossing bands** in the
  sequential-loop shape (bands used before the first bottom-tested loop and
  again only inside the second; `pressure::frontier_seq`) — campaign 20 had
  measured eight. Two and three bands compile, and the same shape with a single
  loop compiles at every band count tried. In the dispatch shape
  (`pressure::frontier_dispatch`) every ingredient is necessary at its size:
  sixteen arms, THREE dynamically impossible `panic!()` arms, nine bands used
  before / inside / after the loop; dropping any one of them compiles.
- **The non-monotone cascade band boundary is not an LLVM shape change.** For
  the empty-`continue` cascade at eleven bands (panics) and twelve (compiles),
  the guest wasm is structurally identical — same locals `i32 i64 i64 i32 i32`,
  same 4 loops / 2 blocks / 2 selects / 4 `br_if`, twelve simply carrying one
  band's extra ops — and so is the pre-lift HIR (4 blocks, 347 vs 366 ops). The
  difference is in the spill analysis: eleven bands give 25 spills / 37 reloads
  / 4 split edges / 8 erased split reloads and a `NoSolution`, twelve give 25
  spills / 44 reloads / 6 split edges / 10 erased — the surviving (non-split)
  reloads are what happen to keep the failing op inside the window. Do not look
  for an LLVM explanation of non-monotone band boundaries.
- **`SinkOperandDefs` cannot sink a spill-slot reload, and never sinks into a
  region.** It moves an operand's defining op next to its use *within a block*;
  the pass that moves ops into regions is `ControlFlowSink`, registered but
  never scheduled. `hir.load_local` carries `MemoryEffect::Read`, so the pass
  logs `defining 'hir.load_local' cannot be moved: * op has memory effects` for
  every reload (74 times on `interact::sink_spill`). Any hypothesis of the form
  "the post-lift sink moves the reload into the region" is closed.

## F12's producer rule and the workaround map (verified 2026-09-09, campaign 22)

The `RemoveLoopInvariantArgsFromBeforeBlock` aliasing panic (F12,
`AliasingViolationError` at hir/src/patterns/rewriter.rs:335) and the seven
campaign-21 programs that do not compile at the default configuration, taken
apart with post-lift IR dumps (`-Z print-ir-after-pass=lift-control-flow`
plus `MIDENC_TRACE='pass:lift-control-flow=trace'`) and one-feature-at-a-time
source ladders (corpus cases `compose::invariant_args_min`/`_guard` and
`programs::prog_*_wa`).

- **The pattern's "loop-invariant before-block argument" is a payload column
  that still carries `ub.poison`.** cfg-to-scf materialises exactly ONE
  `ub.poison` value per TYPE per function and uses it as the initializer of
  every `scf.while` payload column, so every `scf.while` in a function reads
  `scf.while %poison, %poison, ...`. Both of the pattern's invariance tests
  compare against that init operand, so they degenerate into "is this operand
  the poison value?" — the pattern matches as soon as ONE column still carries
  poison at the `scf.condition`/`scf.yield`. The panicking minimal reproducer
  has an in-body `scf.if` whose EVERY arm yields poison in one column;
  canonicalization (`convert-trivial-if-to-select`) collapses that column to
  the poison value itself, the condition op forwards it, and the pattern fires.
  Its passing sibling has no all-arms-poison column. A case that compiles
  proves the pattern did not match — it fires on every match, and its rewrite
  always aborts.
- **The source-level producer is a `return` that leaves the FUNCTION from
  inside a two-level loop nest** (the returned value is the exit payload
  cfg-to-scf must thread out, and it is undefined on the continuing path).
  Verified on seven variants of one 26-line nest: an inner `loop` whose first
  statement is an early `return` panics; so does the same nest with the inner
  loop setting a flag and the `return` in the outer body, and so does a version
  whose returned value is defined on every path (the poison column is the
  synthesized dispatch payload, not the user value). The same nest compiles
  when the inner exit is a labeled `break` or a labeled `continue`, when the
  nest is flattened to a single loop with the same early `return`, and — one
  statement moved — when the `return` is placed BELOW the inner `break`.
- **A `return` is not required: the SECOND producer is an inner counted loop's
  merged EXIT DISPATCH** (verified 2026-09-10, campaign 28, minimal case
  `compose::invariant_args_noreturn`, reduced from `programs_oz::prog_blake2b`
  and containing no `return`, `break` or `continue` at all). Shape: an outer
  loop over a chaining state and an inner counted loop over array-indexed state
  with `.rodata` index tables. In the lifted IR the inner `scf.while`'s
  `before` region ends in an `scf.if` on the mixing loop's exit test whose
  TWELVE results have their first SIX columns equal to the function's single
  `ub.poison` in BOTH arms; canonicalization collapses each all-arms-poison
  column, `scf.condition` forwards it, and the pattern matches — the same IR
  rule as the `return` producer, with the poison coming from the payload the
  outer loop's `scf.index_switch` exit dispatch selects on. The one-ingredient
  sibling that compiles differs only in the inner loop's trip count (six
  instead of seven): LLVM unrolls it, cfg-to-scf sees ONE `scf.while` whose
  condition forwards only real values, and no column carries poison. So the
  question to ask of a source program is "does cfg-to-scf still see a nested
  loop with a merged exit?", not "does it contain an early exit?".
- **Whether a given source program reaches that IR shape is decided by how much
  LLVM leaves for cfg-to-scf, not by an idiom.** On the reduction ladder from
  `prog_varint`, removing the byte buffer, three of the four error returns,
  five of the six running values, the u64 width and the value accumulator all
  keep the panic; removing one of the two xorshift steps, replacing the
  xorshift with an LCG, or dropping the single post-loop `rotate_left` of a
  band shared with the loop body all make it compile. Reduce by removing
  features, not by reasoning about which statistic "looks invariant".
- **F12 is not an opt-level ladder and DWARF can widen it.** The minimal
  reproducer panics at O2, O3 and O1 and compiles at `-Oz`, identically with
  and without guest DWARF; `prog_varint` panics at O2, `-Oz` and O1, compiles
  at `--optimize=max` WITH DWARF and panics at max WITHOUT it
  (`FUZZA_GUEST_DEBUG=0`) — the campaign-16 no-DWARF widening reproduces at
  program scale.
- **`core::hint::black_box` on the rotate/shift constants is the cheapest
  rescue for the freight-driven panics.** Wrapping every USE of a program's
  rotate/shift constants (the `const` definitions stay) turns the CSE-merged
  count bands into runtime counts and rescues four of the five F6 programs and
  one of the two F12 programs at the default level, with no restructuring and
  the same answer on every input. It costs about 1.6-4.8x the MASM of the
  reduced campaign-21 guard (e.g. 3999 vs 2195 lines for the Rabin-Karp
  scanner, 3840 vs 804 for the Feistel mixer).
- **"Move the hot loop into an `#[inline(never)]` helper" works only if the
  state is passed BY REFERENCE.** With six to eight u64s passed by value the
  call's argument list is 17 felts and the spill analysis panics at
  hir-analysis/src/analyses/spills.rs:2366 ("unable to spill sufficient
  capacity to hold all operands on stack at one time at hir.exec ..."): the
  spill loop's candidate set excludes the instruction's own operands, so it
  runs out. The same helper taking `&[u64; N]`/`&mut [u64; N]` compiles for
  every program tried. A call signature at or over 17 felts is not lowerable;
  keep helper signatures small.
- **The rewrites that do NOT move any of these panics**: replacing N named u64
  state values with one `[u64; N]` (LLVM SROAs it straight back), and replacing
  `x.rotate_left(C)` with a `(x << C) | (x >> (BITS - C))` helper (LLVM
  re-canonicalises it to the same funnel shift, so the count bands are
  unchanged). Both were checked on all seven programs and rescued none.
- **A three-level diamond nest whose deepest arm is the only consumer of
  pre-computed words is rescued by moving THAT ARM'S expression into an
  `#[inline(never)]` helper** (or by computing the words lazily inside the
  arm), while moving the whole record loop into a helper, splitting the program
  into three functions, and flattening the diamond into an `if`/`else if` chain
  all leave the `frontier.rs:123` unwrap in place. Shrink what is live ACROSS
  the loop, not the loop's own shape.

## The release configuration: guests without full DWARF (verified 2026-09-10, campaign 24)

A user's `cargo miden build` emits no guest DWARF; the differential harness
builds with `debug = 2`. These are two different pipelines, and this section
records the difference as facts. The per-case knob is the harness pseudo-flag
`--guest-debug=0|1|2` in `run_case_with_flags*`; the sweep knob is
`FUZZA_GUEST_DEBUG`.

- **`debug = 1` (line tables only) is on the DWARF-OFF side of every known
  boundary.** Line tables carry no variable DIEs, so
  `convert_debug_references_for_local` finds neither declares nor values and
  takes its "no debug references" early return — promotion runs exactly as at
  `debug = 0`. Whole-corpus sweeps at `FUZZA_GUEST_DEBUG=1` and `=0` produce
  the IDENTICAL failure list, panic site for panic site. Only `debug = 2`
  masks anything. (The F9 guest-toolchain family was already known to need
  `-C debuginfo=2`; this extends the same rule to the compiler-side F12
  cluster.)
- **Local2Reg's promotion contract, and what the debug level actually
  changes.** A slot is promoted only when it has EXACTLY ONE `hir.load_local`
  and EXACTLY ONE `hir.store_local`, both in the same block, with no op
  implementing `BranchOpInterface` / `RegionBranchOpInterface` /
  `CallOpInterface` between them. Everything else is skipped with a specific
  trace reason ("loaded more than once", "stored more than once", "load and
  store are in different blocks", "found control flow between load and
  store"). The debug level does NOT change that candidate set — the load/store
  trace is byte-identical at `debug = 0` and `debug = 2` — it changes only
  whether `convert_debug_references_for_local` lets an otherwise-eligible slot
  through. Measured over the seven `debug_info::l2r_*` cases: 4 slots promoted
  with full DWARF, 27 without (`l2r_params` alone: 0 vs 14).
- **Function parameters are the promotable population.** The frontend stores
  every parameter unconditionally at entry, so a parameter read once in
  straight-line entry-block code is the canonical promotable slot; loop
  accumulators (many loads), values assigned in several `match` arms (many
  stores), values live across a call (control flow between store and load) and
  loop-carried locals (load and store in different blocks) are rejected
  STRUCTURALLY at every debug level. A `&mut` local is not a wasm local at all
  (its address escapes into the shadow stack), and a constant-indexed
  `[u32; N]` is SROA'd into ordinary promotable scalars while a
  runtime-indexed one is not.
- **The dead-store-erasure arm is not debug-gated**: an unused parameter gets
  no DWARF variable, so the conversion succeeds and the stores are erased at
  every debug level ("preserving dead stores" needs a declare, which rustc
  does not emit for a dead parameter).
- **The corpus is value-clean in the release configuration.** 359 tests x 256
  boundary-biased input pairs at `FUZZA_GUEST_DEBUG=0`: zero divergences; the
  only failures are compile-time (the F12 cluster below) plus the known F9
  value-use family (`signed::sext_shapes`, `wide::wide_loop_cmp`,
  `wide::wide_words` and their twins). The same holds at `=1`. Promotion
  changes codegen, not semantics.
- **Promotion does not move the F2 count-band boundary.** The
  `spill_loop_mix` / `band_guard_oz` family swept over K = 8..16 at the
  default level and K = 6..12 at `--optimize=size-min`, each rung at guest
  debug 0 and 2: `-Oz` panics from K = 10 up and compiles at K <= 9 in BOTH
  configurations, and the default level compiles every rung to K = 16 in both.
  The panic is the same arity-2 `NoSolution` on `arith.rotl` with
  `[Copy, Move]` at lowering.rs:109. Do not re-run this ladder.
- **What the release configuration DOES move is F12's producer set.** Programs
  that compile with DWARF and panic at `rewriter.rs:335` without it:
  `compose::chain_sm`, `programs::prog_rkscan_guard`, `prog_rkscan_wa`,
  `prog_sorts` (pinned twins in-repo). One program changes CLASS with the
  debug level — `prog_rkscan` panics at the emitter's window assert
  (`emit/mod.rs:623`, F6) with DWARF and at `rewriter.rs:335` (F12) without —
  because the promotions reach cfg-to-scf before the spill freight reaches the
  emitter.
- **In the release configuration the campaign-22 workaround map does not
  hold.** For the Rabin-Karp scanner, `black_box` on every rotate-constant
  use, moving the hot expression into an `#[inline(never)]` helper taking
  `&[u64; 4]`, and replacing all four escaping `return`s with a single
  `break 'outer` exit ALL still panic at `rewriter.rs:335` at `debug = 0`,
  while all three compile with DWARF and compute the same answer. The only
  escape found is `--optimize=size-min`, which compiles the scanner (and
  `prog_sorts`) even without DWARF.
- **Two individually-safe configurations can compose into a panic.**
  `prog_varint_wa` (the black-boxed decoder) compiles at `--optimize=size-min`
  with DWARF and at the default level without it, and panics at
  `rewriter.rs:335` with BOTH together. Validate a workaround at the exact
  configuration the user ships, not one axis at a time.

## Spill slots, frames and recursion (verified 2026-09-10, campaign 25)

Where a spill slot actually lives, what a guest frame can do to it, and what
the guest stack limit does when it is crossed (corpus cases in
`tests/frames.rs`).

- **A spill slot is a Miden PROCEDURE LOCAL, not linear memory.**
  `TransformSpills` allocates one function local per spilled value and rewrites
  each reload into a `hir.load_local` of it; the emitter addresses every local
  with `locaddr` (`OpEmitter::local_address` at codegen/masm/src/emit/mem.rs:122
  emits `Locaddr(LocalVariable::absolute_offset)`; `load_local` / `store_local`
  at mem.rs:137 / mem.rs:641 push that address and load/store through it), and
  the assembler advances FMP by the procedure's WORD-aligned local count on
  entry (codegen/masm/src/lower/component.rs:1574-1590: "locaddr.N computes
  -(aligned_num_locals - N)"). So slots are PER ACTIVATION by construction, and
  no recursion or call structure can share them.
- **Slots and guest stack arrays are in disjoint address regions.** FMP starts
  at element address 2^31 (`miden_core::FMP_INIT_VALUE`, miden-core-0.29.x
  src/lib.rs:118; `FMP_ADDR = u32::MAX - 1`), while the guest's wasm linear
  memory maps to element addresses below 2^19 (17 pages = 278 528 elements).
  A frame array cannot alias a spill slot however large it is — the
  slot/array, callee-clobber and frame-fill overlap hypotheses are
  structurally impossible, and the corpus keeps one value-checked guard each
  anyway (`frame_spills` with a 256 KiB array, `call_clobber` with three
  spilling levels that `memset`/`memcpy` their whole frames, `fill_spills` with
  the bulk ops inside the spilled loop).
- **Slot indices sit above the user locals.** For `frame_spills`,
  `-Z print-ir-after-pass=local2reg` shows locals 0..21 and
  `-Z print-ir-after-pass=transform-spills` shows locals 0..39: the eighteen
  new locals are exactly the eighteen reloads the pass logged as "convert
  reload to load".
- **Direct and mutual recursion do not compile — the ASSEMBLER rejects them**
  ("found a cycle in the call graph", miden-assembly-0.29.x
  src/linker/errors.rs:41). Recursion is only expressible through a funcref
  table (`hir.exec_indirect` -> `dynexec`), which stays in the caller's memory
  context: only the new-context `dyncall` writes `FMP_INIT_VALUE`
  (miden-processor src/execution/dyn.rs:86/207), so a recursion chain shares
  one FMP chain and each activation still advances it.
- **The guest shadow stack is 1 MiB and overrunning it is SILENT.** The guest
  wasm declares `(global $__stack_pointer (mut i32) i32.const 1048576)` with
  the data segments at 0x100000 and `(memory 17)`. Nine activations of a
  128 KiB frame ask for 1 179 648 bytes, so `__stack_pointer` wraps below zero
  and the deepest frame lands at 0xFFFE0000. `wasmtime` on exactly that
  harness-built wasm TRAPS ("memory fault at wasm address 0xfffffff8 in linear
  memory of size 0x110000 / wasm trap: out of bounds memory access";
  0xfffffff8 is the deepest frame's last element), while the Miden pipeline
  emits no bounds check and no diagnostic: it maps the wrapped byte address
  into element space just under 2^30 and executes silently, computing the RIGHT
  answer as long as nothing else lives there. Stack overflow in a Miden guest
  is therefore silent memory corruption, not a trap (corpus: `deep_frames` =
  largest fitting rung, `deep_overrun` = the first overrunning one).
- **The native ceiling of a recursion ladder is the 2 MiB libtest thread
  stack, not 8 MiB.** A case whose NATIVE recursion needs more (nine 256 KiB
  frames) aborts the whole test process with "has overflowed its stack /
  fatal runtime error: stack overflow" (SIGABRT), which loses every other
  test's output in that run, including the failure dumps of tests that had
  already failed. Keep native frame totals around 1 MiB, or set
  `RUST_MIN_STACK`.
- **Freight boundary inside a recursive frame** (u64 cluster consumed by one
  wide right-leaning chain per loop body, count bands before/inside/after, the
  recursive dispatch between the two loops): (cluster, bands) = (2, 2), (2, 4)
  and (3, 3) compile and pass; (4, 2) is the in-window arity-2 `NoSolution`
  (lowering.rs:109, `arith.rotl`, `[Copy, Copy]`, exactly 16 felts); (4, 4),
  (6, 6) and (8, 8) are the over-full stack (emit/mod.rs:623, indexes 11 / 13 /
  14). Cluster values are the expensive axis in a frame that also dispatches,
  bands the cheap one — the opposite of the campaign-21 program-scale rule,
  where the number of distinct rotate constants was the lever.
- **Value verdict: no divergence anywhere in this area.** Recursion depths 0..5
  with an eight-u64 cluster live across the dispatch, mutual recursion with
  asymmetric freight, a 256 KiB frame array written at its extremes while
  values are spilled, three call levels that each spill and refill their whole
  frame, `write_bytes`/`copy_from_slice` inside a spilled loop, spills across a
  funcref dispatch and across two return-area calls, and 512 KiB of recursive
  frames all agree with native on pinned grids and at `--optimize=max`,
  `--optimize=size-min` and `FUZZA_GUEST_DEBUG=0`.

## Real programs at -Oz: the constant count is not the lever (campaign 26, 2026-09-10)

Ten `no_std` kernels whose ALGORITHM carries its rotate/shift constants
(Keccak-f, SHA-512, Threefish-256, xxHash64+Murmur, SplitMix/xoshiro/PCG,
base64, ChaCha20+Salsa20, bit reversal/Morton, SipHash-2-4, BLAKE2b), each run
at `--optimize=size-min` with guest debug 2 AND 0, at the default level, at
`--optimize=max` and at `--optimize=basic`. Corpus cases in
`tests/programs_oz.rs` (`prog_*_oz`, pinned with `run_case_with_flags`).

- **The synthetic `-Oz` band cap (N + M <= 11) does NOT transfer to real
  programs.** A 12-round Keccak-f[1600] with the real TWENTY-FOUR rho offsets
  unrolled inside its kept round loop compiles and matches native at every
  optimization level, while a Threefish-256 with FIFTEEN rotation constants
  does not compile at `-Oz` at all, and a four-constant BLAKE2b does not
  compile at the default level. What separates them is not the count but how
  many band results are live as scheduled OPERANDS across the loop: Keccak's
  25 lanes live in a stack array, so every rotate consumes a freshly loaded
  lane, whereas Threefish carries four u64 words in scalars that every one of
  its fifteen constants rotates. Array-resident state is the structural
  reason a big permutation is safe — cite that, not the constant count.
- **A real `-Oz` constant ladder is NON-MONOTONE and cannot be used as a user
  rule.** Merging Threefish's rotation rows down from fifteen distinct
  constants: 13, 12, 11, 9 panic, 8 compiles, 6 panics, 4 compiles (every rung
  value-checked natively, every panic at emit/mod.rs:623). "Use fewer distinct
  rotation constants" is therefore NOT a reliable fix. The campaign-18
  synthetic rescue "no post-loop use of the bands (`M = 0`)" also fails on the
  real program: keeping all fifteen constants but folding the output without
  rotations still panics.
- **Three window-overflow signatures, told apart by the spills trace
  (director re-classification, 2026-09-10).** (1) Erased SPLIT-edge reloads
  — `edges to split > 0` and `erase unused reload` lines — is the stale
  dominator tree (`prog_sha512` at the default level: 4 split edges, 14
  erased). (2) An arity-2 `NoSolution` with a Copy constraint on a stack
  that is IN the window (<= 16 felts) is the solver gap whether or not the
  function spilled elsewhere: Threefish at `--optimize=max` dumps 8
  operands / 15 felts on `arith.rotl` `[Move, Copy]` with `edges to split =
  0` — its single "erase unused reload" is not a split-edge reload — so it
  is the first REALISTIC-program producer of the arity-2 gap. (3) Over-window
  pressure at the emitter (`emit/mod.rs:623`) with spills requested but NO
  edge splits and NO erasure — Threefish at `-Oz`: 8-10 values spilled,
  fourteen `max usage on exit (17)/(18) exceeds K (16), additional spills
  required` lines, `edges to split = 0` — is a third mechanism (the analysis
  ran and still let the emitter's stack exceed 16 felts; ledger entry F17).
  The emitter drop trace (`codegen:operand-scheduling=trace`) shows its
  failing op is the SPILL STORE itself (`hir.store_local` into a spill
  slot): the value chosen for spilling is already past the window when the
  store is emitted. Never classify by the crash site or by the felt total
  alone; when a panic is at `emit/mod.rs:623`, read the last
  `dropping unused operands at:` line of the drop trace to learn which op
  was being emitted.
- **The `-Oz` escape hatch runs in BOTH directions, per program.**
  `prog_sha512` (F6) and `prog_blake2b` (F12) panic at the default level, at
  max and at basic and compile ONLY at `-Oz`; `prog_threefish` compiles at the
  default level and at basic and panics at `-Oz` (with and without guest
  DWARF) and at max; `prog_xxh64` compiles everywhere except basic (F12).
  Guest DWARF moved nothing in this family: the `-Oz` results at
  `--guest-debug=0` are identical to `debug = 2`, panic site for panic site.
- **F12's producer set is wider than "a `return` from a two-level nest".**
  `prog_blake2b` (default, max, basic) and `prog_xxh64` (basic) hit
  `rewriter.rs:335` while containing NO `return`, `break` or `continue` at all
  — plain `while` nests over array-indexed state with an `if`-guarded tail are
  enough. Nor is F12 count-driven: its producer here has the FEWEST distinct
  constants of the ten programs (four).
- **What to tell a user whose `-Oz` build panics**: wrap the rotate/shift
  counts in `core::hint::black_box` (25 one-token edits on Threefish, same
  answer on the 1225-pair native grid, passes at `-Oz` with and without DWARF,
  at the default level, at max and at basic), or move the round function into
  an `#[inline(never)]` helper taking the state BY REFERENCE (`&mut [u64; 4]`,
  also passes in all five). Both are the campaign-22 rescues, and unlike the
  release-configuration Rabin-Karp result they do not fall into F12 here.
  Reducing the constant count is the one thing that does not work.
- **What `-Oz` keeps in these programs** (wat-probed): Keccak's permutation
  stays a real call from three sites and the program keeps ten loops;
  bit-reversal/Morton keeps `morton_spread`/`morton_compact` (four call sites)
  and inlines the single-use `reverse_bits64`; xxHash64's `round` and
  SplitMix's `splitmix` are INLINED even at `-Oz` (one function, no calls,
  eight kept loops) — per-callee size arithmetic, as the earlier -Oz shape
  facts say. When a program looks "safe at -Oz", check whether the reason is
  that LLVM kept the round function as a call.
- **Value verdict: zero divergences.** Twelve cases (the ten programs plus the
  two Threefish rescues) x 512 boundary-biased input pairs at `-Oz` all agree
  with native, as do the pinned per-path grids (block counts, message lengths,
  every xxHash tail path).

## The `core` link-reach map (verified 2026-09-10, campaign 27)

Which parts of `core` a `no_std` guest can use at all, measured facility by
facility at all four optimization levels (corpus cases in `tests/corelib.rs`,
`core_*`). The blocking symbol is always `memcmp`: a guest has no wasi-libc
and no compiler-builtins `mem` symbols, so any comparison `core` performs as
a slice compare fails the link with `rust-lld: error: <obj>: undefined
symbol: memcmp`. A user sees a linker error, never a `midenc` diagnostic.

- **Linkability is a per-PROGRAM, per-OPTIMIZATION-LEVEL property, not a
  per-API one.** When the comparison is inlined it folds into ordinary loads
  and the program links; when it stays outlined the libcall survives.
  Measured (default / `-Oz` / max / basic): a constant-size
  `[u8; 4] == [u8; 4]` links everywhere; `[u32; 8] == [u32; 8]` and a derived
  `PartialEq` over a `[u8; 16]` field link everywhere EXCEPT `-Oz`; ONE
  `str::split(char)` or `split_once(char)` per function links except at
  `--optimize=basic`; `str::find(char)` / `rfind(char)` link only at the
  default level and at max; TWO nested `split(char)` loops in one function
  link at no level at all. So "does `core::str` link" cannot be answered per
  API — it has to be measured for the program (campaign 17's "`str::split`
  with a `char` pattern is unlinkable" was one point of this surface, not the
  rule).
- **Unlinkable at every level**: runtime-length slice `==` / `!=` / `<`,
  `&str == &str` (and `Option<&str> ==`, even between same-length literals),
  `[u8]::starts_with` / `ends_with`, `str::starts_with(&str)`, and
  `str::contains(&str)` / `find(&str)` / `rfind(&str)` (the two-way
  searcher). The LLVM-IR-verified source for the splitters and searchers is
  `core::str::iter::SplitInternal<char>::next`, which compares the encoded
  pattern bytes with a slice `==`.
- **The linkable replacements**: `iter().eq(..)`, `zip(..).all(..)`,
  `iter().cmp(..)`, `str::eq_ignore_ascii_case` /
  `[u8]::eq_ignore_ascii_case`, `[u8]::contains` (element compare), a
  constant-size array `==` outside `-Oz`, and a hand-written `char_indices`
  scan in place of a splitter. `case_prog_expr_wa.rs` is a worked example: the
  hand-written splitter computes the identical answer on the whole 1225-pair
  native grid.
- **`core`'s unstable SORTS link again** — `sort_unstable`,
  `sort_unstable_by`, `sort_unstable_by_key` at every level — because the
  compiler builds `core` with `-Zbuild-std-features=optimize_for_size`, whose
  unstable sort is the non-recursive `core::slice::sort::unstable::heapsort`
  (the symbol is in the guest wasm's name section). This supersedes campaign
  17's "the unstable sorts are recursive and unusable". `select_nth_unstable`
  is the exception: it keeps `core::slice::sort::select::median_of_medians`,
  which calls itself, and the assembler still rejects it with `found a cycle
  in the call graph`.
- **A constant length hides both**: with a constant-size slice LLVM
  specialises `select_nth_unstable` away entirely (no cycle) and folds
  `str::parse` at compile time (no runtime parse). Any probe of these has to
  take its length from the input.
- **Everything else probed passes at all four levels**: the non-comparing
  `core::str` surface (`from_utf8`, `char_indices`, `chars().rev()`, `trim*`,
  `split_whitespace`, `is_char_boundary`, `get`, `encode_utf8`,
  `parse::<u32>`, `from_str_radix`), `core::fmt` including `{:#?}` on derived
  `Debug` over nested enums/structs/arrays/`Option`/`&str`/`char`, the
  iterator adapters, the in-place slice algorithms (`rotate_*`, `reverse`,
  `fill`, `swap`, `split_at_mut`, `copy_within` with distinct ranges,
  `chunks*`, `windows`, `binary_search*`, `is_sorted*`), `Option`/`Result`
  combinators with `?`, `mem::swap` / `replace` / `take`, derived `Ord` with
  `max_by_key` / `min_by` / `clamp`, `array::from_fn` / `map`, the `char`
  APIs, and `core::ptr` (`read_unaligned` / `write_unaligned` / `copy` /
  `copy_nonoverlapping` / `write_bytes` / `offset_from` / `align_offset`).

A guest that fails to LINK aborts the whole `cargo test` process exactly as a
guest that fails to COMPILE does (no `test result` line is printed, every
other test in the batch is lost), so the `*_nolink` cases must be run one at a
time with `-- --ignored --exact <full::path>`. ONE exception, re-measured
2026-09-17: `core_select_nth_nolink` is not a link failure at all — its guest
builds and the recursion is caught by the ASSEMBLER (`found a cycle in the
call graph`, surfacing as a panic at tests/support/src/compiler_test.rs:1024),
so it fails as an ordinary test and can be batched. Every `memcmp` case still
exits the process. All nine `memcmp` boundaries above were re-run one at a time
on 2026-09-17 and none moved beyond the already-recorded `core_eq_reach` shift
to the default level.

### Probing the compile matrix without the harness (campaign 27 recipe)

A guest-side link failure or a compiler panic can be classified in ~5 s
instead of a harness run, because the guest half is just cargo:

```
CARGO_ENCODED_RUSTFLAGS=$'-C\x1ftarget-feature=+bulk-memory,+wide-arithmetic\x1f--cfg\x1fmiden\x1f-C\x1flink-args=--fatal-warnings\x1f-Zlocation-detail=none\x1f-Zunstable-options\x1f-Cpanic=immediate-abort' \
cargo build -Z build-std=core,alloc,panic_abort -Z build-std-features=optimize_for_size \
  --config profile.release.opt-level=<2|"z"|3|1> --config profile.release.lto=true \
  --config profile.release.codegen-units=1 --release --target wasm32-wasip1 ...
target/debug/midenc <the.wasm> --release [--optimize=size-min|max|basic]
```

The `--optimize` level maps to the cargo opt level the compiler passes:
default = 2, size-min = "z", max = 3, basic = 1 (`cargo_profile_opt_level`).
Validated against known results (the `prog_threefish` `-Oz` and `prog_sha512`
default panics reproduce at their exact sites, as does the assembler's
call-graph-cycle error). It does NOT execute the VM, so value checks still
need the harness.

### Arbitrating a divergence: wasmtime on the harness-built wasm

Campaign 27 found two divergences with the SAME wasm op (`i64.mul_wide_s`)
pointing in opposite directions, and only `wasmtime -W wide-arithmetic=y
--invoke entrypoint target/miden_test_shared/wasm32-wasip1/release/
differential_<case>.wasm a b` separated them:

- `str::parse::<i64>` of a runtime-length digit slice: native 9 = wasmtime 9,
  MASM 0 → the wasm is right, `midenc` is wrong (a compiler miscompile;
  `core_parse_i64`, reproduces at all four levels). The same shape with `u64`
  or `i32`, and the plain `(a as i128) * (b as i128)` widening multiply, all
  agree — the bug is specific to the signed 64-bit `from_str` accumulation.
- `i64::checked_mul(10)` in a loop (the same `from_str` fast-path shape by
  hand): native 90, wasmtime 0xdead = MASM 0xdead → the WASM is wrong, i.e.
  the F9 guest-toolchain family, and the smallest producer of it in the
  corpus (`core_chkmul_i64`).

Never attribute a divergence involving `+wide-arithmetic` ops without this
arbitration: "it goes through `mul_wide`" is not enough to call it F9.

### Compile-time reach of the realistic `core` programs (campaign 27)

Ten programs over the surface above, each value-checked on the 1225-pair
native grid (corpus: `tests/corelib.rs`, `prog_*`):

- Three of the ten do not compile at some level, and each fails at a
  DIFFERENT one: a slice-algorithm program panics only at
  `--optimize=basic`, a derived-`Ord` priority table only at
  `--optimize=max`, and a `core::num` measurement pipeline at the default
  level, at max and at basic (it compiles only at `-Oz`) — all three at the
  F6 sites (`frontier.rs:123`, `lowering.rs:109`). The "opt level is not a
  safety ladder" rule from campaign 21 holds for ordinary `core` code with no
  hand-written rotation constants at all.
- For the `core::num` pipeline the lever is total live pressure in the loop
  body, not any one API: removing EITHER the 64-bit lane OR the byte-order
  round trips makes the default level compile, and removing both makes all
  four levels compile (ladder in `case_prog_numeric_guard.rs`). Halving the
  trip count changes nothing.

## Minimal reproducers per class: the corpus map (campaign 28, re-verified campaign 31)

Which committed tests a fix for each known compile-time class must turn green,
and which value-checked siblings must stay green. One filter per class:
`cargo test -p midenc-integration-tests <filter> -- --ignored`. Every entry was
classified by TRACE, not by crash site (the three-signature rule above), and
every entry was re-run alone on 2026-09-17 (campaign 31). **F18 is CLOSED**;
F2, F6, F7, F8, F11, F12, F13, F15 and F17 are open with the reproducers
below.

- **F6, stale dominator tree** (`spill::rewrite_cfg_spills` rebuilds SSA form
  from the `DominanceInfo` the spill ANALYSIS cached before the transform's own
  edge splits; signature: `edges to split > 0` plus `erase unused reload`
  lines). Must turn green: `pressure::window_erased_min` (24 lines, four u64
  values live across a bottom-tested loop plus one crossing band ->
  `emit/mod.rs:623`, all four opt levels), `pressure::overflow_cluster_min`
  (23 lines, five values and no band -> `lowering.rs:109` over a 17-felt
  stack, all four levels), `pressure::zero_trip_frontier` and
  `pressure::frontier_seq` / `frontier_dispatch` (-> `frontier.rs:123`),
  `pressure::zero_trip_overflow`, `wide::wide_limbs_freight_oz` (campaign 31:
  four u128 accumulators over a loop at `-Oz`; the erased reload and the
  failing spill store are the same value), and the realistic members
  `programs_oz::prog_sha512` and the F6 `programs::prog_*` group.
  `corelib::prog_slicealg_basic` LEFT the class on 2026-09-17 — it compiles
  with nightly-2026-09-01 guests and still panics with 04-30 ones, so it is a
  guard now, not a reproducer. Must stay green:
  `pressure::window_erased_guard` (+ `_edges`), `pressure::zero_trip_guard`
  (+ `_repro`), `pressure::while_results`, and the `interact::*` guards that
  carry erased split reloads (2-32 of them) while computing the right answer —
  a fix must keep those answers, not just stop erasing.
- **F17, spill placement past the window** (`emit/mod.rs:623` with
  `additional spills required`, `edges to split = 0`, no erased reloads; the
  drop trace's last op is the spill `hir.store_local`). Must turn green:
  `opt_levels::spill_store_min` (60-line ARX kernel at -Oz) and
  `programs_oz::prog_threefish_oz` (the only realistic member). Must stay green:
  `opt_levels::spill_store_guard` (one rotation row fewer) and the rest of
  `opt_levels` at -Oz.
- **F2, arity-2 solver gap** (in-window <= 16-felt stack, `[Move, Copy]` /
  `[Copy, Move]`). Must turn green: `spills::rotl_window`,
  `spills::spill_loop_mix_oz`, `programs_oz::prog_threefish_o3`. Must stay
  green: `pressure::chain_window` (eighteen counts, one rung below the gap),
  `pressure::unary_window`, `pressure::width_mix`.
- **F11, the spill analysis is blind to `hir.exec_indirect`'s arguments**
  (it reads operand group 0 only — hir-analysis/src/analyses/spills.rs — while
  `hir.exec_indirect` keeps the table index in group 0 and the ARGUMENTS in
  group 1, so dispatch arguments are spilled, never reloaded, and the call is
  budgeted as one felt). Must turn green: `calls::indirect_spill_bb`. The three
  original reproducers (`calls::indirect_spill`, `_line`, `_args`) became
  GUARDS on 2026-09-17: nightly-2026-09-01 LLVM devirtualizes a plain read of
  a `static [fn(..); N]`, so their wasm has ZERO `call_indirect` and they never
  reach `hir.exec_indirect`. The class is re-armed by reading the table through
  `core::hint::black_box(&TABLE)[i]`, which is what `indirect_spill_bb` does —
  it reproduces the original `lowering.rs:109` `arith.bxor` `[Move, Copy]`
  panic exactly. Rule: any future fn-pointer-dispatch case must read its table
  through `black_box`, or it measures nothing.
- **F12, `RemoveLoopInvariantArgsFromBeforeBlock` aliasing** (pattern driver's
  last `trying to match` line). Must turn green: `compose::invariant_args_min`
  (the `return` producer), `programs_oz::prog_blake2b`,
  `programs_oz::prog_xxh64_o1`, `programs::prog_varint` /
  `prog_varint_guard_oz` / `prog_varint_wa_oz_nodwarf` / `prog_rle` /
  `prog_rkscan_guard_nodwarf` / `prog_rkscan_ref_nodwarf`,
  `corelib::prog_numeric_nodwarf` and `compose::chain_sm_nodwarf`. Must stay
  green: `compose::invariant_args_guard`,
  `compose::invariant_args_noreturn_guard`.
  TWO producers STOPPED producing on 2026-09-17 with the nightly-2026-09-01
  guests and are guards now, not reproducers: `compose::nest_continue` and
  `compose::invariant_args_noreturn` (the return-free minimal case). Both still
  panic verbatim with nightly-2026-04-30 guests, so this is LLVM no longer
  leaving cfg-to-scf a nested loop with a merged exit for those two sources —
  the pattern is unchanged. The minimal reproducer of the class is now
  `compose::invariant_args_min` alone.
- **F18, coercion folders mutate the operand constant's attribute — CLOSED
  2026-09-17 by ef358e356** (the folders now allocate a fresh immediate per
  result instead of calling `set_from_immediate_lossy` on the attribute reached
  through `foldable_operand_of_trait`). The three reproducers are un-ignored
  and guard it: `wide::sext_const_shared` (8-line loop-free),
  `wide::parse_i64_hand`, `corelib::core_parse_i64` — the last one needs the
  nightly-2026-09-01 guests as well, because its checked path also carried the
  F9 defect. Also green: `wide::sext_const_split`, `wide::zext_const_shared`,
  `wide::trunc_const_shared`, `wide::parse_i64_hand11`, `parse_i64_short`,
  `corelib::core_parse_u64`. Reach measured 2026-09-10: only the SIGNED folder
  has a plain-Rust producer — `I64MulWideS` sign-extends the wasm operand
  directly, while `I64MulWideU` bitcasts to `u64` first (so `Zext::fold`
  mutates the bitcast's materialised constant, not the shared one) and every
  64-bit shift/rotate count that `mask_movement_count` truncates to `u32` ends
  up as a distinct attribute from the `i64` constant a plain use holds. A
  wrong-width push is invisible in every IR dump; read the MASM.

## Guest toolchain nightly-2026-09-01 (rebase of 2026-09-17)

The branch was rebased onto a `next` that bumped the guest toolchain from
nightly-2026-04-30 to nightly-2026-09-01 (and merged the compiler-audit
fixes). Every fallout below was arbitrated by rebuilding the guests with the
old toolchain (`RUSTUP_TOOLCHAIN=nightly-2026-04-30 <test binary> <filter>
--exact`, which the harness's nested `cargo` invocations honour): a case that
passes that way is a toolchain-shape shift, not a compiler regression.

- **LLVM now devirtualizes constant fn-pointer tables.** A runtime index into
  a `static TABLE: [fn(..) -> ..; N]` becomes a switch of direct calls (the
  wasm loses every `call_indirect`), so funcref-table recursion turns into a
  direct call-graph cycle the assembler rejects (`recursion_indirect`,
  `recursion_wide`, `deep_frames`, `deep_overrun` failed; `rec_freight`,
  `rec_mutual`, `rec_slots`, `recursion_frames` happened to survive). Read the
  table through `core::hint::black_box(&TABLE)[i]` to keep the dispatch
  indirect; every recursion case now does.
- **`memcmp` reach moved down a level:** the constant-size array `==` of
  `core_eq_reach` is an outlined `memcmp` libcall at the default level now
  (it was only at `-Oz` before), so the case is F13-ignored at O2 too. Expect
  other per-level `memcmp` boundaries in the link-reach map to have moved;
  re-probe before quoting one.
- **A failed guest build exits the test process.** `midenc-compile`
  (`rust.rs`, since 2026-05) calls `std::process::exit(cargo status)` when
  the guest `cargo build` fails, so a link failure (`undefined symbol:
  memcmp`) or any rustc error in ONE case kills the whole `cargo test`
  invocation with no summary — "error: test failed" and `Broken pipe` lines
  from the other in-flight case builds are the tell. Never leave a
  link-failing case un-ignored, and run suspected non-linking cases alone.
- **Two realistic programs moved into F6 at the default level:**
  `corelib::prog_ordkeys` (8 split edges, then `frontier.rs:123` unwraps
  `None`) and `programs::prog_iters` (`emit/mod.rs:623`, 2 split edges, 6
  erased reloads). Both compile with nightly-2026-04-30 guests.
- **F16 is no longer silent for `deep_overrun`:** the wrapped shadow-stack
  address now lands where the VM's u32 range assertion fires ("operation
  expected u32 values, but got values: [4295098224]") instead of an unused
  element region; still no stack-overflow diagnostic, and wasmtime still
  traps out of bounds.
- **The `+wide-arithmetic` miscompile (F9) is FIXED by this toolchain.** All
  sixteen F9 reproducers pass (campaign 31, 2026-09-17), and the evidence is
  wasmtime, not the test result: `wasmtime run -W wide-arithmetic=y --invoke
  entrypoint differential_<case>.wasm a b` now returns the NATIVE value on
  every pinned pair where it used to return the MASM one, and the wide op is
  still in each wasm (`i64.mul_wide_s` / `mul_wide_u` / `add128` / `sub128`),
  so the shape survives and only the stale read is gone. Consequences: the
  DWARF-masked guards (`signed::sext_shapes` + `_repro`, `wide::wide_words` +
  `_edges`, `wide::wide_loop_cmp` + `_edges`) no longer depend on the
  harness's `debug = 2` and pass at `FUZZA_GUEST_DEBUG=0`; rows that were
  pinned OUT of a case because of F9 can go back in (`wide::parse_i64_hand11`
  got its sixteen-digit row back); and a divergence through a wide op is no
  longer presumptively the toolchain's — arbitrate every one with wasmtime.
  NOTE for reading wasmtime output: `--invoke` parses arguments as i32, so a
  `u32` above `i32::MAX` must be passed in its signed form.
- Fixes that landed with the rebase and matter to the ledger: coercion
  folders allocate a fresh immediate per result (F18 — CLOSED, see the corpus
  map), `i64.rem_s` has a dedicated lowering (`i64_srem` un-ignored upstream),
  heap-growth overflow handling, sparse-lattice meet, anchor hash collisions,
  structural region equivalence in CSE, `switch_shapes` / `sext_shapes`
  un-ignored upstream.

## Trap parity (campaign 29, 2026-09-17)

The first campaign to check the trap oracle — "Miden traps if and only if
native Rust panics, and returns the same value otherwise" — via
`run_case_traps` / `run_case_traps_with_inputs` (`tests/traps.rs`). 25 cases
covering array and slice bounds, slice-range and argument panics, `/` and `%`
by zero and at `MIN / -1` on all four widths, `checked_*`/`TryFrom`/
`from_utf8`/`from_u32`/`NonZeroU32`/`Result` unwraps, the assertion macros,
`br_table` arms, `#[inline(never)]` frames and `call_indirect` dispatch. Each
case has an `_edges` twin pinning the boundary grid.

**Every Rust panic family has trap parity, at every configuration.** 54 tests
pass (1 ignored, the W7 probe below) in the default env and under
`--optimize=max`, `--optimize=size-min`, `--optimize=basic` and
`FUZZA_GUEST_DEBUG=0`, and at `FUZZA_INPUT_PAIRS=256` in the default env. No
divergence in either direction.

- **One MASM op behind every Rust panic.** Guests build with
  `-Cpanic=immediate-abort` (`MANDATORY_RUST_FLAGS`,
  midenc-compile/src/pipeline/frontends/rust.rs:1800), so a panic never
  reaches the `#[panic_handler]` at all on the wasm side: rustc emits a wasm
  `unreachable`, which midenc lowers to `push.0
  assert.err="entered unreachable code"`. Bounds check, slice range, `/0`,
  `MIN / -1`, `unwrap`, `assert!`, `unreachable!()`, `todo!()` — all the same
  op and the same VM error text. The harness's `TRAPPING_CASE_HEADER` wasm
  arm is belt-and-braces; only its host arm (`_exit(101)`) is load-bearing.
- **Rust's own guards run before the VM's.** For 64-bit division LLVM emits
  the zero-divisor and `MIN / -1` tests as explicit `i32.eqz`/`i64.ne` +
  `br_if` to `unreachable` BEFORE the `i64.div_u`/`i64.div_s`, so
  `::miden::core::math::u64::div` and `::intrinsics::i64::checked_div` never
  see a zero divisor from safe Rust. The intrinsics' own assertions
  (codegen/masm/intrinsics/i64.masm:217 documents "traps if `b == 0` or the
  result overflows") are a second line of defence with no plain-Rust
  producer. `i64 %` came out as `a - (a/b)*b`, so the new dedicated
  `i64.rem_s` lowering is not on this path either.
- **The trap's POSITION is not preserved, only the decision.** In the MASM
  the trap is sunk past the value computation (`… push.0 movup.2 eq if.true
  push.0 assert …` at the end of the function), and LLVM merges sibling
  guards into one `unreachable` block upstream of that. Do not write a case
  that tries to observe work done before a trap — nothing before a trap is
  observable through this harness, and the oracle is exactly the trap-or-value
  decision.
- **Release guests: only the `checked_`/explicit forms panic.** Overflow
  checks are off, so `a + 1` at `u32::MAX`, `x << 40`, `i32::MIN.abs()` and
  `wrapping_div(MIN, -1)` all return; `debug_assert!`/`debug_assert_eq!` are
  compiled out and must not trap. What panics is the language-mandated set:
  indexing, slice ranges, `/` and `%` by zero, `MIN / -1` AND `MIN % -1` for
  `i32`/`i64` (the division guards are emitted regardless of
  `-C overflow-checks`), `unwrap`/`expect`, and the `assert!` family.
- **Statically-present, dynamically-dead panics stay dead.** A `panic!`, a
  zero divisor and an out-of-range index all guarded by the cross-modulus
  contradiction `h % 6 == 5 && h % 3 == 0` trap on neither target at any
  optimization level (`trap_dead_guard`).
- **`usize::try_from(u64)` is a FALSE divergence source** and must never
  appear in a case: `usize` is 64 bits natively and 32 bits on wasm, so the
  conversion panics on the guest and returns on the host by target, not by
  compiler. (Same family as the campaign-13 `rodata_big` index-wrap trap.)
- **A trapping edge does not move the compile-time class boundary.** A
  five-deep loop nest with an accumulator escaping each level stops compiling
  at `--optimize=max` (`failed to schedule operands … for inst 'arith.rotl'
  with error: NoSolution, constraints: [Move, Copy]` over a full 16-entry
  window at codegen/masm/src/lower/lowering.rs:109 — the F2 arity-2 solver
  gap, `spills::rotl_window`), but a sibling with the index masked into range
  and therefore no trapping edge at all panics identically. The nest alone is
  what reaches F2; `trap_deep_nest` is kept one level shallower. Do not file
  a trap-edge variant of a known compile-time class without building that
  no-trap sibling first.
- **W7: Miden does not bounds-check linear memory.** A `read_volatile` at
  256 MiB past the guest's 17-page memory returns zero on Miden
  (`masm value 5`) while the host segfaults (`native: signal 11`) and
  wasmtime traps ("memory fault at wasm address 0x100ffff0 in linear memory
  of size 0x110000 / wasm trap: out of bounds memory access"). Kept as the
  `#[ignore]`d `traps::trap_oob_read` probe — the same missing enforcement
  `frames::deep_overrun` hits from the other side, where a wrapped
  shadow-stack address only trips the VM's u32 range assertion by accident.
- Tooling: `scratch/c29run.sh <case> grid` is the native half of the oracle
  in ~2 s (trap/value map over the 35x35 boundary grid, plus a repeat-stability
  check). It must strip the case's `extern "C"` ABI first — since Rust 1.81 a
  panic crossing an `extern "C"` boundary aborts instead of unwinding, so
  `catch_unwind` would never see it. Use it before every harness run: it
  caught three trap-density design errors and two collapsed value functions
  in this campaign.

## Memory-effect ordering (campaign 30, 2026-09-17)

Whether any middle-end pass can merge, drop, move or fold a load or a store
it must not. Corpus: `tests/memorder.rs` (14 cases + pinned grids), every case
native-grid-checked (1225 boundary pairs) before the harness run and swept at
`FUZZA_GUEST_DEBUG=0`, `--optimize=max`, `--optimize=size-min` and
`--optimize=basic`.

- **The declared-effect table is complete and conservative.** Every op in
  `dialects/{hir,arith,scf,cf,ub,wasm}` that implements
  `MemoryEffectOpInterface` was audited against what it actually touches, and
  nothing reachable from plain Rust understates its effects:
  `hir.load`/`hir.load_local` Read, `hir.store`/`hir.store_local` Write,
  `hir.mem_cpy` Read(source)+Write(destination) and `hir.mem_set`
  Write(destination) (that is how they PRINT; the structs are `MemCpy` /
  `MemSet`), `hir.mem_grow` Read+Write, `hir.mem_size` Read,
  `hir.spill` Write / `hir.reload` Read, the four `hir.assert*` ops and
  `ub.unreachable` Write (so nothing is moved across a trap guard),
  `hir.local_address` and `ub.poison` effect-free (address/constant
  materialization only), and the five signed sub-word wasm loads
  (`wasm.i32_load_8s`, `wasm.i32_load_16s`, `wasm.i64_load_8s`,
  `wasm.i64_load_16s`, `wasm.i64_load_32s`) Read on their address operand.
  `hir.exec`, `hir.exec_indirect`, `hir.call` and `hir.syscall` do NOT
  implement the interface at all, which is the CONSERVATIVE state: CSE treats
  an op with no interface as a write (cse.rs:488-493) and
  `is_memory_effect_free()` returns false for it (operation.rs:1264), so a
  call is never sunk and never merged across. SDK-only ops (`hir.exec_fpi`,
  `hir.mem_stream`, the event/advice/crypto family) have no plain-Rust
  producer.
- **`#[derive(EffectOpInterface)]` with no `#[effects]` means effect-FREE, not
  unknown** (`hir-macros/src/operations/effects.rs`: an empty effect map is
  filled with an empty `MemoryEffect` group, so `has_no_effect()` is true).
  That is why the audit has to read the FIELD-level `#[effects]` attributes
  too — the wasm signed sub-word loads declare theirs on the `addr` operand,
  and a `grep -B6 '#\[effects('` that only looks above the struct misses them.
- **CSE never merges a heap load; it only merges `hir.load_local`s.** Measured
  over four W1 cases with `-Z print-ir-after-pass=cse`: `hir.load` and
  `hir.store` counts are identical before and after the pass in every case
  (25/25, 12/12, 10/10, 22/22), while 2 to 7 `hir.load_local`s are merged per
  case — same-block reloads of one wasm local with no store between them.
  Every opaque-write kind blocks the merge for the documented reason: an
  `#[inline(never)]` helper is a `hir.exec` (unknown effects), `black_box(&mut
  _)` / `write_volatile` / an atomic RMW is a `hir.store`, a runtime-range
  bulk op is `hir.mem_cpy`/`hir.mem_set`. Caveat for designing such a probe:
  LLVM guards every runtime-length bulk op with its own `len != 0` branch, so
  a "load; bulk write; load" shape does NOT stay in one block (27 blocks in
  `memorder::cse_bulk`) and CSE's same-block requirement rules out the merge
  before the effect check is reached. Use a CALL or a scalar store as the
  opaque write when the effect model is what you are testing.
- **SCCP and SinkOperandDefs are no-ops on memory.** On a `static`-heavy
  function (an immutable `.rodata` table read at a constant index beside its
  written `static mut` twin) the
  `sparse-conditional-constant-propagation` dump is byte-identical to its
  input: 27 `hir.load`s in, 27 out. A `static` is never constant-folded
  through a load — there is no memory model in the pass, and no `Foldable`
  impl for `hir.load`/`hir.store`. `sink-operand-defs` likewise moves nothing
  in a store/load-forwarding case (18 loads / 21 stores, unchanged).
- **No value divergence anywhere in the area.** Loads across five kinds of
  opaque write at six widths; store-then-load forwarding at every byte lane
  and every width of a 4-aligned buffer (u32 store then u8/i8 lane, four lane
  stores then the word, a u16 store at byte offset 0..3 then both touched
  words, an unaligned u32 load one byte after a u32 store); two raw-pointer
  views plus `align_to_mut::<u32>()`; `swap`/`replace`/`take`/`ptr::swap` at
  possibly-equal runtime indexes; program order inside one expression with and
  without the campaign-20 freight; `copy_from_slice`/`copy_nonoverlapping` at
  all sixteen `(src % 4, dst % 4)` combinations and lengths 0..=17;
  unaligned `fill`/`write_bytes`; and `static`/`static mut`/atomic reads
  around in-place writes — all agree with native, in the default env and at
  all four sweep configurations.
- **The memcpy element fast path rejects overlap in BOTH directions.** The
  `mem_overlap` / `copy_same_pos` ignore texts describe `dst > src` and
  `dst == src`; a FORWARD-overlapping `copy_within` (`dst < src`) whose byte
  count, source and destination are all 4-aligned aborts in the same
  `miden-core-lib memcopy_elements` assert ("source and destination ranges
  must not overlap", mem.masm:100) even though a forward copy would be correct
  for that direction. The byte fallback loop copies upward, so a forward
  overlap with a non-multiple-of-4 byte count agrees with native at every
  overlap distance 1..=8 (`memorder::copy_fwd`). Rule for new cases: an
  overlapping `copy_within` is only safe when `count % 4 != 0` (or the offsets
  differ mod 4), whatever the direction.
- **Freight does not reorder memory.** The `seq_expr` ordering shapes carrying
  an eight-u64 cluster and three rotate bands spill 77 values / 74 reloads /
  one split edge / seven erased split reloads (vs 38 / 44 / 0 / 2 without the
  freight) and still compute the native answer: the spill transform places
  slots around the loads and stores rather than through them.
- **Without guest DWARF the LOCAL traffic shrinks and the heap traffic does
  not.** `local2reg` on a lane-forwarding case logs the SAME twelve "found
  promotable local" lines at `debug = 2` and `debug = 0`; four of them are
  then blocked by "debug declarations cannot all be converted safely" at
  `debug = 2` only. After the pass: 20 `store_local` / 85 `load_local` with
  DWARF vs 17 / 82 without, and 18 `hir.load` / 21 `hir.store` in both. The
  whole module passes at `FUZZA_GUEST_DEBUG=0`.

## Fix uptake (campaign 31, 2026-09-17)

The whole `#[ignore]` ledger — 83 tests — re-run one per cargo invocation
against the rebased `next` and the nightly-2026-09-01 guests, every verdict
arbitrated rather than taken at face value. 25 pass now; the ignore count is
83 → 60. The arbitration is the durable part:

- **A pass is not a fix.** Three different causes produced the 25 passes, and
  only two of them are fixes. Always separate them before un-ignoring:
  * *compiler fix* — the case also passes with `RUSTUP_TOOLCHAIN=
    nightly-2026-04-30 <test binary> --ignored --exact <path>` (F18);
  * *guest-toolchain fix* — the wasm changed but the SHAPE survived: wasmtime
    on the harness-built wasm now returns the native value and the op is still
    in the wasm (F9);
  * *shape loss* — the case no longer produces the IR the bug needs; the old
    toolchain still reproduces the panic verbatim. Six cases were this, and
    un-ignoring them without saying so would have silently deleted two classes
    from the ledger.
- **CLOSED: F18** (ef358e356) and **F9** (the nightly-2026-09-01 LLVM). Their
  guards are `wide::{sext_const_shared, parse_i64_hand}` +
  `corelib::core_parse_i64`, and the sixteen former F9 reproducers across
  `signed`, `wide`, `calls` and `corelib`.
- **Still open, with the reproducers moved**: F11's three reproducers became
  guards (devirtualization) and the class is re-armed by
  `calls::indirect_spill_bb`; F12 lost two producers at the default level
  (`compose::nest_continue`, `compose::invariant_args_noreturn`) and keeps
  `compose::invariant_args_min` as its minimal one; F6 lost
  `corelib::prog_slicealg_basic` and gained `wide::wide_limbs_freight_oz`.
  F2, F7, F8, F13, F15, F17 and the `memory` overlap findings are unchanged,
  same sites.
- **A class can hide behind an optimization level.** Un-ignoring on the
  default level alone is not enough: `compose::nest_continue` still panics
  (F12) at `--optimize=size-min` and `--optimize=basic`, and
  `calls::indirect_spill_args` / `_line` still panic (F11) at
  `--optimize=basic`, because `-O1` does not devirtualize the fn-pointer
  table. Sweep every un-ignored case over the four configurations before
  calling its class closed.
- **The C21 "7 of 12" realistic programs did not move**, and neither did any
  `memcmp` boundary in the link-reach map. The rebase's fixes did not touch
  the freight cliff.
- **Re-climbing the two closed ladders**: ten shared coercion constants across
  a loop (`wide::coerce_const_bands`) reach no boundary at all, while FOUR
  u128 accumulators live across a loop with three rotate bands
  (`wide::wide_limbs_freight`) stop at `--optimize=size-min` in F6 — the
  freight that bounds wide arithmetic is live 64-bit VALUES, not shared
  constants. In that reproducer the erased split reload and the operand of the
  failing spill store are the SAME value (`%55`), which is the cleanest F6-vs-F17
  discrimination in the corpus: the two traces name one value, so the
  classification does not rest on the precedence rule.

## Operation equivalence and CSE (campaign 32, 2026-09-17)

What upstream fab7b7db0 ("midenc-hir: compare region operations
structurally") actually changed for plain-Rust guests, and what it did not.
Corpus: `tests/cse.rs` (12 cases + pinned grids), each evidenced with
`-Z print-ir-after-pass=<pass>` plus
`MIDENC_TRACE='pass:<pass>=trace,rewriter=trace'`.

- **The structural REGION comparison stays unreachable, post-rebase.** Measured
  on a function with three byte-identical `if`s (`cse::twin_if`): the
  `entrypoint` body CSE sees carries `cf.br` 6, `cf.cond_br` 3, `hir.exec` 6,
  `hir.load_local` 21, `hir.store_local` 11 and the usual `arith.*` — and ZERO
  `scf.*`. The `after` dump equals the `before` one. The `scf.if`s appear first
  in the `lift-control-flow` dump (`scf.if` 3, `scf.yield` 6) and the post-lift
  `canonicalizer`, `sink-operand-defs` and `transform-spills` dumps all still
  show 3. CSE runs six passes before lifting, so `is_equivalent_with_mapping`'s
  region arm has no producer here; what the commit changed on this path is the
  commutative-operand multiset alone.
- **The `Commutative` list and its plain-Rust reach.** The trait is on `Add`,
  `AddOverflowing`, `Mul`, `MulOverflowing`, `And`, `Or`, `Xor`, `Band`, `Bor`,
  `Bxor`, `Eq`, `Neq`, `Min`, `Max` (dialects/arith/src/ops/binary.rs).
  Reachable from a `(u32, u32) -> u32` guest: `add`, `mul`, `band`, `bor`,
  `bxor`, `eq`, `neq`. NOT reachable: `and`/`or`/`xor` (the i1 logical forms —
  the frontend maps `I32And`/`I32Or`/`I32Xor` to `band`/`bor`/`bxor` and never
  builds them) and `min`/`max` (wasm has no i32 min/max operator, the frontend
  never calls `builder.min`/`max`, and `core::cmp::min`/`max` lower to a
  compare plus a select). `Sub`, `Shl`, `Shr`, `Div`, `Mod`, `Lt`, `Lte`, `Gt`,
  `Gte` are not marked, and none of them merged with swapped operands in any
  case here. Three more arith ops turned out to have no plain-Rust producer at
  all, which is worth recording next to the routing facts: `arith.ashr`
  (`i32.shr_s` reaches HIR as `arith.shr` on a signed operand type),
  `arith.sdiv` (`I32DivS` calls `builder.div`) and `arith.smod` (`I32RemS`
  goes through the `wasm.i32_rem_s` expansion).
- **LLVM closes every straight-line escape hatch; only volatile reads open
  one.** EarlyCSE/GVN merge `x + y` with `y + x` whenever both are visible on
  the same SSA values under dominance, and they intersect the IR flags rather
  than giving up on them. Measured in the guest wasm: source order written
  both ways, `wrapping_add` beside `unchecked_add`, and a value `black_box`
  between the copies ALL came out as one `i32.add`. The `black_box` attempt
  fails twice over — on wasm it is a shadow-stack store plus load, so its
  result is a different HIR value too. Pointer/GEP arithmetic (the
  "SelectionDAG builds the address add per block" idea) also produced nothing:
  InstCombine turns `inttoptr(add(shl i, 2), ptrtoint p)` back into the GEP and
  the two loads merge.
  The hatch that works: **two volatile reads of one address**. They are
  distinct LLVM values, so LLVM keeps both commutative ops; the wasm stack
  order follows the unreorderable volatile load order, so issuing the second
  pair in the opposite order puts the swapped operands into HIR; and HIR has no
  volatility, so CSE merges the reloads and only then can the multiset key
  match.
- **Byte accesses are the only heap loads CSE can merge.** `prepare_addr`
  (dialects/wasm/src/mem.rs) calls `enforce_alignment` only when
  `memarg.align > 0`, and that is what emits the `divmod` + `hir.assertz`.
  `hir.assertz` declares Write and has no folder, so for every access of 2
  bytes or more the second load's own alignment check sits between the two
  loads and `has_other_side_effecting_op_in_between` stops there. This is the
  MECHANISM behind campaign 30's "CSE never merges a heap load": it is true for
  every width except 1 byte. With `u8` reads the merge is routine —
  `hir.load` 4 -> 2 in every `cse::comm_*` helper.
- **Two more barriers that silently kill such a shape.** (1) `hir.store_local`
  is a Write, so any wasm `local.set`/`local.tee` between the reloads blocks
  the merge. LLVM inserts one whenever a value has to outlive an intervening
  computation — which is why `bool << k` (lowered through a `cf.select` of two
  constants) and `x * x` (needs a `local.tee` to duplicate the value) both
  defeated the shape until the cases were rewritten to a bare `wrapping_sub`
  combiner and to per-operand loads. (2) CSE's memory-read candidates require
  `existing.parent() == op.parent()`, so a reload in a DOMINATED block never
  merges with one in the dominator — the whole hatch is single-block only.
- **The merges that fire, and the ones that must not.** `arith.add`,
  `arith.mul`, `arith.band`, `arith.bor`, `arith.bxor`, `arith.eq` and
  `arith.neq` each merge with their swapped twin (op count -1 per pair, with
  the `replaced op with Some(%N): arith.<op>` line). An op over {a, a} does not
  merge with one over {a, b}; `a * b` does not merge with `c * a` when `c` is a
  different SSA value holding the same byte; an `arith.eq` does not merge with
  an `arith.neq` over the same pair. The nested case works too:
  `(a+b)*(c^d)` and `(d^c)*(b+a)` merge all three ops in ONE pass run, because
  `simplify_block` rewrites operands in place and the later op's key is
  computed afterwards. No divergence in any configuration.
- **`a + a` never reaches HIR as an add with two equal operands** — LLVM
  rewrites it to `a << 1`. Use `*` for that corner. And the POSITIVE half of
  the {a, a} corner (two `a * a` merging with each other) has no producer at
  all, for the `local.tee` reason above.
- **Without guest DWARF nothing about this changes.** The `debug = 0` CSE dump
  of the hatch case is op-for-op identical to the `debug = 2` one (`hir.load`
  4 -> 2, `arith.add` 4 -> 2, `arith.mul` 2 -> 1), and the whole module passes
  at `FUZZA_GUEST_DEBUG=0`, `--optimize=max`, `--optimize=size-min` and
  `--optimize=basic`.
- **The post-lift erase path is reachable and ordered.** `Rewriter::erase_op`'s
  `erase_tree` is reached by the post-lift canonicalizer whenever
  `convert-trivial-if-to-select` replaces an `scf.if`: the trace prints
  `erased op scf.yield`, `erased ^block<N>`, `erased op scf.yield`,
  `erased ^block<M>`, then `erased op scf.if` — nested ops before their blocks,
  blocks in post-order, the region op last. The `while-remove-unused-args` /
  `index-switch-remove-unused-results` cascade is NOT a test of that order: the
  pattern MOVES the body ops into the replacement first, so the `scf.while` it
  erases has empty regions by then.
- **`MIDENC_TRACE='rewriter=trace'` can crash the compiler.** On a guest whose
  post-lift form reaches `IfRemoveUnusedResults`, enabling the rewriter trace
  panics at `hir/src/program_point.rs:486:63` with `AliasingViolationError
  { kind: Immutable, location: dialects/scf/src/canonicalization/
  if_remove_unused_results.rs:86:32 }` — the `TracingRewriterListener` borrows
  an operation the pattern already holds mutably. The same compile with no
  trace, or with `pass:canonicalizer=trace` and
  `-Z print-ir-after-pass=canonicalizer` but no `rewriter=trace`, exits 0.
  Reproducer: `cse::dead_region`. Take erase-order evidence on a shape that
  traces cleanly (`cse::dead_outer`) until this is fixed.

## SCCP reach and lattice joins (campaign 33, 2026-09-17)

What sparse conditional constant propagation (`hir-transform/src/sccp.rs`) can
still see on plain-Rust guests after the rebase's two dataflow fixes, and
therefore what its lattice join is ever asked to decide. Corpus:
`tests/sccp.rs` (10 cases + 8 pinned grids), each evidenced with
`-Z print-ir-after-pass=<pass>` plus
`MIDENC_TRACE='pass:<pass>=trace,rewriter=trace'`.

- **The sparse `meet` has no caller on ANY compile, so b7aed6ca1 ("apply meet
  in sparse lattice guards", guard.rs:365) is dead code.** `grep -rn
  'SparseBackwardDataFlowAnalysis for' --include='*.rs' .` returns nothing: the
  trait (hir-analysis/src/sparse/backward.rs:23) has zero implementors
  workspace-wide, and the only calls to the sparse guard's `meet` are the five
  in that file (lines 181, 244, 309, 360, 413), all inside functions generic
  over an implementor. The DENSE backward meet (guard.rs:345) is a different
  method and was already right; its one user is `Liveness`
  (hir-analysis/src/analyses/liveness.rs:261) inside TransformSpills. The three
  solver loaders in the workspace are SCCP's (`DeadCodeAnalysis` +
  `SparseConstantPropagation`, both FORWARD), `LivenessAnalysis`'s (those two
  plus `Liveness`), and the advice-taint family's — and the latter runs only
  under `-Zlint` (midenc-compile/src/pipeline/backend.rs:167), which the
  harness never passes.
- **Anchor identity is now decided by value, not by hash.**
  `LatticeAnchorRef::intern` (hir-analysis/src/anchor.rs:39) keys the intern map
  by `anchor_id()` (an FxHash of `dyn_hash`) but stores a `SmallVec` BUCKET per
  key, and picks an existing entry only when `anchor.equivalent_to(existing)` —
  `dyn_eq` against the canonical value, which every `LatticeAnchorExt` impl
  (anchor.rs:334-425) takes from the borrowed IR entity. A hash collision now
  costs a bucket scan instead of sharing lattice state. 13210e157 is a real fix;
  it just cannot be aimed at deterministically from Rust source.
- **SCCP never sees a block argument in a plain-Rust guest, so its lattice join
  is never exercised at a merge.** Two independent measurements:
  (1) across all 1304 harness-built `differential_*.wasm`, the only block-result
  construct LLVM's wasm backend emits is `loop (result i32)` — 108 occurrences
  in 106 files, and ZERO `block (result …)` / `if (result …)`. Every `if`, every
  `match` and every loop-carried value merges through a wasm LOCAL, which is
  `hir.store_local`/`hir.load_local` traffic until Local2Reg, two passes after
  SCCP. (2) The block arguments the frontend *does* build — `translate_loop`'s
  exit block and the synthetic function-exit block — are gone before SCCP runs:
  `sccp::loop_result` enters the FIRST canonicalizer with 16 blocks and four
  block arguments and leaves with none. Two of the four are empty,
  predecessor-less blocks (the `loop (result i32)` exit is unreachable because
  LLVM only types it to satisfy the function signature); the rest are merged
  away by `simplify-br-to-block-with-single-predecessor`, which logs
  `merging ^blockN into ^blockM replacing uses of its block arguments`.
  Measured on 22 cases in all — the ten below plus `canon::col_cascade`,
  `memory::slice_ops`, `programs::prog_sorts`, `programs::prog_utf8` (at
  `-Oz`), `compose::chain_sm`, `compose::nest_continue` and six of
  `control_flow` (`switch_shapes`, `triangle`, `do_while`, `sm_bits`,
  `switch_loop_mix`, `threading`) — every one shows zero block arguments in its
  SCCP dump. The other would-be carrier is just as empty: every `cf.switch` in
  these bodies is printed with bare successors
  (`cf.switch %76 [#builtin.u32<0> -> ^block19, …], ^block20 : (u32)`), and
  every `cf.cond_br` / `cf.br` likewise — no successor operands anywhere,
  because `br`/`br_if`/`br_table` can only carry values to a block that has
  params, and no wasm block here has any.
- **Which `loop (result i32)` shapes exist.** A loop whose every exit is a
  `return` and whose `end` is therefore unreachable, sitting last in the
  function. The producer is several `break`s falling into ONE tail expression
  that is also the function's result: LLVM tail-duplicates the tail into a
  `return` inside the loop. A loop with a genuine fall-through tail gets rotated
  into a plain `loop` instead — that is the difference between the two drafts of
  `sccp::loop_result`.
- **SCCP is a no-op on every one of the ten cases.** The `entrypoint` op
  histogram is identical before and after the pass in all ten (55/84/78/187/80/
  132/120/132/65/93 ops in and out), no op is folded or erased, and the constant
  count is unchanged: the pass re-uniques each function's existing
  `arith.constant`s one-for-one (8 to 20 per case). This holds for the two
  const-operand cases built specifically for W2 — literal rotate/shift counts,
  `/` and `%` by literals, unsigned compares against literals, and their 64-bit
  twins with the frontend's own truncated counts — so the canonicalizer's folder
  reaches everything first.
- **The constant-uniquing key IS type-aware in practice.** `OperationFolder`'s
  `UniquedConstant { dialect, value, ty }` (hir/src/folder.rs:355) keeps
  same-value/different-type constants apart, and that is visible in the
  post-SCCP dumps rather than only in the source: `sccp::const_ops` carries
  `5 : i32` beside `5 : u32`, `16 : i32` beside `16 : u32`, `12 : i32` beside
  `12 : u32` and `8 : i32` beside `8 : u32`; `sccp::const_wide` carries
  `3 : u32` beside `3 : u64`. No F18-shaped wrong-width materialization from
  this path.
- **The dead-code half has no plain-Rust producer, and the reason is
  structural.** A constant SCCP could see must live in the same SSA graph LLVM
  optimized, so LLVM has already used it; hiding it from LLVM (`black_box`, an
  opaque helper, a `static`) hides it from SCCP too. Measured both ways:
  `sccp::dead_flag`'s `phi(1, 1)` is folded by InstSimplify and its dependent
  `if` is DELETED before the wasm (the tail is unconditional, no trace of the
  dead arm), while `sccp::dead_arm`'s `black_box`ed selector keeps the
  `br_table 1 2 3 4 0` default arm alive for LLVM *and* leaves it unprovable for
  SCCP (the selector is a `hir.load` of a shadow-stack slot).
- **No-DWARF changes nothing about what SCCP sees.** At `debug = 0` the guest
  wasm of `sccp::if_merge`, `sccp::loop_result` and `sccp::switch_merge` has the
  same block-result count (0/1/0) and the same `local.set`/`local.get` count
  (12/29/26) as at `debug = 2`; only the `di.*` ops leave the HIR. The idea that
  more stackification without debug info would turn local-carried phis into
  block results is REFUTED for these shapes. The whole module passes at
  `FUZZA_GUEST_DEBUG=0`, `--optimize=max`, `--optimize=size-min`,
  `--optimize=basic` and `FUZZA_INPUT_PAIRS=256`, with no divergence anywhere.

## Heap programs over a case-local allocator (campaign 34, 2026-09-17)

The corpus was allocation-free until this campaign. `tests/heap.rs` runs 23
`alloc` programs — `Vec`, `Box`, `Rc`/`RefCell`, a `Box`-linked list,
`Box<dyn Trait>`, `VecDeque`, `BinaryHeap`, `BTreeMap`/`BTreeSet`, sorts,
iterator `collect` pipelines, `String`, `core::fmt`, allocator OOM and
`memory.grow` — each over a ~20-line bump allocator the case owns, so the
wasm guest and the native `cdylib` run the SAME allocator code and no SDK
crate is involved.

### The allocator recipe and its three rules

`cargo-miden` builds guests with `-Z build-std=core,alloc,panic_abort`
(midenc-compile/src/pipeline/frontends/rust.rs:832), so `extern crate alloc`
needs no harness change; the native `cdylib` takes `alloc` from the host
sysroot.

```rust
const ARENA_SIZE: usize = 1 << 16;
#[repr(align(16))]                              // rule 3
struct Arena(UnsafeCell<[u8; ARENA_SIZE]>);
unsafe impl Sync for Arena {}
static ARENA: Arena = Arena(UnsafeCell::new([0; ARENA_SIZE]));
static mut NEXT: usize = 0;
struct Bump;                                    // alloc = bump, dealloc = no-op
#[global_allocator] static GLOBAL: Bump = Bump;
```

1. **RESET the allocator at entry** (`unsafe { NEXT = 0 }` as the first
   statement of `entrypoint`). The native `cdylib` stays loaded across every
   input pair of a run while the VM starts fresh each time, so without the
   reset the arena leaks on the native side only. Under `run_case` that does
   not even fail cleanly: `CASE_HEADER`'s panic handler is `loop {}`, so a
   native OOM HANGS the test. Verified sound at `FUZZA_INPUT_PAIRS=256`
   (`heap_vecsum`, 256 pairs x up to 63 pushes, 14.6 s).
2. **Results must be ADDRESS-INDEPENDENT**: content hashes and `len() as u32`
   only — never a pointer, never `capacity()`, never a `usize` payload
   (`usize` is 64 bits natively and 32 on wasm). The `usize` trap bites index
   arithmetic too: `(input2 as usize).wrapping_add(9) % len` wraps on wasm
   only and produced a false divergence in `heap_deque_edges` at
   (0, 4294967295) before the index was rebuilt in `u32`.
3. **The arena needs `#[repr(align(16))]`**: a `[u8; N]` static is only
   byte-aligned, while the loads LLVM emits for `u32`/`u64` elements carry
   `align=4`/`align=8` memargs.

`handle_alloc_error` needs no `alloc_error_handler`: the stable default
handler panics, which is a wasm `unreachable` on the guest
(`-Cpanic=immediate-abort`) and the case header's `_exit(101)` on the host,
so allocator exhaustion has trap parity under `run_case_traps`
(`heap_oom`, `heap_oom_edges`).

### The `alloc` row of the link-reach map (default level unless stated)

Everything below LINKS (no `memcmp`), and all of it assembles except one
entry:

- `Vec` (push/extend/reserve/insert/remove/drain/retain/dedup/resize/
  truncate/swap_remove) at element sizes 1, 3, 4, 8 and padded `(u8, u32)`;
  `Box`, `Box<[T; N]>`, `Option<Box<T>>`, `Vec<Box<T>>`, `Box<dyn Trait>`
  (vtable dispatch survives as `call_indirect` only when the collection is
  read through `black_box` — the campaign-31 devirtualization rule applies
  to vtables too); `Rc` + `RefCell` including `strong_count` and
  `try_unwrap`; `VecDeque`; `BinaryHeap` + `into_sorted_vec`; `BTreeMap` /
  `BTreeSet` including `range`; `String` (`push`/`push_str`/`truncate`/
  `pop`/`from_utf8`/`chars`/`char_indices`/`bytes`); `alloc::format!`,
  `to_string`, `write!` through `core::fmt::Write`, `{:?}` on a derived
  `Debug`, radix/width/padding specs, and `str::parse::<u32>()` back out of
  a runtime-length slice; `collect::<Vec<_>>()` from `map`/`filter`/`chain`/
  `zip`/`rev`/`take`/`skip`/`step_by` plus `fold`/`max`/`min`/`position`.
- **`core`'s STABLE sorts do NOT assemble**: `Vec::sort`, `sort_by` and
  `sort_by_key` reach `core::slice::sort::stable::tiny::mergesort`, which
  calls itself, and the assembler fails with `found a cycle in the call
  graph` at ALL FOUR optimization levels (probed one level at a time). This
  is the `core_select_nth_nolink` family, not a `memcmp` link failure, so it
  does not take the `cargo test` process down. `sort_unstable`,
  `sort_unstable_by` and `sort_unstable_by_key` are fine.
  Replacement that works: a hand-written BOTTOM-UP merge sort over a scratch
  `Vec` (`heap_sort`) — an allocating stable sort is not what is
  unsupported, the recursion is.
- **A recursive DROP GLUE is an opt-level cliff.** A `Box`-linked list
  dropped iteratively (`Option::take` loop) assembles at the default level,
  `--optimize=max` and `--optimize=size-min`, but at `--optimize=basic`
  LLVM keeps `core::ptr::drop_glue::<Node>`, which calls itself through the
  `Option<Box<Node>>` link, and the assembler rejects it
  (`heap_list` vs `heap_list_basic`).

### The two `core` libraries are NOT the same library

The guest's `core` is built by `-Z build-std` with `optimize_for_size`; the
native `cdylib`'s `core` is the host sysroot's. Where an API's result is
UNSPECIFIED, the two can legitimately disagree, and the harness reports that
as a divergence. Verified instance: the guest wasm contains only
`core::slice::sort::unstable::heapsort::heapsort`, while the native `.so`
contains `core::slice::sort::unstable::ipnsort` with `median3_rec`,
`small_sort_network` and `insertion_sort_shift_left`. An unstable sort does
not specify the order of EQUAL keys, so `sort_unstable_by_key(|e| e >> 40)`
over elements whose payload outlives the key diverged at
(983633457, 2147483648) with no compiler involvement.
**Rule for any case using an unstable sort**: the key must be INJECTIVE over
the element (`e.rotate_left(17)` is a bijection) or the element must BE the
key (`sort_unstable()`), so ties are indistinguishable.

### Bulk-op lowering facts for `alloc` code

- Every bulk move `alloc` performs is a wasm `memory.copy`; the guests
  contain ZERO `memcpy`/`memmove`/`memset` libcalls and zero `memory.fill`
  (measured on all 21 case wasms). Counts per case run 0 (`heap_list`, a
  linked list moves one node at a time) to 84 (`heap_btree`).
- The frontend represents each one as
  `hir.mem_cpy %dst, %src, %count : (ptr<u8, byte>, ptr<u8, byte>, u32)` —
  BYTE-typed pointers and a RUNTIME count — so codegen always emits the
  runtime 4-alignment test with `::miden::core::mem::memcopy_elements` on
  one arm and the byte fallback loop on the other. The choice is made at
  execution time, per call.
- **`alloc`'s containers are full of OVERLAPPING copies, and both memcpy
  arms get them wrong.** Campaign 30 recorded the element path's
  direction-independent overlap assert and that the byte loop copies upward;
  campaign 34 found the plain-Rust producers and the second, SILENT symptom:

  | shape | operands | arm | outcome |
  |---|---|---|---|
  | `Vec<u32>::insert` (dst > src) | 4-aligned | element | VM abort |
  | `Vec<u32>::remove` / `drain` (dst < src) | 4-aligned | element | VM abort |
  | `BTreeMap`/`BTreeSet` insert+remove | 4-aligned | element | VM abort |
  | `Vec<u8>::insert`, `String::insert` (dst > src) | 1 byte apart | byte loop | SILENT wrong value |
  | `Vec<u8>::remove` (dst < src) | 1 byte apart | byte loop | correct |

  The byte-loop row is new: the fallback arm has no overlap assert at all,
  so it corrupts data with no diagnostic. Both wrong rows were arbitrated
  with wasmtime on the harness-built wasm (wasmtime == native), so the
  compiler is at fault, not the guest toolchain. Corpus:
  `heap::heap_vec_shift`, `heap_vec_drain`, `heap_vec_shift_u8`,
  `heap_string_insert`, `heap_btree` (+ `_repro` twins), bounded by the
  passing `heap_vec_remove_u8`, `heap_vec_edit`, `heap_string`,
  `heap_bheap` (a container that reorders by SWAPS, not shifts).
  Practical consequence: `Vec::insert`/`remove`/`drain`, `String::insert`
  and ALL of `BTreeMap`/`BTreeSet` are unusable on Miden today — three
  inserts into one B-tree leaf is enough to abort.

### `memory.grow`'s heap model

`intrinsics/mem.masm` models a DYNAMIC heap that starts at ZERO pages, based
at the first page boundary past all static memory and function tables
(`codegen/masm/src/linker.rs`, e.g. 0x120000), and capped by
`HEAP_END = (2^30 - 1) * 4` bytes. `memory_grow(n)` returns the PREVIOUS page
count, or `-1` when `size + n` overflows `u32`, when `heap_base + new_size *
64KiB` overflows 32 bits, or when that top passes `HEAP_END`; on failure the
metadata is untouched. The ceiling is therefore about 65500 pages.

`memory.size` on Miden counts the DYNAMIC heap, while a wasm engine counts
the whole linear memory, so absolute page counts differ by target and a case
may only observe what both models agree on: success flags and size DELTAS.
`heap_grow` pins exactly that (a 1..5-page growth raises the size by exactly
that many pages; a 2^20-page request returns `usize::MAX` and leaves the size
unchanged; a zero-page growth changes nothing) and passes at all four
optimization levels and without DWARF.

Not covered by a test, read out of mem.masm + linker.rs: the standard wasm
allocator idiom `let base = memory_grow(0, k) * 65536` computes a base of 0
under this model on the first growth, which is the start of the DATA
SEGMENTS, not of the new pages — a plain-Rust allocator that grows memory the
usual way would write over its own statics. There is no way for a plain-Rust
guest to learn the dynamic heap base (the SDK gets it from the `heap_base`
intrinsic), so this could not be turned into a differential case.
