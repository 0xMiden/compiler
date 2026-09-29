# What the compiler does with plain Rust, stage by stage

One verified fact per bullet, with its proof: a differential test
(`module::test`) or a source path and function. Panic sites are quoted as the
ignore texts quote them; only `frontier.rs:123` and `stack.rs:80` were
re-checked against the current tree. A `Dead end:` bullet or sentence is a
shape or lever verified to produce nothing; do not retry it. Open classes and
their reproducers are the tagged `#[ignore]` attributes (`KNOWLEDGE.md`,
"Corpus map on demand").

## Guest toolchain

- Guests build with nightly-2026-09-01, `-Z build-std=core,alloc,panic_abort`,
  `build-std-features=optimize_for_size`, LTO, cgu=1 and target features
  `+bulk-memory,+wide-arithmetic`, NO `+multivalue`
  (`midenc-compile/src/pipeline/frontends/rust.rs`, `MANDATORY_RUST_FLAGS`).
  Tuple/struct/array/u128 returns are sret pointers, u128 parameters two i64
  (`calls::ret_area`, `calls::sret_shapes`). Dead end: multi-value returns
  and block parameters.
- `-Cpanic=immediate-abort`: every panic is a wasm `unreachable`, lowered to
  `push.0 assert` "entered unreachable code"; `#[panic_handler]` never runs on
  wasm (`control_flow::trap_branch`). No overflow checks, no `debug_assert!`:
  what panics is indexing, slice ranges, `/` and `%` by zero, `MIN / -1` and
  `MIN % -1` on i32/i64, `unwrap`, `assert!` (`traps::trap_div_zero`,
  `traps::trap_asserts`). LLVM branches to `unreachable` before every
  division, so the MASM division intrinsics never see those operands
  (`traps::trap_div_wide`).
- LLVM devirtualizes constant fn-pointer tables (a runtime index into a
  `static [fn; N]` becomes a switch of direct calls). `call_indirect`
  survives only for `black_box(&TABLE)[i]` reads, runtime-indexed
  `[&dyn Trait; N]` and fn pointers crossing `#[inline(never)]` boundaries
  (`calls::call_indirect`, `calls::indirect_chain`, `calls::dyn_trait`); at
  `--optimize=basic` a plain table read stays indirect
  (`calls::indirect_spill_args`).
- The previous LLVM's `+wide-arithmetic` stale-local-read miscompile is gone:
  the wide ops are still in the wasm and wasmtime returns the native value
  (`wide::wide_words`, `wide::sat_add_u128`, `corelib::core_chkmul_i64`).
- Guest `core` is not host `core`: the guest's `sort_unstable` is `heapsort`,
  the host's `ipnsort`, so unspecified results (order of equal keys) differ
  (`heap::heap_sort`). `usize` is 32-bit on the guest, 64-bit natively
  (`heap::heap_deque_edges`, `traps::trap_try_from`).
- The harness builds guests with `debug = 2`, `cargo miden build` with none;
  `debug = 1` acts like `debug = 0` on every known boundary
  (`compose::chain_sm_nodwarf`, `debug_info::l2r_params`). rustc emits every
  wasm-local value location as `[DW_OP_WASM_local, DW_OP_stack_value]`,
  `DW_OP_consts` only for a negative i64 initializer, and salvaged arithmetic
  the decoder drops except `DW_OP_plus_uconst` (`debug_info::dbg_negconst`,
  `debug_info::dbg_salvage`, `debug_info::dbg_byval`).
- LLVM pre-cleaning to route around: const-const arithmetic folds;
  known-bits-provable values fold; `x & 7` bounds peel, `% 97` survives;
  everything inlines without `#[inline(never)]`; tail recursion becomes a
  loop; identical arm tails merge; identical-incoming phis fold
  (`sccp::dead_flag`); EarlyCSE/GVN merge `x + y` with `y + x` across
  `black_box` and flag differences (`cse::comm_arith`); `a + a` becomes
  `a << 1`; pure helpers, even `#[inline(never)]`, sink to their use
  (`spills::spill_split`); `x / d` beside `x % d` fuses to mul-sub
  (`boundaries::udiv_bounds`).
- `<=`/`>=` in branch or select position become strict compares with swapped
  arms; `le_s`/`ge_s`/`ge_u` need a bool materialized in an
  `#[inline(never)]` helper (`signed::scmp_bool`, `wide::ucmp_ge`).
- Constant divisors stay `div`/`rem` with an immediate; only unsigned
  power-of-two divisors become shifts, and no multiply-high form exists
  (`arith::div_const_forms`).
