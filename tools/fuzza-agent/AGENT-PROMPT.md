**Target area:** `[area]` — a free-form description of what to cover, e.g.
`memory read/write`, `control flow`, `u64 arithmetic`.

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
  beats grinding five ritual zero-delta cases). Record the argument in the
  scratch log.

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
  zero, use `wrapping_*` for arithmetic). The harness fuzzes with random
  `u32` pairs; a native-side panic is a case failure.
- Be careful with mutable `static`s. The native `cdylib` is loaded **once** and
  reused across all 16 proptest inputs, so a static (e.g. an `AtomicU32`) that
  you mutate carries state from one invocation to the next — which breaks
  determinism unless you restore it before returning. (Statics are still a
  useful tool: non-zero initializers are how you reach the data-segment code.)

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
3. **Write** `case_<name>.rs` designed to exercise that construct through the
   `(u32, u32) -> u32` entrypoint. Keep it minimal — the less incidental code,
   the easier to interpret failures.
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
   the report until the next `fuzza-cov-clean`.
5. **Record the outcome** in a scratch log at
   `tools/fuzza-agent/scratch/<area>-log.md` (create the dir if missing) so you
   don't repeat dead-end ideas. This path lives outside `target/`, so it
   survives `fuzza-cov-clean`; it is gitignored, so it won't be committed.
   - If `ΔR > 0` in the target area: keep the case, continue.
   - If `ΔR == 0`: keep the case only if it hits new regions elsewhere worth
     having; otherwise delete it and its `#[test]` entry.
   - If the test failed (native/MASM divergence): mark the `#[test]` with
     `#[ignore = "<short reason + minimal failing input>"]` so the suite stays
     green; note it as a potential compiler bug finding. The compile-side
     coverage the case triggered before the divergence is still captured in
     the report — a failing case is not wasted work and the delta still
     counts. (The noted input is the *first* random failure, not a minimized
     one — proptest shrinking is disabled.) To understand the divergence,
     re-emit the intermediate artifacts with `MIDENC_EMIT` (WAT/HIR/MASM) and
     trace MASM execution with `MIDENC_TRACE=executor=trace` — see the `emit`
     and `trace` skills.
   - If the test panicked during compile (cargo-miden error): the case
     probably hit unsupported Rust constructs — delete it, try something
     simpler.
6. **Stop** per the objective above. When you stop by the unreachability
   argument, write the per-function reasoning into the scratch log so the next
   run doesn't re-explore the same dead ends.

## Reference commands

```bash
# Scope every report below to the target area. Set this to the source paths you
# resolved the free-worded area to above (see "Resolve the area to paths"), and
# keep it exported for the whole session:
export FUZZA_AREA='codegen/masm/src/emit/mem.rs,dialects/wasm/src/mem.rs,codegen/masm/src/data_segments.rs'

# First run (clean-slate baseline; takes several minutes for instrumented build):
cargo make fuzza-cov

# Each iteration (reuses the instrumented build; ~20-60s warm):
cargo make fuzza-cov-step

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
