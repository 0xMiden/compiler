# Corpus map: classes, reproducers, guards, dead ends

One entry per filed issue; the ignored test's doc comment and ignore reason
carry the detail. Run reproducers one per invocation (`-- --ignored --exact
end_to_end::differential::tests::<module>::<test>`). A fix PR un-ignores the
reproducers and keeps every guard green at every level. Mechanisms are in
`PIPELINE-FACTS.md`.

## Open classes

- **#1418, `memory.copy` lowered with memcpy semantics.** An overlapping copy
  aborts the VM when src, dst and count are 4-aligned, and is silently wrong
  on an upward overlap otherwise. Cause: wasm `memory.copy` is a memmove, but
  `OpEmitter::memcpy` calls the non-overlapping `memcopy_elements` on its
  element arm and copies forward on its byte arm. Minimal:
  `memory::mem_overlap`, `memory::copy_same_pos` (+ `_repro`). Realistic:
  `heap::heap_vec_shift_u8`, `heap::heap_string_insert` (silent),
  `heap::heap_vec_shift`, `heap::heap_vec_drain`, `heap::heap_btree` (abort),
  each + `_repro`. Guards: `heap::heap_vec_remove_u8`,
  `memory::copy_same_pos_disjoint`, `memory::copy_same_bytes`,
  `memorder::copy_fwd`, `boundaries::memnoop_same`. Un-ignore all thirteen.
- **#1419, `RemoveLoopInvariantArgsFromBeforeBlock` aliasing panic**
  (`AliasingViolationError` at `rewriter.rs:335`). Cause: the invariance test
  compares against the `scf.while` init operand, the function's single shared
  `ub.poison`, so the pattern matches whenever a column still carries poison,
  and its rewrite aborts on every match. Minimal: `compose::invariant_args_min`.
  Realistic: `programs::prog_varint`, `programs::prog_rle`,
  `programs_oz::prog_blake2b`, `programs_oz::prog_xxh64_o1`. Configuration
  twins: `compose::chain_sm_nodwarf`, `programs::prog_rkscan_guard_nodwarf`,
  `programs::prog_rkscan_ref_nodwarf`, `programs::prog_varint_guard_oz`,
  `programs::prog_varint_wa_oz_nodwarf`, `corelib::prog_numeric_nodwarf`.
  Guards: `compose::invariant_args_guard`,
  `compose::invariant_args_noreturn_guard`, and two former producers that
  compile at the default level with the current guests (they still panic with
  nightly-2026-04-30): `compose::nest_continue` (still panics at size-min and
  basic) and `compose::invariant_args_noreturn`. Un-ignore the eleven above.
- **#1420, TransformSpills reuses a stale dominator tree after splitting
  edges.** `edges to split > 0` plus `erase unused reload`, then a panic at
  `frontier.rs:123`, `lowering.rs:109` or `emit/mod.rs:623`. Cause:
  `rewrite_cfg_spills` rebuilds SSA from the `DominanceInfo` cached before its
  own splits; the frontier builder's `i > 1` test also skips two-predecessor
  joins. Minimal: `pressure::window_erased_min`,
  `pressure::overflow_cluster_min`, `pressure::frontier_seq`,
  `pressure::frontier_dispatch`, `pressure::zero_trip_frontier`,
  `pressure::zero_trip_overflow`. Realistic: `programs::prog_rkscan`,
  `programs::prog_sponge`, `programs::prog_histogram`, `programs::prog_feistel`,
  `programs::prog_tlv`, `programs::prog_iters` (+ `_edges`),
  `programs::fir_cordic_o3`, `programs::prog_fixedpoint_o3`,
  `programs::prog_rkscan_guard_o1`, `programs_oz::prog_sha512`,
  `corelib::prog_ordkeys` (+ `_edges`), `corelib::prog_ordkeys_max`,
  `corelib::prog_numeric_full`, `wide::wide_limbs_freight_oz`,
  `trapspill::cascade_cont_max`, `trapspill::guard_above` (also carries #1422
  markers). Fail only at basic (running tests): `frames::frame_spills`,
  `frames::rec_mutual`, `pressure::window_erased_guard` (each + `_edges`).
  Guards that must keep their ANSWERS: `pressure::window_erased_guard`,
  `pressure::zero_trip_guard` (+ `_repro`), `pressure::while_results`, the six
  `interact` cases (2 to 32 erased reloads each), `corelib::prog_slicealg_basic`,
  the `programs::*_guard` and `*_wa` siblings. Un-ignore every reproducer
  above, then sweep the three basic-only tests at basic.
