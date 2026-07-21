**Target area:** `[area]` — a free-form description of what to cover, e.g.
`memory read/write`, `control flow`, `u64 arithmetic`.

## Read first

Before touching anything, read (in this order):

1. [`README.md`](README.md) — how the harness and the coverage loop work.
2. [`KNOWLEDGE.md`](KNOWLEDGE.md) — the accumulated fact base: compiler
   reachability facts, LLVM pre-cleaning traps, case-writing tricks, and
   operational gotchas. Do **not** re-derive anything recorded there.
3. `tests/integration/src/end_to_end/differential/tests.rs` and the case files
   it references — do not duplicate constructs existing cases already cover.
   Pay special attention to the `#[ignore]`d tests: they are the single source
   of truth for known bugs (failure, exact inputs, what has been bounded, and
   the un-ignore conditions). Some otherwise-reasonable shapes are currently
   blocked by bugs documented there; never re-report one as a new finding.

## Resolve the area to paths

The target area is worded for humans; your first job is to translate it into
the concrete compiler source paths it refers to, then export those as
`FUZZA_AREA` so the coverage report scopes itself to the area.

1. **Find the source.** Search the compiler crates (`codegen/`, `frontend/`,
   `dialects/`, `hir/`, `hir-*/`, …) for the ops, emitters, passes, and
   translation arms that implement the area. For example, `memory read/write`
   resolves to the load/store emitters (`codegen/masm/src/emit/mem.rs`), the
   wasm address preparation (`dialects/wasm/src/mem.rs`), and the data-segment
   lowering (`codegen/masm/src/data_segments.rs`).
2. **Export the resolved paths** as a comma-separated list of workspace-relative
   prefixes (a file, or a directory ending in `/`):

   ```bash
   export FUZZA_AREA='codegen/masm/src/emit/mem.rs,dialects/wasm/src/mem.rs,codegen/masm/src/data_segments.rs'
   ```

   `export` only helps in a shell that persists. If you are an agent whose
   Bash tool starts a fresh shell per command, the export is lost — prefix
   every coverage command instead:
   `FUZZA_AREA='...' cargo make fuzza-cov-step`.

   With `FUZZA_AREA` set, `report.md` gains a **"Target area"** section with an
   area-only headline, an area-only delta, and the full list of cold functions
   in the area — read that section instead of hand-summing the global numbers.
3. **Sanity-check the mapping.** After the first `cargo make fuzza-cov`, confirm
   the "Target area" section lists the functions you expect; widen or narrow the
   paths if it's catching too much or too little. Record the resolved paths in
   the scratch log so later steps reuse the same scope.

## Objective

Maximize region coverage in the target area of the Miden compiler via the
fuzza harness. **Stop when any of these holds:**

- Five consecutive new cases each add zero new regions in the target area, or
- twenty cases total, or
- **you can argue the remaining cold regions in the area are unreachable** from
  a `(u32, u32) -> u32` `#![no_std]` entrypoint (this is the *preferred* exit —
  reading the remaining cold functions and explaining why each is out of reach
  beats grinding five ritual zero-delta cases). Record the full argument in the
  scratch log, and distill the durable parts — new reachability facts, verified
  dead ends — into [`KNOWLEDGE.md`](KNOWLEDGE.md): scratch logs are gitignored
  and machine-local; `KNOWLEDGE.md` is what future runs actually see.

## Case constraints

Each new case is a single `.rs` file at
`tests/integration/src/end_to_end/differential/cases/case_<name>.rs` and MUST:

- Contain only a `#[unsafe(no_mangle)] pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32`
  (plus any helper `fn`s / `const`s it needs). The harness prepends
  `#![no_std]` and a panic handler — do not include those yourself.
- Build under `#![no_std]` (no `std::`, no heap allocations, no external
  crates).
- Be deterministic: same `(input1, input2)` must always produce the same
  output. No `unsafe` outside the `no_mangle` attribute.
- Avoid operations that panic on valid `u32` inputs (guard division/modulo by
  zero **and signed division against `MIN / -1`**, use `wrapping_*` for
  arithmetic). The harness fuzzes with random `u32` pairs; a native-side panic
  is a case failure.
- Be careful with mutable `static`s. The native `cdylib` is loaded **once** and
  reused across all 16 proptest inputs, so a static (e.g. an `AtomicU32`) that
  you mutate carries state from one invocation to the next — which breaks
  determinism unless you restore it before returning. (Statics are still a
  useful tool: non-zero initializers are how you reach the data-segment code.)