- u128 compares, bitwise ops, clz/ctz and popcount become i64 limb ops; u128
  `/`, `%` and dynamic shifts become compiler-builtins functions in the guest
  (`wide::u128_cmp`, `wide::u128_bits`, `wide::u128_udiv`,
  `wide::u128_shifts`, `wide::i128_ashr`).
- Checked, saturating and overflowing arithmetic never reaches wasm (op +
  compare + select, `mul_wide` + high-word select, explicit branches)
  (`wide::u64_sat_forms`, `arith::ovf_mul`).
- The wasm backend never emits `if`/`else` or result-typed `block`/`if`; the
  only block result is `loop (result i32)` for a loop whose every exit is a
  `return`; returns are per-site; `br_table` is rebased to 0
  (`sccp::loop_result`, `sccp::if_merge`). A u64 `match` is a `br_table` on a
  wrapped half (`control_flow::sm_wide`); a bit-driven state machine is
  jump-threaded into nested loops (`control_flow::sm_bits`).
- LLVM merges all trapping edges of a function into ONE `unreachable`, and the
  MASM trap is sunk past the value computation: only the trap-or-value
  decision is observable (`trapspill::both_kinds`, `traps::trap_loop_late`).
- Max fully unrolls constant-trip inner loops in rolled outer loops
  (`programs::fir_cordic_o3`). Size-min keeps 4-8-trip loops and small
  multi-site helpers, and turns 48+-byte constant copies into `memory.copy`
  (`opt_levels::loop_keep_oz`, `opt_levels::helper_calls_oz`,
  `opt_levels::mem_libcalls_oz`).
- Dead end: -Oz as a coverage source; it opens no new compiler function
  (`opt_levels::loop_keep_oz`).

## Frontend routing (Rust → wasm → HIR)

- Rust `as` casts become `trunc`/`zext`/`sext`/`bitcast`, never `hir.cast`;
  `wasm.SignExtend` is `trunc` + `sext` (`signed::sext_widths`). Unsigned
  translators bitcast U32/U64 to I32/I64 around every op; U8/U16/U32-typed
  values come only from widening loads (`memory::loadwiden`). Dead end:
  `hir.cast` / `OpEmitter::cast`.
- Division: `arith.div` → `checked_div`, `arith.mod` → `checked_mod`,
  `wasm.i32_rem_s` → `wrapping_mod`, `i64.rem_s` has its own lowering
  (`signed::i64_srem`); `arith.divmod` comes only from `prepare_addr`
  (always U32); nothing emits an unchecked division.
- `i64.mul_wide_s` sign-extends its wasm operands directly, `mul_wide_u`
  bitcasts to u64 first; a constant multiplicand is the only constant-operand
  `sext`/`zext`, and each widening multiply gets its own extension pair
  (`wide::sext_const_shared`, `wide::zext_const_shared`, `signed::mulwide_dyn`).
  Dead end: a multi-use 4-felt operand from widening multiplies.
- Every shift and rotate count is wrapped in `arith.band(trunc(count),
  width - 1)` (`mask_movement_count`, `frontend/wasm/src/code_translator/mod.rs`):
  the count band of the spill section.
- Never built by the wasm frontend: `min`/`max`, `neg`/`not`/`incr`, checked
  and overflowing ops, `clo`/`cto`, `ilog2`/`pow2`/`exp`/`is_odd`/`inv`,
  `ext2*`, `sdiv`/`smod`/`sdivmod`, `ashr` (`i32.shr_s` is `arith.shr` on a
  signed type), `bnot` (`!x` is `bxor x, -1`), the i1 `and`/`or`/`xor`,
  `hir.cast`, `builtin.ret_imm`, and `Call`/`Syscall`/`ExecFpi` (SDK only)
  (`frontend/wasm/src/code_translator/mod.rs`; `cse::comm_bits`).
- The locals argument: LLVM keeps values on the wasm stack only within one
  block, so every cross-block or multi-use value is a wasm local, i.e.
  `hir.store_local`/`hir.load_local`. HIR joins get no block parameters, and
  the block arguments the frontend builds (`translate_loop`'s exit, the
  function exit) are gone after the first canonicalizer
  (`sccp::loop_result`, `sccp::switch_merge`).
- Every parameter gets an unconditional `hir.store_local` at entry
  (`declare_parameters`, `frontend/wasm/src/module/func_translator.rs`); an
  unused parameter of a kept function is a dead store (`memory::local_shapes`).
- Every pointer goes through `prepare_addr` (`dialects/wasm/src/mem.rs`), which
  emits the `divmod` + `hir.assertz` alignment check only when
  `memarg.align > 0`; no constant-address canonicalization exists. 128-bit
  memory traffic is i64 pairs; I128 loads/stores come only from spill and sret
  slots (`memory::straddle_wide`, `memory::packed2_fields`).
