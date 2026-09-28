# fuzza operational reference

What an agent needs to run a campaign. Companion:
[`PIPELINE-FACTS.md`](PIPELINE-FACTS.md) (what the compiler does with plain
Rust, stage by stage, and the dead ends). Read both before writing a case and
rebuild the corpus map with grep (below). Bug detail lives only at the
`#[ignore]`d test and in the filed issue. A fact
goes in only with a proof pointer (`module::test` or source path + function);
when a fact changes, rewrite it in place.

## How the harness decides

A case is the body of `entrypoint(u32, u32) -> u32` plus helpers
(`tests/integration/src/end_to_end/differential/harness.rs`). The harness
prepends a `#![no_std]` header, builds the case natively as a host `cdylib`
(the reference) and through cargo-miden to MASM, and compares the two over 16
boundary-biased pairs (half uniform, half from `INTERESTING_U32`, one pair in
eight forced equal). A mismatch or VM error fails the test; a compile panic
fails it before any input runs. The reference is native code, so a
guest-toolchain miscompile is a divergence too (arbitrate, below). Entry
points: `run_case`; `run_case_with_inputs` (pinned grid, the `_edges` and
`_repro` twins); `run_case_with_flags` and `run_case_with_flags_and_inputs`
(midenc flags per case, plus the pseudo-flag `--guest-debug=0|1|2` for the
guest debug level); `run_case_traps`, `run_case_traps_with_inputs`,
`run_case_traps_with_flags` (trap parity: `TRAPPING_CASE_HEADER`, the host
runs in a forked child that `_exit(101)`s on panic, each input must agree on
value-or-trap, a trap on both sides is a match).

## Running and sweeping

- `CARGO_TARGET_DIR=$PWD/target cargo test -p midenc-integration-tests --lib
  <filter> -- --test-threads=8`. One filter per invocation: names after `--`
  are OR-ed with the first filter.
- `--exact` needs the FULL path `end_to_end::differential::tests::<module>::<test>`
  (a partial path runs zero tests). `--ignored` runs ignored tests; run them
  one per invocation. `--skip` is a substring filter.
- Env knobs: `MIDENC_DIFF_FLAGS='<flags>'` (whitespace-split, appended after
  the case's flags; an option the case pins keeps the case's value, keyed on
  the text before `=`), `FUZZA_GUEST_DEBUG=0|1|2` (default 2),
  `FUZZA_INPUT_PAIRS=N`. The native build is never affected.
- `--optimize=`: default = LLVM 2, `size-min` z, `max` 3, `basic` 1.
  `RUST_MIN_STACK=8388608` for max sweeps (big unrolled guests overflow the
  2 MiB test thread in the assembler; not a finding).
- Link skips for a whole-corpus sweep: only `corelib`, `--skip core_str_find`
  at size-min, plus `--skip core_str_patterns` at basic.
- Machine time (about 700 tests): default run about 6 min, a flag sweep 8 to
  9 min, `FUZZA_INPUT_PAIRS=256` about 73 min (over half in `programs`,
  `programs_oz`, `memory`). A warm case about 2 s; Keccak-f[800] about 4 s
  per pair.
- 32 running tests pin `--optimize` (13 in `opt_levels`, 14 in `programs_oz`,
  `calls::add128_checked_oz`, `corelib::prog_slicealg_basic`,
  `programs::fir_cordic_guard_o3`, `wide::chk_add_u128_o1` + `_repro`); level
  sweeps never measure them at the sweep's level. None pins `--guest-debug`.
- Sweep every kept case at max, size-min, basic and `FUZZA_GUEST_DEBUG=0`
  before calling it a guard.

## Evidence commands

- `MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`: spills, reloads,
  `edges to split = N`, `erase unused reload`, `unused phi`, `additional spills
  required`, `convert reload to load`.
- `MIDENC_TRACE='pattern-rewrite-driver=trace'`: `trying to match '<pattern>'`
  before each attempt, `pattern matched successfully` after a rewrite (lines
  carry `dialect=`/`op=`).
- `MIDENC_TRACE='codegen:operand-scheduling=trace'`: `there are N used operands
  out of M`; the last `dropping unused operands at:` names the op the emitter
  was on when it panicked.
- `rewriter=trace` can itself panic (`cse::dead_region`). `pass:local2reg=trace`
  logs `found promotable local` BEFORE the debug check; subtract the `debug
  declarations cannot all be converted safely` lines.