- **#1421, five local fixes.** (1) The spill analysis reads operand group 0
  only, so `hir.exec_indirect` arguments are never reloaded:
  `calls::indirect_spill_bb`, and at basic `calls::indirect_spill_args`,
  `calls::indirect_spill_line` (guards at the default level beside
  `calls::indirect_spill` and `calls::direct_loop`, `calls::direct_line`,
  `calls::direct_args`). (2) A 256-target `br_table` overflows the u8
  operand-group index (`hir/src/ir/operation/builder.rs`):
  `control_flow::switch256`, guard `control_flow::switch255`. (3) A 17-felt
  signature panics in the spill analysis instead of a diagnostic:
  `calls::sig17`. (4) A guest build failure calls `process::exit` in
  midenc-compile: `corelib::core_eq_reach` (any memcmp `_nolink` case). (5)
  `rewriter=trace` panics with `AliasingViolationError` from
  `if_remove_unused_results.rs`: `cse::dead_region` with the trace on.
  Un-ignore `calls::indirect_spill_bb`, `control_flow::switch256`,
  `calls::sig17`.
- **#1422, operand-pressure cluster (fix after #1420).** (a) Arity 2 has only
  `TwoArgs` and no in-window fallback: `spills::rotl_window`,
  `spills::spill_loop_mix_oz`, `programs_oz::prog_threefish_o3`; guards
  `opt_levels::band_guard_oz`, `pressure::chain_window`,
  `pressure::unary_window`, `pressure::width_mix`. (b) Over-window pressure
  reaches the emitter with spills requested and no splits (the failing op is
  the spill store): `opt_levels::spill_store_min`,
  `programs_oz::prog_threefish_oz`; guards `opt_levels::spill_store_guard`,
  `programs_oz::prog_threefish_oz_guard`. (c) Non-dominating phi seeds from
  `insert_required_phis`: `control_flow::unroll_chain`, `spills::unroll_rotmix`;
  guard `spills::unroll_u32`. (d) Deep nests exceed the 16-felt region-exit
  budget (`spills.rs:1533`): `control_flow::deep_nest_overflow`,
  `opt_levels::nest8_oz`; guards `control_flow::nest8`,
  `control_flow::wide_exits`. Un-ignore the nine reproducers.

## Diagnostic gaps (not filed as bugs)

- Undiagnosed shadow-stack overrun (`frames::deep_overrun`, largest fitting
  rung `frames::deep_frames`); no linear-memory bounds check (`traps::trap_oob_read`).
- Link limits are `rust-lld: undefined symbol: memcmp`, not a midenc
  diagnostic: `corelib::core_eq_reach` (default level),
  `corelib::core_eq_reach_oz`, `corelib::core_str_patterns_basic`,
  `corelib::core_str_find_oz`, `corelib::core_slice_eq_nolink`,
  `corelib::core_str_eq_nolink`, `corelib::core_starts_with_nolink`,
  `corelib::core_str_search_nolink`, `corelib::prog_expr`. Recursion is the
  assembler's call-graph cycle: `corelib::core_select_nth_nolink`,
  `heap::heap_sort_stable_nolink`, `heap::heap_list_basic`.
- Full guest DWARF blocks Local2Reg (quality only): `debug_info::l2r_params`.
- `--optimize` sets only the guest LLVM level; midenc has no middle-end
  level (`cargo_profile_opt_level` in `midenc-compile/src/pipeline/frontends/rust.rs`).

## Closed classes kept as guards

- Coercion folders mutated the shared constant's attribute (wrong-width push,
  visible only in MASM), fixed by ef358e356: `wide::sext_const_shared`,
  `wide::parse_i64_hand`, `corelib::core_parse_i64`, `wide::sext_const_split`,
  `wide::zext_const_shared`, `wide::trunc_const_shared`,
  `wide::coerce_const_fanout`, `wide::parse_i64_hand11`,
  `wide::parse_i64_short`, `corelib::core_parse_u64`.
- LLVM `+wide-arithmetic` stale-local read (the wasm was wrong), gone with
  nightly-2026-09-01: `signed::sext_shapes`, `signed::checked_mul_i64`,
  `signed::sat_mul_i64`, `signed::pow_i64`, `wide::sat_add_u128`,
  `wide::sat_sub_u128`, `wide::fixmul_u64`, `wide::chk_add_u128_o1` (each +
  `_repro`), `wide::wide_words`, `wide::wide_loop_cmp` (each + `_edges`),
  `wide::wide_limbs_chain`, `calls::add128_checked` (+ `_oz`),
  `corelib::core_chkmul_i64`, `corelib::core_mulwide_s`.
- `br_table` selectors #1235/#1243: `control_flow::switch_shapes_repro`;
  sign extension i1288: `signed::sext_shapes_repro`.

## Dead ends

Shapes and levers that produce nothing, and why. Do not retry them.