- `memory.copy` becomes `hir.mem_cpy %src, %dst, %count` with byte-typed
  pointers and a runtime count, `memory.fill` becomes `hir.mem_set`; `alloc`
  code has no `memcpy`/`memmove`/`memset` libcalls (`heap::heap_btree`,
  `memory::copy_ladder`).
- Dead end: the data-segment insert/overlap arms; wasm-ld emits sorted
  disjoint segments (`memory::segment_mix`).
- Declared memory effects are complete and conservative: `hir.load`/
  `load_local` Read, `hir.store`/`store_local` Write, `hir.mem_cpy`
  Read(src)+Write(dst), `hir.mem_set` Write, `hir.mem_grow` Read+Write,
  `hir.mem_size` Read, `hir.assert*` and `ub.unreachable` Write, the signed
  sub-word wasm loads Read on `addr`. `hir.exec`/`exec_indirect`/`call`/
  `syscall` implement no effect interface and count as writes
  (`memorder::cse_reload`, `memorder::cse_atomic`). A derive with no
  `#[effects]` means effect-FREE (`hir-macros/src/operations/effects.rs`).
- wasm-ld emits one funcref table (slot 0 null, no holes). It lowers lazily
  to `builtin.function_table` (root word + tag word per slot, after the
  globals), and `call_indirect` to `hir.exec_indirect` = bounds check + tag
  check + `dynexec`; tag = interned signature index + 1 (`signature_type_tag`)
  (`calls::call_indirect`, `calls::indirect_sigs`, `calls::dyn_trait`,
  `calls::fnptr_value`, `calls::indirect_collision`). At most 15 argument
  felts; one more is a clean diagnostic (`calls::indirect_wide`). Dead end:
  funcref tables beyond one contiguous table (PIC base, holes, multi-table,
  intrinsic entries).
- Recursion compiles only through a `black_box`ed table: the assembler's
  cycle check sees direct `exec` edges only (`calls::recursion_indirect`,
  `frames::rec_mutual`).

## Middle end: canonicalizer, CSE, SCCP, Local2Reg

- Fixed pipeline at every level, on a function pass manager
  (`midenc-compile/src/pipeline/backend.rs`): Canonicalizer → CSE → SCCP →
  SinkOperandDefs → Local2Reg → TransformSpills → LiftControlFlowToSCF →
  Canonicalizer → SinkOperandDefs → TransformSpills. `ControlFlowSink` and
  DCE are commented out; there is no middle-end knob: `--optimize` sets only
  the guest LLVM level (`cargo_profile_opt_level`,
  `midenc-compile/src/pipeline/frontends/rust.rs`). Dead end: middle-end
  knobs (`Aggressive` simplification, `ControlFlowSink`, DCE, per-pass flags)
  and the advice-taint and postdominance analyses need a source change or
  `-Zlint`.
- CSE and SCCP see only pre-lift bodies, which hold zero region ops
  (`cse::twin_if`). CSE merges same-block `hir.load_local`s with no store
  between (the strongest Copy-constraint lever), and heap loads only 1 byte
  wide: for 2+ bytes the second load's alignment `hir.assertz` (a Write) sits
  between them (`memorder::cse_widths`, `cse::reload_write`). Dead end: CSE
  region equivalence and non-dominance paths.
- Commutative add, mul, band, bor, bxor, eq, neq merge with their swapped
  twin, nested trees in one pass; {a, a} vs {a, b} and eq vs neq do not
  (`cse::comm_arith`, `cse::comm_cmp`, `cse::nested_swap`, `cse::multiset`,
  `cse::noncomm_arith`). Swapped operands reach HIR only via two volatile reads
  of one address issued in opposite order (`cse::comm_bits`).
- Dead end: a positive `{a, a}` merge needs a `local.tee` per square
  (`cse::multiset`); commutative twins split with `black_box` or pointer math
  are re-merged by LLVM (`cse::comm_arith`); a load, bulk write and load in
  one block are split by LLVM's `len != 0` guard (`memorder::cse_bulk`).
- SCCP is a no-op on plain-Rust guests: identical op histograms before and
  after, it only re-uniques existing `arith.constant`s, never sees a block
  argument or successor operand, and never folds a `static` through a load
  (`sccp::if_merge`, `sccp::nested_merge`, `memorder::static_write`). The
  folder keys constants by (dialect, value, type) (`UniquedConstant`,
  `hir/src/folder.rs`; `sccp::const_ops`, `sccp::const_wide`).
