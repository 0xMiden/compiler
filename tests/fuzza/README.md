# midenc-fuzza

Differential fuzzing harness for the Miden compiler.

## How it works

Each test case under `src/cases/` is a single `.rs` file containing only a
`#[unsafe(no_mangle)] pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32`
plus any helpers it needs. The harness prepends `#![no_std]` and a panic
handler, then for each case:

1. Builds it natively as a host `cdylib` and loads it with `libloading`.
2. Builds it via `cargo-miden` to a MASM package.
3. Runs both with 16 random `(u32, u32)` input pairs via proptest.
4. Asserts the outputs match.

A divergence is a likely compiler bug; the case file itself is the minimal
reproducer (proptest shrinking is disabled).

## Running the tests

```bash
cargo test -p midenc-fuzza
# or
cargo nextest run -p midenc-fuzza
```

Cases known to surface compiler divergences are marked `#[ignore = "..."]`
in `src/tests.rs` with the failing inputs noted. Run them explicitly with
`--ignored` (or `--run-ignored all` under nextest) to investigate.

## Adding a case manually

1. Create `src/cases/case_<name>.rs` with the `entrypoint` function.
2. Wire it up in `src/tests.rs`:
   ```rust
   #[test]
   fn <name>() {
       run_case("<name>", include_str!("cases/case_<name>.rs"));
   }
   ```

A case must build under `#![no_std]` (no `std::`, no heap, no external
crates), be deterministic, and avoid panicking on valid `u32` inputs (guard
division/modulo by zero, use `wrapping_*` for arithmetic). A native-side
panic is treated as a case failure.

## Coverage-guided case generation

The `cargo make` tasks below produce a Markdown report of which compiler
functions and regions are covered by the current case set. The intended
workflow is to hand `AGENT-PROMPT.md` to an agent and let it use the report
to pick new cases.

```bash
# First run (clean-slate baseline; takes several minutes for the
# instrumented compiler build).
cargo make fuzza-cov

# Each iteration (reuses the instrumented build; ~20-60s warm).
cargo make fuzza-cov-step

# Nuke all coverage state and start over.
cargo make fuzza-cov-clean
```

Outputs live under `target/fuzza-coverage/`:

- `report.md` — what humans and agents read.
- `report.prev.json` — previous snapshot, used to compute the "Delta since
  previous run" section in `report.md`.
- `html/html/index.html` — per-line highlighted source view for debugging
  which exact lines a case hits.

### Using AGENT-PROMPT.md

`AGENT-PROMPT.md` is a prompt template for an agent that grows coverage in a
specific compiler area. To use it:

1. Pick a compiler directory or file to target — e.g.
   `codegen/masm/src/emit/`, `frontend/wasm/src/code_translator/mod.rs`,
   `dialects/scf/`.
2. Run `cargo make fuzza-cov` (if you haven't yet) and skim
   `target/fuzza-coverage/report.md` to confirm the area has untouched
   functions whose signatures could plausibly be reached from a
   `(u32, u32) -> u32` no_std cdylib. Compiler code behind
   SDK/protocol/account/note scripts, the debug-engine UI, or anything
   gated on a `#[component]` attribute is **not reachable** from this
   harness — pick a different area if that's most of what shows up in the
   cold list.
3. Open `AGENT-PROMPT.md`, replace the `[area]` placeholder on the first
   line with the path you chose, and hand the file to the agent.
4. The agent iterates via `cargo make fuzza-cov-step`, reading the delta
   in each new `report.md` to decide whether each case was productive and
   when to stop.