- `-Z print-ir-after-pass=<pass>` logs to target `pass:<pass>`: it needs the
  flag, `MIDENC_TRACE='pass:<pass>=trace'` and `--nocapture`. Pass names:
  `canonicalizer`, `cse`, `sparse-conditional-constant-propagation`,
  `sink-operand-defs`, `local2reg`, `transform-spills`, `lift-control-flow`
  (the second canonicalizer/spill runs share the name; tell them by order).
- `cargo make fuzza-probe <test> hir|wat|masm` dumps to
  `target/fuzza-probe/<case>/`; by hand, `MIDENC_EMIT` needs absolute
  `kind=DIR` specs.
- Guest wasm: `target/miden_test_shared/wasm32-wasip1/release/differential_<case>.wasm`
  (`wasm-tools print <wasm> | grep -c call_indirect`; WAT `align=` is bytes).
- Arbitration: `wasmtime run -W wide-arithmetic=y --invoke entrypoint <wasm> a b`.
  wasmtime == native: midenc is wrong; wasmtime == MASM: the wasm is wrong.
  `--invoke` parses i32, so pass large u32 in signed form (after `--`).
- Old-toolchain arbitration: `RUSTUP_TOOLCHAIN=nightly-2026-04-30 <test binary>
  --ignored --exact <path>` (the nested cargo honours it).
- Standalone probes: build the case with cargo and the flags of
  `MANDATORY_RUST_FLAGS` (`midenc-compile/src/pipeline/frontends/rust.rs`)
  plus build-std, `optimize_for_size`, LTO, cgu=1, then run `midenc <wasm>
  --release [--optimize=...]` to classify a link failure or panic in seconds
  (no VM). The native half is a host build over the 35x35 boundary grid;
  strip `extern "C"` first or panics abort instead of unwinding.

## Classifying a compile-time panic

The crash site does not name the mechanism. Take the spills, pattern and (for
emitter panics) drop traces, then apply in order:
1. `edges to split > 0` plus `erase unused reload`: stale dominator tree
   (#1420), whatever the site (`frontier.rs:123`, `lowering.rs` `NoSolution`,
   `emit/mod.rs` index overflow).