- Dead end: SCCP folding or dead-arm deletion (`sccp::dead_flag`,
  `sccp::dead_arm`); `OperationFolder::try_fold` and the sparse `meet` have no
  caller (`hir/src/folder.rs`, `hir-analysis/src/sparse/backward.rs`);
  removing DWARF does not turn local merges into block results
  (`sccp::if_merge`).
- Canonicalization producers (default level unless noted):
  `SimplifySwitchFallbackOverlap` fires once per switch however many arms
  merge (`canon::arms_merge`); `SimplifyCondBrLikeSwitch` needs a
  two-successor `cf.switch`, i.e. trap or impossible arms
  (`canon::trap_dispatch`); `SimplifyPassthroughCondBr` fires ten times per
  two-level frame when an in-loop `return` follows the `break`
  (`canon::passthru_frame`, `opt_levels::deadfall_oz`); `SimplifyPassthroughBr`
  on a loop-carried bool condition (`control_flow::do_while`);
  `WhileRemoveDuplicatedResults` on a three-level triangle nest
  (`control_flow::triangle`); `WhileUnusedResult` →
  `IndexSwitchRemoveUnusedResults` once per loop whose only branch is an EMPTY
  `continue` (`canon::col_cascade`); `IfRemoveUnusedResults` on a three-level
  diamond whose deepest arm alone consumes crossing bands
  (`interact::sink_spill`); `WhileRemoveUnusedArgs` K times for K
  early-`break` scans with band traffic (`interact::scan_spill`);
  `RemoveUnusedSinglePredBlockArgs` in trap/`br_table` shapes
  (`control_flow::switch_trap_arm`); `FoldRedundantYields`,
  `ConvertTrivialIfToSelect` and `SplitCriticalEdges` routinely
  (`control_flow::sm16`, `control_flow::wide_exits`);
  `RemoveLoopInvariantArgsFromBeforeBlock` panics on every match (#1419).
  `SplitCriticalEdges` shares the greedy fixpoint, so a critical-edge guard
  can become satisfiable mid-run.
- Dead end: `CanonicalizeI64RotateBy32ToSwap` (the count band hides the 32,
  `mask_movement_count`); `SimplifyBrToReturn` (claimed first by
  `SimplifyBrToBlockWithSinglePred`, `control_flow::cf_shapes`);
  `WhileConditionTruth` (LLVM folds in-body reads of the condition,
  `control_flow::do_while`); `FoldConstantIndexSwitch` and `cf.Select::fold`
  (selectors are never constant post-lift, `control_flow::switch_forms`).
- Dead end: pattern variants beyond the producers above:
  `WhileRemoveDuplicatedResults` fires only on the three-level triangle
  (`control_flow::triangle`), the column cascade needs an EMPTY `continue`
  (`canon::col_cascade`), duplicate `match` arms do not reach
  `SimplifyCondBrLikeSwitch` (`control_flow::sm16`), and six return sites stop
  `SimplifyPassthroughCondBr` (`canon::passthru_frame`).
- `RemoveUnusedSinglePredBlockArgs` reads `successors()[0]` for both
  destinations, so else-successor arguments are never removed
  (`dialects/cf/src/canonicalization/simplify_successor_arguments.rs`).
- Crossing freight does not change how often a pattern fires, except that it
  creates the `IfRemoveUnusedResults` and default-level `WhileRemoveUnusedArgs`
  producers above (`interact::cascade_spill`, `interact::passthru_spill`). A
  trapping arm adds firings of `ConvertTrivialIfToSelect` and
  `SimplifyCondBrLikeSwitch`; a guard inside what a pattern is about removes
  one (`trapspill::select_arm`, `trapspill::dispatch_arm`,
  `trapspill::cascade_cont`).
- `Rewriter::erase_op` erases nested ops, then blocks in post-order, the region
  op last (`cse::dead_outer`). `SinkOperandDefs` never moves a
  `hir.load_local` (Read), so it cannot sink a spill reload
  (`interact::sink_spill`).
- Local2Reg (`dialects/hir/src/transforms/local2reg.rs`) promotes a slot only
  with exactly one load and one store in one block and no branch or call
  between; entry-block parameters are the population (`debug_info::l2r_params`,
  `debug_info::l2r_livecall`). Full DWARF blocks most conversions
  (`convert_debug_references_for_local`), not the candidates: 4 promoted vs 27
  over the `l2r` cases of `debug_info`. Dead-store erasure is not debug-gated.
- Dead end: Local2Reg declare conversion, poison arm and `is_declaration`:
  no single-op `[WasmLocal]` location, no read-before-write local, no imports
  (`debug_info::dbg_byval`).

## Spills, the operand window and the scheduler

- The window is 16 felts. It bounds flat signatures, the sret pointer
  included (16 compiles: `calls::call_sigs16`, `calls::wide_calls`; 17
  panics: `calls::sig17`), `call_indirect` arguments (15), and helper state
  passed by value (pass `&[u64; N]` instead: `programs::prog_rle_wa`).
- The only plain-Rust carrier of SSA values across blocks is the count band:
  the folder dedups constants function-wide and CSE merges identical `band`s
  into the dominating one, so each distinct constant shift/rotate count reused
  in several blocks is ONE u32 felt live between them. LLVM makes such shifts
  itself (`x * 7` → `x << 3`, address math). User values cross only as
  locals, reloaded per block (`spills::spill_split`, `compose::calls_all`).
- The analysis counts LIVE felts: up to about nine bands alone request no
  spill. A u64 cluster makes it spill: 8 bands + an 8-value cluster across two
  loops gives 57 spills, 84 reloads, 6 split edges (`interact::cascade_spill`).
  Under freight a trapping edge is worth about two felts and can decide a
  rung (`trapspill::guard_above` vs `trapspill::guard_above_masked`); the
  guard's kind is invisible (`trapspill::body_index`, `trapspill::body_assert`).
- Edge splits come from asymmetric pressure across a join
  (`spills::spill_split`) and from over-capacity loop headers (16+ bands used
  before the loop and on its loop-carried accumulator,
  `spills::spill_loop_mix`); symmetric pressure never splits
  (`spills::spill_twin`).
- `rewrite_cfg_spills` (`hir-transform/src/spill.rs`) rebuilds SSA form from
  the dominator tree cached BEFORE the transform split edges, so split-edge
  reloads are erased and spilled values stay on the stack.
  `DominanceFrontier::new` fills frontiers only at joins with three or more
  predecessors (`i > 1`, `hir/src/ir/dominance/frontier.rs`), and
  `frontier.rs:123` unwraps `None` at such a join reached through a split edge
  (`pressure::frontier_seq`). Erased reloads and `unused phi` warnings also
  occur in correct programs (`interact::dispatch_spill`).
- `insert_required_phis` seeds every predecessor edge with the spilled value,
  so a join reachable around the definition leaves SSA-invalid IR; it always
  ends in a compile-time panic, never a silent miscompile
  (`control_flow::unroll_chain`, `spills::unroll_rotmix`).
- `TransformSpills` rewrites each reload into a `hir.load_local` of a spill
  slot (`convert reload to load`); post-lift dumps show no reload ops. Slots
  are procedure locals (`locaddr`), one set per activation, numbered above the
  user locals, disjoint from linear memory (FMP starts at element 2^31)
  (`frames::frame_spills`, `frames::call_clobber`, `frames::rec_slots`).
- The spill analysis reads operand group 0 only; `hir.exec_indirect` keeps its
  arguments in group 1, so they are spilled, never reloaded, and the call is
  budgeted as one felt (`hir-analysis/src/analyses/spills.rs`;
  `calls::indirect_spill_bb`). Dispatches under pressure pass when every
  argument is a plain local loaded right before the call
  (`calls::indirect_args`, `calls::dispatch_pressure`).
- The solver (`codegen/masm/src/opt/operands/`) never checks that expected
  operands are on the stack: SSA-invalid input surfaces as an arity-2
  `NoSolution` or an arity-3+ subtract overflow in `Stack::movdn`
  (`stack.rs:80`, `spills::unroll_rotmix`). Arity 1 skips the solver and
  arity 3+ copies deepest-first, so neither fails in-window
  (`pressure::unary_window`); arity 2 has only `TwoArgs`, and a Copy operand
  near the bottom of a full window needs index 16: a chain passes at 18 shared
  counts, fails at 20 (`pressure::chain_window`, `spills::rotl_window`).
- Dead end: solver interiors (`CopyAll`, `SwapAndMoveUp`, the first evict of
  `MoveDownAndSwap`, fuel exhaustion): operands stay adjacent to their op
  (`scale::chain300`). Terminator reloads, splits carrying successor
  arguments and pre-lift live-through are never reached (`spills::spill_loop`).
- At -Oz bands stay un-hoisted: with N bands dying in the loop and M live
  after it, N + M <= 11 compiles and M = 0 has no boundary
  (`opt_levels::band_guard_oz`, `opt_levels::drop_batch_oz`,
  `spills::spill_loop_mix_oz`). Local2Reg promotion does not move it.
- Band boundaries are not monotone: the empty-`continue` cascade compiles at
  2 to 9 and 12 to 14 bands and panics at 10, 11, 15, 16; a real cipher panics
  at 6, 9, 11, 12, 13 distinct constants and compiles at 4 and 8. The cause is
  spill placement, not the wasm (`interact::cascade_spill`,
  `programs_oz::prog_threefish_oz_guard`).
- Realistic programs make bands by construction; at the default level the
  number of DISTINCT constants is the lever (`programs::prog_sponge_guard`), at
  -Oz the band results live as operands across the loop, so array-resident
  state is safe (`programs_oz::prog_keccakf_oz`), scalar state is not
  (`programs_oz::prog_threefish_oz`).
- Wide arithmetic is bounded by live 64-bit values, not shared constants
  (`wide::coerce_const_bands`, `wide::wide_limbs_freight`); in a recursive
  frame the cluster is the expensive axis (`frames::rec_freight`).
- Freight-tolerant: in-loop wide dispatch, chains of early-`break` scans
  (`programs::prog_states`, `programs::prog_scanchain`). Fragile: return-heavy
  loops in an outer loop, asymmetric diamonds, two passes sharing constants,
  three-level nests whose deepest arm is the only consumer
  (`programs::prog_rkscan`, `programs::prog_histogram`, `programs::prog_tlv`).
- Workarounds with DWARF: `black_box` on every USE of the rotate/shift
  constants (`programs::prog_sponge_wa`, `programs::prog_feistel_wa`,
  `programs_oz::prog_threefish_oz_wa`), or moving the deepest arm's expression
  into an `#[inline(never)]` helper taking state by reference
  (`programs::prog_tlv_wa`, `programs::prog_rle_wa`). Without DWARF the same
  rescues can fall into #1419 (`programs::prog_rkscan_ref_nodwarf`), and two
  individually safe configurations can compose into a panic
  (`programs::prog_varint_wa_oz_nodwarf`). Opt level is not a safety ladder
  either way (`programs_oz::prog_sha512`, `programs_oz::prog_threefish_o3`).
- Dead end: user fixes that fail: fewer distinct rotation constants
  (non-monotone, `programs_oz::prog_threefish_oz_guard`), `[u64; N]` state,
  hand-written rotates, flattening or splitting the program for the frontier
  panic (`programs::prog_tlv_guard`), another `-O` (`programs_oz::prog_sha512`).
- Callee pressure is independent of caller pressure; wide by-value results
  cross calls in every layout (`calls::callee_pressure`, `compose::sret_exits`).

## Control-flow lifting (cfg-to-scf)

- Every payload column is a 1-felt u32 discriminator or a `ub.poison`
  placeholder; user state crosses regions in locals (`control_flow::nest8`,
  `control_flow::exit_values`). Dead end: multi-felt cfg-to-scf columns.
- Region-op result columns grow about two per nesting level; exit
  multiplicity does not matter (`control_flow::wide_exits`). The 16-felt
  region-exit budget is hit at depth 9 at O2 and depth 7 at -Oz and below O2
  (`control_flow::nest8`, `control_flow::deep_nest_overflow`,
  `opt_levels::nest8_oz`).
- cfg-to-scf creates one `ub.poison` per type per function and uses it to
  initialize every `scf.while` column, so
  `RemoveLoopInvariantArgsFromBeforeBlock` matches as soon as a column still
  carries poison at the `scf.condition`. Producers: a `return` leaving the
  function from inside a two-level nest (`compose::invariant_args_min`) and an
  inner counted loop's merged exit dispatch with no early exit
  (`programs_oz::prog_blake2b`). The same nests with a labeled
  `break`/`continue`, one loop, or an unrolled inner loop compile
  (`compose::invariant_args_guard`, `compose::invariant_args_noreturn_guard`,
  `compose::nest_continue_inline`). Ask "does cfg-to-scf still see a nested
  loop with a merged exit"; reduce by removing features.
- Without DWARF, Local2Reg promotions create more producers of that pattern
  (`compose::chain_sm_nodwarf`, `corelib::prog_numeric_nodwarf`).
- An in-loop trapping edge becomes one extra u32 exit column plus a top-level
  `cf.switch` to the block holding `ub.unreachable` (`trapspill::body_index`).
  A function has at most one `ub.unreachable`, so `combine_exit`
  (`ReturnLikeOpKey`, `hir-transform/src/cfg_to_scf/transform.rs`) never
  combines two (`trapspill::both_kinds`, `trapspill::helper_arg`). Exit kinds
  are exactly `builtin.ret` and `ub.unreachable` (`control_flow::ret_args`).
- Dead end: two `ub.unreachable`s for `combine_exit` (LLVM merges traps, a
  helper only moves the one), and one case per panic kind under freight (all
  kinds are the same `unreachable`: `trapspill::body_get`,
  `trapspill::body_div`).
- Scale is free below the switch-width cap: about 800 blocks, 64 arms, 16
  loop-carried variables, 12 nesting levels (`control_flow::blocks_max`,
  `scale::match64`, `control_flow::sm16`, `scale::deep_nest`). A 255-target
  `br_table` builds, 256 overflows the u8 successor-group index
  (`control_flow::switch255`, `control_flow::switch256`). Short-circuit order
  survives end to end (`control_flow::shortcircuit`).

## Codegen and the emitter

- `hir.mem_cpy` has memmove semantics: the ranges may overlap, the destination
  receives the values the source range held before the copy, and `count == 0`
  is a no-op. `OpEmitter::memcpy` (`codegen/masm/src/emit/mem.rs`) lowers it
  as a byte range of `count * size_of(pointee)` bytes whatever the pointee
  type is, for pointers in the byte address space only
  (`codegen::memory::mem_cpy`), and tests src, dst and the byte length for
  4-alignment at runtime. The element arm calls the compiler intrinsic
  `::intrinsics::mem::memmove_elements` (`codegen/masm/intrinsics/mem.masm`),
  which copies in descending address order when `write_ptr > read_ptr` and
  ascending otherwise (overlap up `heap::heap_vec_shift`, `memory::mem_overlap`,
  `heap::heap_btree`; down `heap::heap_vec_drain`; identical ranges
  `memory::copy_same_pos`; zero length `boundaries::memnoop_same`). The byte
  loop (`emit_memcpy_byte_loop`) follows the same rule, descending when
  `dst > src`, and serves every other copy (up `heap::heap_vec_shift_u8`,
  `heap::heap_string_insert`; down `heap::heap_vec_remove_u8`,
  `memorder::copy_fwd`; identical ranges `memory::copy_same_bytes`). No core-lib
  copy routine is called.
- `OpEmitter::memset` is a per-byte loop, about 25 cycles per byte
  (`memory::frame_1m`).
- Dead end: emitter arms with no producer. The `_imm` load/store and
  quad-word arms serve only unit tests and the `__stack_pointer` initializer;
  constant user addresses still go through `prepare_addr`
  (`memory::mem_globals`). Unchecked division, U64 `checked_divmod` and int32
  N-bit normalization have no frontend caller
  (`codegen/masm/src/emit/int32.rs`; `arith::div_const_forms`). Felt memory
  ops, `mem_stream` and `store_array` (a `todo!()`) have no producer
  (`codegen/masm/src/emit/mem.rs`; `memory::copy_ladder`).
- The emitter pushes a constant from its IMMEDIATE while IR dumps print the
  result type, so an attribute/type mismatch shows only in the MASM
  (`wide::parse_i64_hand`).
- Post-op drops pick their arm by unused vs used operand counts after a region
  op whose body kills stack values: interleave (`opt_levels::band_guard_oz`),
  all-unused batch (`opt_levels::drop_batch_oz`), solver
  (`opt_levels::drop_solver_oz`). The dead-result drop fires only at index 0,
  from the unused low half of `mul_wide` in a `mulhi`
  (`opt_levels::mulhi_dead_oz`). Dead end: block-entry interleave drops
  (entry liveness is uniform) and a dead HIGH half of `mul_wide` (LLVM emits a
  narrow `i64.mul`).
- The only `assertz` producer is `prepare_addr`'s alignment check; `assert`/
  `assert_eq` come only from felt intrinsics (`memory::packed2_fields`).

## Runtime semantics on the VM

- Linear memory is not bounds-checked: a read 256 MiB past the 17-page memory
  returns 0 where the host segfaults and wasmtime traps (`traps::trap_oob_read`).
- The shadow stack is 1 MiB (`__stack_pointer` = 0x100000, data after it,
  `(memory 17)`); frames up to about 1 MB pass (`memory::frame_1m`,
  `frames::deep_frames`). An overrun is not diagnosed: the wrapped address runs
  silently or trips the u32 range assertion by accident (`frames::deep_overrun`).
- Linear memory maps to element addresses below 2^19; spill slots live at FMP
  (from 2^31), so a frame cannot alias a slot (`frames::frame_spills`).
- `memory.grow` grows a dynamic heap of zero initial pages past static data,
  capped at `HEAP_END = (2^30 - 1) * 4` bytes, returning the old page count or
  -1; `memory.size` counts only that heap, so only flags and deltas compare
  (`heap::heap_grow`, `codegen/masm/intrinsics/mem.masm`). So a
  `memory_grow(0, k) * 65536` base points at the data segments. Dead end:
  the dynamic heap base; only the SDK intrinsic knows it.
- Trap parity holds for every Rust panic family at every configuration,
  allocator exhaustion included; statically present, dynamically dead panics
  stay dead (`traps::trap_index`, `traps::trap_slice_range`,
  `traps::trap_fnptr_dispatch`, `traps::trap_dead_guard`, `heap::heap_oom`).
- Edge values agree with Rust: shift counts mask `% width`, division at every
  boundary, clz/ctz of 0, len-0 copies, 0/1-trip loops, sub-word signs, wide
  multiplies, u128 carries and libcalls (`boundaries::shift_counts`,
  `boundaries::sdiv_bounds`, `boundaries::bitcnt_zero`,
  `boundaries::subword_sign`, `wide::wide_mul_edges`, `wide::u128_bounds_edges`).
- Loads and stores agree at every lane, packing and straddle tried (byte
  lanes, packed fields, u64 at 4 mod 8, u128 across Miden words, narrow i64
  stores, enums, odd aggregates) (`memory::lane_bytes`,
  `memory::packed_fields`, `memory::straddle_wide`, `memory::narrow64`).
- No pass merges, drops, moves or folds a load or store it must not, with or
  without freight or DWARF (`memorder::fwd_lanes`, `memorder::alias_views`,
  `memorder::swap_take`, `memorder::seq_freight`, `memorder::copy_grid`).

## Link reach (`core` and `alloc`)

- The blocking symbol is `memcmp` (no wasi-libc, no compiler-builtins `mem`):
  any comparison `core` keeps as an outlined slice compare fails the guest
  link with `rust-lld: undefined symbol: memcmp`, never a midenc diagnostic.
- Linkability is per program and per level. Fails at every level:
  runtime-length slice `==`/`!=`/`<`, `&str ==`, `starts_with`/`ends_with`,
  `contains`/`find`/`rfind` with a `&str` pattern
  (`corelib::core_slice_eq_nolink`, `corelib::core_str_eq_nolink`,
  `corelib::core_starts_with_nolink`, `corelib::core_str_search_nolink`), two
  nested `split(char)` loops (`corelib::prog_expr`). Constant-size array `==`
  fails at the default level and -Oz (`corelib::core_eq_reach`,
  `corelib::core_eq_reach_oz`); one `split(char)` per function only at basic
  (`corelib::core_str_patterns_basic`); `find(char)` at -Oz and basic
  (`corelib::core_str_find_oz`). The splitters compare the encoded pattern
  with a slice `==` (`SplitInternal<char>::next`).
- Linkable replacements: `iter().eq`, `zip().all`, `iter().cmp`,
  `eq_ignore_ascii_case`, `[u8]::contains`, a hand-written `char_indices`
  splitter (`corelib::prog_expr_wa`).
- Recursion is an assembler error (`found a cycle in the call graph`), not a
  link failure, and does not kill the test process: `select_nth_unstable`
  (`corelib::core_select_nth_nolink`), the STABLE sorts via
  `tiny::mergesort` (`heap::heap_sort_stable_nolink`), and the `Box`-list drop
  glue at basic only (`heap::heap_list_basic`). Unstable sorts link at every
  level (non-recursive `heapsort` under `optimize_for_size`,
  `corelib::core_sorts`); a hand-written bottom-up merge sort works
  (`heap::heap_sort`).
- Links and runs at every level: the non-comparing `core::str` surface,
  `core::fmt` (incl. `{:#?}`, `dyn Write`), iterator adapters, in-place slice
  algorithms, combinators, derived `Ord`, `char` APIs, `core::ptr`
  (`corelib::core_str_scan`, `corelib::prog_debugdump`,
  `corelib::prog_rawptr`, `programs::prog_fmt`, `programs::prog_iters`).
- `alloc` links and assembles: `Vec`, `Box`, `Box<dyn Trait>`, `Rc`/`RefCell`,
  `VecDeque`, `BinaryHeap`, `BTreeMap`/`BTreeSet`, `String`, `format!`,
  `collect` (`heap::heap_vec_kinds`, `heap::heap_dyn`, `heap::heap_rc`,
  `heap::heap_bheap`, `heap::heap_fmt`, `heap::heap_iters`). Their
  overlapping shifts (`Vec::insert`/`remove`/`drain`, `String::insert`,
  B-tree nodes) compile and match native (`heap::heap_vec_shift`,
  `heap::heap_vec_drain`, `heap::heap_vec_shift_u8`,
  `heap::heap_string_insert`, `heap::heap_btree`).
