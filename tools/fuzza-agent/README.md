# fuzza-agent

Coverage-guided case-generation harness for the Miden compiler's differential
fuzzing tests. The differential test harness itself lives in
`tests/integration/src/end_to_end/differential/`; this directory only holds
the agent-facing tooling that grows the case set:

- [`AGENT-PROMPT.md`](AGENT-PROMPT.md) — the inner-loop prompt: one agent
  grows coverage in one area.
- [`OUTER-LOOP.md`](OUTER-LOOP.md) — the director playbook for running many
  areas as a campaign.
- [`KNOWLEDGE.md`](KNOWLEDGE.md) — the committed fact base every run must read
  first: verified reachability facts, case-writing tricks, and operational
  gotchas.
- `cov.py` — renders the llvm-cov JSON into the agent-readable `report.md`.
- `scratch/` — per-run agent notes (gitignored, machine-local).

## How the differential tests work

Each case under `tests/integration/src/end_to_end/differential/cases/` is a
single `.rs` file containing only a
`#[unsafe(no_mangle)] pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32`
plus any helpers it needs. The harness prepends `#![no_std]` and a panic
handler, then for each case:

1. Builds it natively as a host `cdylib` and loads it with `libloading`.
2. Builds it via `cargo-miden` to a MASM package.
3. Runs both with 16 random `(u32, u32)` input pairs via proptest.
4. Asserts the outputs match.

A divergence is a likely compiler bug, and the case file itself is the
reproducer. Note that proptest shrinking is disabled, so the failing
`(input1, input2)` pair a case reports is the *first* random failure, not a
minimized one. To investigate a divergence, re-emit the intermediate
artifacts with `MIDENC_EMIT` (WAT/HIR/MASM) and trace VM execution with
`MIDENC_TRACE=executor=trace` — see the `emit` and `trace` skills.

### How coverage accumulates

`fuzza-cov-step` runs with `--no-clean`, so llvm-cov **merges** each run's
profile data into the previous run's rather than resetting it. Consequences:

- The headline `Regions covered` is cumulative across every step since the
  last clean; the "Delta since previous run" section is the per-step change.
- A failing (`#[ignore]`d) case still contributes the compile-side coverage it
  reached before diverging — a failing case is not wasted work.
- Deleting a case's `#[test]` does **not** remove its regions from the report
  until the next `cargo make fuzza-cov-clean`.

## Running the tests

```bash
cargo test -p midenc-integration-tests differential
# or
cargo nextest run -p midenc-integration-tests -E 'test(/differential::/)'
```

Cases known to surface compiler bugs are marked `#[ignore = "..."]` in
`tests/integration/src/end_to_end/differential/tests.rs` with the reason and
the exact failing inputs in the attribute. Run them explicitly with
`--ignored` (or `--run-ignored all` under nextest) to investigate. Ignored
cases come in three flavors:

- **Runtime divergences** — each has a deterministic `<case>_repro` twin that
  pins the exact failing input pair via `run_case_with_inputs`, so the bug
  fails reliably rather than only when proptest happens to draw the input.
- **Compile-time compiler panics** — the case *is* the reproducer; the panic
  message and source location are in the ignore reason.
- **Out-of-scope artifacts** — kept for coverage/documentation (e.g.
  `mem_grow`); the ignore reason explains why they will not be "fixed".

The specifics of each bug — the failure, the exact inputs, what passing
sibling cases have *bounded*, and what would allow un-ignoring — live only
with the tests themselves (doc comments and ignore reasons in `tests.rs`), so
fixing a bug means cleaning up at the test site alone.

## Adding a case manually

1. Create `tests/integration/src/end_to_end/differential/cases/case_<name>.rs`
   with the `entrypoint` function.
2. Wire it up in `tests/integration/src/end_to_end/differential/tests.rs`:
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

One determinism subtlety: the native `cdylib` is loaded **once** and reused
across all 16 proptest inputs, so a mutable `static` that a case writes to
carries state between invocations. Restore any static you mutate before
returning, or the case will be flaky.

## Coverage-guided case generation

The `cargo make` tasks below produce a Markdown report (via `cov.py`) of
which compiler functions and regions are covered by the current case set.
The intended workflow is to hand `AGENT-PROMPT.md` to an agent and let it
use the report to pick new cases.