2. Arity-2 `NoSolution` with a Copy constraint on a stack of at most 16 felts:
   the arity-2 gap (#1422), even if the function spilled elsewhere. The felt
   total alone does not decide at -Oz (dead, undropped operands are invisible
   to the analysis).
3. `emit/mod.rs` index overflow with `additional spills required`, no splits,
   no erasure, last drop-trace op a spill `hir.store_local`: spill placement
   past the window (#1422).
4. `AliasingViolationError` at `rewriter.rs`: the last `trying to match` names
   the pattern (`remove-loop-invariant-args-from-before-block` is #1419).

A pass is not a fix. Before un-ignoring, tell a compiler fix (passes with the
old guest toolchain too) from a guest-toolchain fix (wasmtime now returns
native, op still in the wasm) and a shape loss (old toolchain still panics),
then sweep every level. When a twin and its control differ by one trapping
edge, the markers can appear in both: report the spill/reload balance, and
build the masked-index control before calling a trap-edge variant new.

## Gotchas

- A guest that fails to build or link makes midenc-compile `process::exit`,
  killing the whole run with no summary (`error: test failed`, `Broken pipe`).
  Run suspected non-linking cases alone. An assembler call-graph cycle is an
  ordinary failure.
- A passing test swallows its output (`--nocapture`); when tracing several
  tests in one run, log lines belong to the `test <name> ...` line above.
  Quote globs and `::` paths in zsh.
- Under `run_case` the native panic handler is `loop {}`: a native panic
  (bounds, OOM) HANGS the test; use `run_case_traps`. Native recursion past
  the 2 MiB test thread aborts the process and loses all other output.
- Coverage reports: judge progress by the area headline (duplicate monomorph
  rows inflate `Area delta`); check warm sibling monomorphs and a cold arm's
  callee before calling it a gap; panic unwinding leaves phantom warm
  regions; re-render with `cov.py ... --area '<paths>'`; ignored cases add
  nothing after `fuzza-cov-clean`; rerun a step that reports 0 tests. Agent
  shells lose `export`s: prefix `FUZZA_AREA='...'` on every call.
- Coverage bookkeeping: `fuzza-cov-step` rotates `report.json` into
  `report.prev.json` and overwrites that on the next step, so copy a
  baseline aside right after producing it; the rendered tables cap at 30
  rows, so extract an area's full per-function list from `report.json`;
  measure what a lever opened with a per-function set diff between two
  `report.json` snapshots, never with the rendered "newly-exercised" count
  (a test-crate-rebuild artifact).
- Corpus map on demand: every `#[ignore = "..."]` string starts with its
  filed issue (`#1418`: ...) or `gap: ` (diagnostic gaps and link limits, not
  filed as bugs). A class's reproducers, i.e. the un-ignore list of its fix
  PR: `grep -rn -A3 '#\[ignore'
  tests/integration/src/end_to_end/differential/tests/ | grep '#1420'`.
  Completeness check (must print nothing): `grep -rn '^#\[ignore'
  tests/integration/src/end_to_end/differential/tests/ | grep -v
  '"\(#1[0-9]\{3\}\|gap\): '`. Twins follow the naming convention (`_repro`,
  `_edges`, `_guard`, `_oz`, `_o1`, `_basic`, `_max`, `_nodwarf`, `_min`);
  guards are named in the reproducers' doc comments ("Bounded by"). Closed
  classes kept as guards are found by their markers in the doc comments
  (`ef358e356`, `nightly-2026-09-01`) or by a doc link to a test that
  carries one.

## Writing cases

Rules: plain Rust only; `//` comments (the prepended header makes `//!` an
E0753); every signature at most 16 felts; no slice or `str` `==`; restore
mutable statics before returning (the native library is reused across
pairs); deterministic results; typed generated literals (an E0689 kills the
batch); no `gen` identifiers (edition 2024); no `&STATIC[a..b]` in a static
initializer (E0658).

Tricks that work:
- `core::hint::black_box` on values LLVM must not see, and on every fn-pointer
  table read (`black_box(&TABLE)[i]`), or the dispatch is devirtualized.
- Two volatile reads of one address stay distinct (swapped commutative
  operands in HIR).
- `while i < input % 97` keeps a zero-trip bypass; `% 97 + 3` gives a
  bottom-tested loop; `& 7` bounds are peeled.
- Impossible guards: `h % 6 == 5 && h % 3 == 0`. Dynamic zero:
  `x.wrapping_mul((input & 1) as u64)`.
- Pin a pure helper with `PIN.fetch_add(0, Relaxed)` folded into its result.
- Freight: a u64 cluster (defined before the region, consumed in ONE wide
  expression inside, used after) plus count bands (constant shift/rotate
  counts reused before, inside, after). Keep 6 to 8 felts in passing cases and
  start from a committed `interact` case.
- Trap predicates from LOW accumulator bits (mixing chains saturate the top
  bits); measure trap density on the native grid first.
- Exit tags in the top nibble (`(tag << 28) | (acc & 0x0fff_ffff)`) plus a
  native scan make each `_edges` grid cover every exit and 0/1/n-trip path.
- Allocator (`heap::heap_vecsum`): case-local bump allocator over a
  `#[repr(align(16))]` `UnsafeCell<[u8; N]>` arena, no-op dealloc. Rules:
  reset `NEXT = 0` first in `entrypoint`; address-independent results (content
  hashes, `len() as u32`; never pointers, `capacity()`, `usize`); keep the
  `align(16)`.
- Index with `(x % N) as usize`, never `(x as usize) % N` or
  `usize::try_from(u64)` (`usize` differs by target).
- Unstable sorts need injective keys (guest and host sort differently).
- Large frames: `[MaybeUninit<u32>; N]` (zeroing costs about 25 cycles/byte).
- Remainders need an operand pair with no matching division; `<=`/`>=`/`ge_u`
  need a bool materialized in an `#[inline(never)]` helper.

LLVM pre-cleaning and fruitless shapes (`Dead end:`): `PIPELINE-FACTS.md`.

## Out-of-scope surfaces

- Linker stubs, `export_name` overrides and the routing behind them
  (`frontend/wasm/src/module/linker_stubs.rs`, `frontend/wasm/src/intrinsics/`,
  `emit/felt.rs`): implementation detail of the current linking scheme.
- Floats: compute operators fail with `Wasm op <Name> is not supported`, `f64`
  in a signature fails type conversion, f32 bit transport is
  implementation-defined; no case may assert it.
- SDK crates, `Call`/`Syscall`/`ExecFpi`, felt intrinsics, advice/crypto ops.
- Direct recursion (assembler rejects it) and signatures over 16 felts.
- `call_indirect` runtime failures (bad index, null or mismatched slot): UB
  natively, covered by `end_to_end/indirect_call_traps.rs`.
- `--test-harness` codegen: not swept, executor semantics unclear.
