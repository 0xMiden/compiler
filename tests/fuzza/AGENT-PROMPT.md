# fuzza agent prompt template

Template for launching an agent to grow fuzza compiler coverage. Copy, fill in
the `[area]` placeholder with the compiler directory/file you want to target
(e.g. `codegen/masm/src/emit/`, `frontend/wasm/src/code_translator/mod.rs`,
`dialects/scf/`), and hand the prompt to the agent.

Before launching, confirm reachability: open `target/fuzza-coverage/report.md`
and check that the target area has untouched functions whose signatures could
plausibly be reached from a `(u32, u32) -> u32` no-std cdylib. Compiler code
behind SDK/protocol/account/note scripts, the debug-engine UI, or anything
gated on a `#[component]` attribute is **not reachable** from this harness —
pick a different area if that's most of what shows up in the cold list.

---

## Objective

Maximize region coverage in **[area]** of the Miden compiler via the fuzza
harness. **Stop when five consecutive new cases each add zero new regions in
[area]**, or after twenty cases total, whichever comes first.

## Case constraints

Each new case is a single `.rs` file at
`tests/fuzza/src/cases/case_<name>.rs` and MUST:

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

Wire the new case in `tests/fuzza/src/tests.rs`:

```rust
#[test]
fn <name>() {
    run_case("<name>", include_str!("cases/case_<name>.rs"));
}
```

## Loop

1. **Read** `target/fuzza-coverage/report.md`. Focus on entries whose `File:line`
   is inside **[area]** — first in the "Top untouched functions" section, then
   in "Partially-covered functions".
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
3. **Write** `case_<name>.rs` designed to exercise that construct through the
   `(u32, u32) -> u32` entrypoint. Keep it minimal — the less incidental code,
   the easier to interpret failures.
4. **Run** `cargo make fuzza-cov-step`. Read the new `report.md`. The "Delta
   since previous run" section tells you:
   - `Regions covered: +ΔR` — was your case productive?
   - `Newly-exercised functions` — did you hit the target or adjacent code?
5. **Record the outcome** in a scratch log so you don't repeat dead-end ideas:
   - If `ΔR > 0` in **[area]**: keep the case, continue.
   - If `ΔR == 0`: keep the case only if it hits new regions elsewhere worth
     having; otherwise delete it and its `#[test]` entry.
   - If the test failed (native/MASM divergence): mark the `#[test]` with
     `#[ignore = "<short reason + minimal failing input>"]` so the suite stays
     green; note it as a potential compiler bug finding.
   - If the test panicked during compile (cargo-miden error): the case
     probably hit unsupported Rust constructs — delete it, try something
     simpler.
6. **Stop** per the objective above.

## Reference commands

```bash
# First run (clean-slate baseline; takes several minutes for instrumented build):
cargo make fuzza-cov

# Each iteration (reuses the instrumented build; ~20-60s warm):
cargo make fuzza-cov-step

# Nuke all coverage state and start over:
cargo make fuzza-cov-clean
```

Outputs live under `target/fuzza-coverage/`:
- `report.md` — what you read.
- `report.prev.json` — previous snapshot (used for the delta).
- `html/html/index.html` — per-line highlighted source view for debugging
  which exact lines you hit.