```bash
# Optional: scope the report to a target area. FUZZA_AREA is a comma-separated
# list of workspace-relative path prefixes; an agent driven by AGENT-PROMPT.md
# derives these from a free-worded area (e.g. "memory read/write") as its first
# step. Export it in a persistent shell — or, in a fresh-shell-per-command
# agent, prefix each invocation (FUZZA_AREA='...' cargo make fuzza-cov-step).
export FUZZA_AREA='codegen/masm/src/emit/mem.rs,dialects/wasm/src/mem.rs'

# First run (clean-slate baseline; takes several minutes for the
# instrumented compiler build).
cargo make fuzza-cov

# Each iteration (reuses the instrumented build; ~20-60s warm).
cargo make fuzza-cov-step

# Cheap reachability probe: run ONE test outside llvm-cov with an IR dump
# (MIDENC_EMIT) and list the freshly-written artifacts. Wire the candidate
# case with a temporary #[ignore] first — the probe runs with
# --include-ignored.
cargo make fuzza-probe <test-name> hir

# Nuke all coverage state and start over.
cargo make fuzza-cov-clean
```

When `FUZZA_AREA` is set, `report.md` gains a **"Target area"** section: an
area-only `Regions covered` headline, an `Area delta since previous run` line,
and the full list of untouched / partially-covered functions in the area. This
is the section an area-focused agent should drive from — it removes the need to
hand-filter the global tables by `File:line`.

`report.md` can also be re-rendered with different `--area` scoping without
rerunning any tests:

```bash
python3 tools/fuzza-agent/cov.py target/fuzza-coverage/report.json . \
    --prev target/fuzza-coverage/report.prev.json --area '<paths>' \
    > target/fuzza-coverage/report.md
```

Outputs live under `target/fuzza-coverage/`:

- `report.md` — what humans and agents read.
- `report.prev.json` — previous snapshot, used to compute the "Delta since
  previous run" section in `report.md`.
- `html/html/index.html` — per-line highlighted source view for debugging
  which exact lines a case hits.

`cov.py` drops boilerplate trait impls (`fmt`, `clone`, `From`/`Into`, …) from
the report via `BORING_NAME_RE`, so don't be surprised when such a function
never appears in the cold lists even though it's uncovered. If your target area
is one where those matter (parsing, attribute handling), edit that regex.

Agents writing scratch notes should put them under `tools/fuzza-agent/scratch/`
(see [`AGENT-PROMPT.md`](AGENT-PROMPT.md)). That directory lives outside
`target/`, so it survives `fuzza-cov-clean`, and its contents are gitignored —
which also means they are machine-local and invisible to future clones.
Durable discoveries (reachability facts, verified dead ends, bugs found, an
area's closure argument) belong in [`KNOWLEDGE.md`](KNOWLEDGE.md), which is
committed and is required reading for every future run.

### Using AGENT-PROMPT.md

`AGENT-PROMPT.md` is a prompt template for an agent that grows coverage in a
specific compiler area. To use it:

1. Describe the area in plain words — e.g. `memory read/write`, `control flow`,
   `u64 arithmetic`. You don't need to know the file layout; the agent resolves
   the phrase to concrete source paths as its first step.
2. Open `AGENT-PROMPT.md`, replace the `[area]` placeholder on the first line
   with your description, and hand the file to the agent.
3. The agent resolves the phrase to source paths, exports them as `FUZZA_AREA`,
   and runs `cargo make fuzza-cov`. It then sanity-checks that the "Target area"
   section of `report.md` lists the functions you'd expect — and that they're
   plausibly reachable from a `(u32, u32) -> u32` no_std cdylib. Compiler code
   behind SDK/protocol/account/note scripts, the debug-engine UI, or anything
   gated on a `#[component]` attribute is **not reachable** from this harness;
   if the area is mostly that, it's a poor target.
4. The agent iterates via `cargo make fuzza-cov-step`, reading the "Target
   area" delta in each new `report.md` to decide whether each case was
   productive and when to stop.

To run several areas as a campaign — a director session that picks areas,
launches one `AGENT-PROMPT.md` agent per area, verifies and commits each
iteration's cases, and accumulates knowledge across iterations — see
[`OUTER-LOOP.md`](OUTER-LOOP.md).