- Stay away from known compile-breakers unless they are your target: flat
  signatures over 16 stack felts, function pointers, and recursion (see
  `KNOWLEDGE.md`), plus the shapes behind the `#[ignore]`d compiler-panic
  reproducers in `tests.rs`.

Wire the new case in `tests/integration/src/end_to_end/differential/tests.rs`:

```rust
#[test]
fn <name>() {
    run_case("<name>", include_str!("cases/case_<name>.rs"));
}
```

## Loop

1. **Read** `target/fuzza-coverage/report.md`. With `FUZZA_AREA` set, read its
   **"Target area"** section: work the "Area — untouched functions" list first,
   then "Area — partially-covered functions". (Without `FUZZA_AREA`, fall back
   to the global "Top untouched"/"Partially-covered" sections and filter by
   `File:line` yourself.)
2. **Pick one target function.** Skim its source at `File:line` to understand
   what Rust-level construct triggers it (e.g. a particular HIR op kind, a
   specific branch in wasm translation).

   **Mind the routing layer.** Between Rust source and the codegen emitter
   sits the wasm frontend (`frontend/wasm/src/code_translator/`), which
   translates each Wasm op into a specific HIR op; the emitter
   (`codegen/masm/src/emit/`) then dispatches by HIR op kind. Picking a fat
   cold emitter function only pays off if user-level Rust actually causes
   the frontend to emit the matching HIR op — e.g. Rust `as` casts get
   translated to HIR `trunc`/`zext`/`sext`, **not** HIR `cast`, so targeting
   `OpEmitter::cast` via `as` won't work. Two consequences:
   - Function-region size in the cold list is a noisy signal: a fat
     untouched function can be unreachable from `(u32, u32) -> u32` Rust at
     all, while a smaller neighbour may be the real win.
   - Before betting on an emitter target, sanity-check the chain by
     skimming `frontend/wasm/src/code_translator/` (or the relevant
     dialect/op-builder code) to confirm "Rust construct X causes HIR op Y
     which dispatches to emitter Z."
   - Constants in Rust source do **not** generally reach `_imm` emitter
     variants. The wasm frontend calls general builder methods
     (`builder.add`, `builder.eq`, …) regardless of whether an operand is
     a literal; getting to an `_imm` arm usually requires HIR-level
     canonicalization (e.g. arith constant folding), not raw user code.

   `KNOWLEDGE.md`'s routing-facts section records many verified chains and
   dead ends — check it before spending a probe.
3. **Write** `case_<name>.rs` designed to exercise that construct through the
   `(u32, u32) -> u32` entrypoint. Keep it minimal — the less incidental code,
   the easier to interpret failures.

   **Probe before paying a coverage step.** Wire the case's `#[test]` with a
   temporary `#[ignore]`, then run just that test *outside* llvm-cov with an
   IR dump to check that the shape actually reaches your target:

   ```bash
   cargo make fuzza-probe <test-name> hir   # or wat / masm
   ```

   This takes well under a minute warm (vs. a full coverage step), keeps the
   IR printers out of the coverage data, and writes the dumps to
   `target/fuzza-probe/<case>/`. Un-ignore the test only when the dumped IR
   contains the construct you're aiming at; otherwise rework or delete the
   case — most wasted steps come from shapes LLVM pre-cleaned away. (If you
   ever set `MIDENC_EMIT` by hand, use an absolute `kind=DIR` spec — bare
   kinds dump into the process CWD and litter the source tree, and relative
   dirs vanish into the ephemeral build workspace.)
4. **Run** `cargo make fuzza-cov-step`. Read the new `report.md`:
   - The **"Target area"** section's `Area delta since previous run:
     +ΔR regions` line is your productivity signal for the area — this is the
     number the stop condition is phrased in terms of.
   - The global "Delta since previous run" section's `Regions covered: +ΔR` and
     `Newly-exercised functions` tell you whether the case also helped adjacent
     code.

   Note: coverage **accumulates** across `fuzza-cov-step` runs (the profile data
   is merged, not reset). So the area headline is cumulative, the delta is
   per-step, and deleting a case's `#[test]` does **not** drop its regions from
   the report until the next `cargo make fuzza-cov-clean`.

   Two report gotchas: when duplicate monomorphized `(file, name)` rows exist,
   the `Area delta` line inflates by a constant — judge productivity by the
   difference of the area *headline* between steps; and a `fuzza-cov-step`
   launched right after a backgrounded `fuzza-cov` can produce an empty report
   (0 tests, 0 regions) — just rerun the step. You can also re-render
   `report.md` with corrected `--area` scoping without rerunning any tests:

   ```bash
   python3 tools/fuzza-agent/cov.py target/fuzza-coverage/report.json . \
       --prev target/fuzza-coverage/report.prev.json --area '<paths>' \
       > target/fuzza-coverage/report.md
   ```