- `hir.cast` / `OpEmitter::cast`: Rust casts never build it
  (`signed::sext_widths`). `_imm` load/store and quad-word arms: only unit
  tests and the `__stack_pointer` initializer; constant user addresses still
  go through `prepare_addr` (`memory::mem_globals`).
- Unchecked division, U64 `checked_divmod`, int32 N-bit normalization: no
  frontend caller (`codegen/masm/src/emit/int32.rs`; `arith::div_const_forms`).
- Multi-value returns and block parameters: no `+multivalue`
  (`calls::sret_shapes`). Felt memory ops, `mem_stream`, `store_array`,
  word-sized `memcopy_words`: no producer (`memory::copy_ladder`).
- Middle-end knobs (`Aggressive` simplification, `ControlFlowSink`, DCE,
  per-pass flags), advice-taint and postdominance analyses: need a source
  change or `-Zlint` (`midenc-compile/src/pipeline/backend.rs`).
- CSE region equivalence and non-dominance paths: CSE runs pre-lift
  (`cse::twin_if`). Positive `{a, a}` merge: each square needs a `local.tee`
  (`cse::multiset`). Splitting commutative twins with `black_box` or pointer
  math: LLVM re-merges (`cse::comm_arith`). Load / bulk write / load in one
  block: LLVM's `len != 0` guard splits it (`memorder::cse_bulk`).
- SCCP folding or dead-arm deletion: SCCP sees nothing LLVM missed
  (`sccp::dead_flag`, `sccp::dead_arm`); `OperationFolder::try_fold` and the
  sparse `meet` have no caller (`hir/src/folder.rs`,
  `hir-analysis/src/sparse/backward.rs`). Removing DWARF does not turn local
  merges into block results (`sccp::if_merge`).
- `CanonicalizeI64RotateBy32ToSwap`: the count band hides the 32
  (`mask_movement_count`). `SimplifyBrToReturn`: claimed first by
  `SimplifyBrToBlockWithSinglePred` (`control_flow::cf_shapes`).
  `WhileConditionTruth`: LLVM folds in-body reads of the condition
  (`control_flow::do_while`). `FoldConstantIndexSwitch`, `cf.Select::fold`:
  selectors are never constant post-lift (`control_flow::switch_forms`).
- Pattern variants: `WhileRemoveDuplicatedResults` fires only on the
  three-level triangle (`control_flow::triangle`); the column cascade needs an
  EMPTY `continue` (`canon::col_cascade`); duplicate `match` arms do not reach
  `SimplifyCondBrLikeSwitch` (`control_flow::sm16`); six return sites stop
  `SimplifyPassthroughCondBr` (`canon::passthru_frame`).
- Multi-felt cfg-to-scf columns: state crosses in locals
  (`control_flow::nest8`). Two `ub.unreachable`s for `combine_exit`: LLVM
  merges traps, a helper only moves the one (`trapspill::both_kinds`,
  `trapspill::helper_arg`). One case per panic kind under freight: all kinds
  are the same `unreachable` (`trapspill::body_get`, `trapspill::body_div`).
- Local2Reg declare conversion, poison arm, `is_declaration`: no single-op
  `[WasmLocal]` location, no read-before-write local, no imports
  (`debug_info::dbg_byval`). Data-segment insert/overlap arms: wasm-ld emits
  sorted disjoint segments (`memory::segment_mix`).
- Dead HIGH half of `mul_wide`: LLVM emits a narrow `i64.mul`
  (`opt_levels::mulhi_dead_oz`). A multi-use 4-felt operand from widening
  multiplies: each gets its own zext pair (`signed::mulwide_dyn`).
- Solver interiors (`CopyAll`, `SwapAndMoveUp`, the first evict of
  `MoveDownAndSwap`, fuel exhaustion): operands stay adjacent to their op
  (`scale::chain300`). Block-entry interleave drops: entry liveness is uniform
  (`opt_levels::drop_solver_oz`). Terminator reloads, splits carrying
  successor arguments, pre-lift live-through: never reached (`spills::spill_loop`).
- Funcref tables beyond one contiguous table (PIC base, holes, multi-table,
  intrinsic entries): wasm-ld never emits them (`calls::indirect_sigs`).
- The dynamic heap base: only the SDK intrinsic knows it (`heap::heap_grow`).
  -Oz as a coverage source: it opens no new compiler function
  (`opt_levels::loop_keep_oz`).
- User fixes that fail: fewer distinct rotation constants (non-monotone,
  `programs_oz::prog_threefish_oz_guard`), `[u64; N]` state, hand-written
  rotates, flattening or splitting the program for the frontier panic
  (`programs::prog_tlv_guard`), another `-O` (`programs_oz::prog_sha512`).