5. **Record the outcome** in a scratch log at
   `tools/fuzza-agent/scratch/<area>-log.md` (create the dir if missing) so you
   don't repeat dead-end ideas. This path lives outside `target/`, so it
   survives `fuzza-cov-clean`; it is gitignored, so it won't be committed —
   promote anything durable into `KNOWLEDGE.md`.
   - If `ΔR > 0` in the target area: keep the case, continue.
   - If `ΔR == 0`: keep the case only if it hits new regions elsewhere worth
     having; otherwise delete it and its `#[test]` entry.
   - If the test failed (native/MASM divergence): mark the `#[test]` with
     `#[ignore = "<short reason + the exact failing inputs>"]` so the suite
     stays green, and **add a pinned twin** — a second `#[ignore]`d test named
     `<name>_repro` that calls `run_case_with_inputs` with the exact failing
     pair, so the bug reproduces deterministically instead of only when
     proptest happens to draw it (see `switch_shapes_repro` and
     `sext_shapes_repro` in `tests.rs`). If the case mixes several constructs,
     split it so each divergence gets its own minimal reproducer — the passing
     siblings *bound* the bug for free. The test's doc comment and ignore
     reason are the bug's **only** documentation (nothing goes in README or
     `KNOWLEDGE.md`), so make them self-sufficient: the failure, the exact
     inputs, what sibling cases bound, and what would allow un-ignoring. The
     compile-side coverage the case triggered before the divergence is still
     captured in the report — a failing case is not wasted work and the delta
     still counts. (The noted input is the *first* random failure, not a
     minimized one — proptest shrinking is disabled.) To understand the
     divergence, re-emit the intermediate artifacts with `MIDENC_EMIT`
     (WAT/HIR/MASM) and trace MASM execution with
     `MIDENC_TRACE=executor=trace` — see the `emit` and `trace` skills —
     unless your campaign policy defers root-causing.
   - If the build failed at compile time, distinguish two situations. A clean
     diagnostic for a known-unsupported construct (function pointers /
     `call_indirect`, a recursive call graph, …) means the case is out of
     scope — delete it and note the dead end in the scratch log. A compiler
     *panic* on safe, supported-looking Rust is a **finding**, not a dead end:
     minimize the case and keep it as an `#[ignore = "<panic message +
     location>"]`d test (see `spill_edge` and `i64_srem` in `tests.rs`) — here
     too the test is the bug's only documentation, so make its comment
     self-sufficient. Check the existing `#[ignore]`d tests first: several
     panics are already known and must not be re-reported as new.
6. **Stop** per the objective above. When you stop by the unreachability
   argument, write the per-function reasoning into the scratch log and the
   durable reachability facts into `KNOWLEDGE.md` so the next run doesn't
   re-explore the same dead ends.

## Reference commands

```bash
# Scope every report below to the target area. Set this to the source paths you
# resolved the free-worded area to above (see "Resolve the area to paths").
# In a persistent shell, export once; in a fresh-shell-per-command agent,
# prefix each invocation instead: FUZZA_AREA='...' cargo make fuzza-cov-step
export FUZZA_AREA='codegen/masm/src/emit/mem.rs,dialects/wasm/src/mem.rs,codegen/masm/src/data_segments.rs'

# First run (clean-slate baseline; takes several minutes for instrumented build):
cargo make fuzza-cov

# Each iteration (reuses the instrumented build; ~20-60s warm):
cargo make fuzza-cov-step

# Cheap reachability probe: run ONE test outside llvm-cov with an IR dump
# (MIDENC_EMIT) and list the freshly-written artifacts:
cargo make fuzza-probe <test-name> hir     # kinds: hir / wat / masm, or a,b list

# Nuke all coverage state and start over (also wipes target/fuzza-coverage, but
# NOT your scratch log under tools/fuzza-agent/scratch/):
cargo make fuzza-cov-clean
```

Outputs live under `target/fuzza-coverage/`:
- `report.md` — what you read; includes the "Target area" section when
  `FUZZA_AREA` is set.
- `report.prev.json` — previous snapshot (used for the delta).
- `html/html/index.html` — per-line highlighted source view for debugging
  which exact lines you hit.
