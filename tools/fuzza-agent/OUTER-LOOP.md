# fuzza outer loop — director playbook

`AGENT-PROMPT.md` is the *inner* loop: one agent grows coverage in one area.
This file is the *outer* loop: a director session that runs a whole campaign —
picks areas, launches one inner-loop agent per iteration, verifies and commits
each iteration's cases, and keeps durable knowledge flowing into
`KNOWLEDGE.md`. The shape below ran a six-area campaign on 2026-07-17
(15 kept cases, 3 compiler bugs and 1 executor blind spot found, every area
closed by an unreachability argument).

## Roles

- **Director** (the main session): owns the area queue, launches exactly one
  subagent per iteration, verifies results, commits, maintains the journal and
  `KNOWLEDGE.md`. The director is the only one who touches git.
- **Inner-loop subagent** (one per iteration): follows `AGENT-PROMPT.md` for a
  single area with the director's overrides; leaves its changes in the working
  tree; reports back in a structured form.

## Campaign setup

- Keep a director journal at the repo root as `work_log.md` (untracked, never
  committed). Record there, before iteration 1: the stop condition (e.g. "run
  the queue to exhaustion"), the divergence policy (ignore + pinned twin;
  root-cause now or defer), commit granularity (one commit per area), and the
  verification policy (per-iteration = the suite state from the subagent's
  `fuzza-cov-step` runs; one full `test-all` + clippy + format pass at campaign
  end). The journal is what survives context loss and what subagents read for
  cross-iteration state.
- Read `KNOWLEDGE.md` and the `#[ignore]`d tests in the differential test
  modules first —
  together they say what is known-unreachable, what is currently bug-blocked
  (each blocking test's comment names its unblock condition), and which shapes
  must not be re-reported. Prior campaign journals (`work_log.md` is
  untracked) record what earlier runs exhausted when present on this machine;
  otherwise a fresh baseline report shows the current state.

## Area queue heuristics

- Pick by *bug yield*, not by cold-region count: emulation-heavy surface
  (signed ops, wide integers, division), transform boundaries (spills,
  cfg-to-scf), and anything the corpus has never *executed* at runtime beat
  large-but-unreachable cold lists.
- Feed forward: every iteration's report names promising outside-area surface
  it warmed incidentally — queue it. (The scheduling/spills area was found
  because a calls-iteration case first activated a scheduling tactic.)
- Save already-well-exercised areas for late gap-check passes with a small
  case budget and a bias toward the unreachability exit.
- An area blocked by a known bug is a *re-run candidate*, not a dead area —
  the blocking test's comment names the unblock condition.
- When composing a brief from the report's cold tables, read the untouched
  AND partially-covered tables together: an untouched row for a generic
  function may be a unit-test-only or phantom `<_, _>` monomorph whose real
  instantiation sits warm in the partial table, and a cold dispatch ARM can
  be llvm-cov attribution noise — verify via its dedicated callee's coverage
  before making it an iteration's headline target.
- For gap-check iterations, tell the agent to extract the area's full
  per-function coverage from `report.json` directly (the rendered report
  caps each table at 30 rows) and to prefer compile-side probes
  (`fuzza-probe` wat/hir dumps) over paid coverage steps — a closure-heavy
  iteration can finish without a single `fuzza-cov-step`.
- When plain-Rust areas are exhausted, the next campaigns are *capability*
  campaigns: a harness lever (e.g. DWARF in guest builds, a compiler
  configuration via `MIDENC_DIFF_FLAGS`) flipped for the whole corpus,
  followed by a fallout sweep — every case is a differential probe of the
  new configuration for free, and any failure is a finding. Measure what
  the lever opened with a per-function *set diff* between two `report.json`
  snapshots (functions warm in one and cold in the other); the rendered
  delta's "newly-exercised functions" count is a test-crate-rebuild
  artifact and must not be used for that judgment. Copy each baseline's
  `report.json` aside right after producing it — `fuzza-cov-step` rotates
  it into `report.prev.json` and overwrites that on the next step, so the
  baseline is gone two steps later. Region gains attributed to functions a
  compile-panicking case unwinds through (e.g. `emit_if` arms) are llvm-cov
  counter-expression phantoms, not reachable code.
- Bug-directed campaigns (compass = findings, not regions) work as
  pressure ladders per fragile subsystem: parametric families climbed until
  they break, one passing boundary guard per ladder, every panic classified
  by signature against the known classes before it is reported. Follow a
  block of such campaigns with (a) a configuration sweep of the whole corpus
  (`MIDENC_DIFF_FLAGS=--optimize=max|size-min|basic`, `FUZZA_INPUT_PAIRS`,
  `FUZZA_GUEST_DEBUG=0`) — known classes move with the guest opt-level and
  new shapes surface only there — and (b) a program-scale campaign of
  realistic no_std programs with pinned grids and deep fuzz, which is where
  long-range pass interactions show up.
- Campaign types that paid off in the 2026-09-09 block (six campaigns, all
  plain Rust, zero runtime divergences, eleven realistic-program and
  minimal reproducers pinned): (1) *closure audits* — the knowledge base's
  "pattern X can never fire" arguments were wrong nine times out of
  fifteen once checked with the pattern-rewrite-driver trace; an audit
  campaign that traces every registered canonicalization over the corpus
  is cheap and turns closures into producers; (2) *compositions* — freight
  ladders from one campaign loaded onto the producers of another, with
  per-pass IR dumps (`-Z print-ir-after-pass=<pass>` plus
  `MIDENC_TRACE='pass:<pass>=trace'`) to prove the interaction happened
  before value-checking it; (3) *realistic programs written FOR the cliff
  shapes* — the ladders say which structured-control-flow shapes break at
  low pressure, and programs built around those shapes turn "synthetic
  panic" into "seven of twelve user programs do not compile", which is
  the evidence a fix priority needs; (4) a *workaround map* for the
  programs that do not compile (helper boundaries, by-value vs
  by-reference state, `black_box` on constants) — it produced both a user
  answer and a new panic site. For realistic programs the "known class
  = no new twin" rule is suspended: the ignored `prog_*` twin next to its
  largest compiling `_guard` IS the deliverable.
- Classify panics by trace, never by marker or crash site: erased
  split-edge reloads and "unused phi" warnings are present in programs
  that compute the right answer, the ">16 felts means a spill defect" rule
  of thumb does not hold at -Oz, and one crash site (`emit/mod.rs:623`)
  now has THREE mechanisms behind it. Precedence: `edges to split > 0` plus
  `erase unused reload` lines = the stale dominator tree; an arity-2 Copy
  `NoSolution` on an in-window stack with no split erasure = the solver
  gap; `additional spills required` with no splits and a spill store as the
  drop trace's last op = spill placement past the window; the pattern
  trace's last `trying to match` line = the aliasing panic. In the
  2026-09-10 block the inner-loop agents mislabeled three twins by site;
  the director's re-trace caught all three, so the re-trace is not
  optional.
- Arbitrate EVERY runtime divergence with wasmtime on the harness-built
  wasm (`wasmtime run -W wide-arithmetic=y --invoke entrypoint
  target/miden_test_shared/wasm32-wasip1/release/differential_<case>.wasm
  a b`): wasmtime agreeing with MASM is the guest toolchain, wasmtime
  agreeing with native is the compiler. Then the director owns the root
  cause: the one genuine compiler miscompile of the block (a constant
  folder retyping a shared constant) was invisible in every IR dump — the
  printer shows result types, not immediate variants — and was found by
  reading the MASM for a wrong-width push, bisecting the shape by hand,
  and reading the folder source. Budget an hour of director time for it;
  do not hand a live miscompile back to an agent for "classification".
- A *minimal-reproducer suite* is a worthwhile closing campaign: one
  smallest runnable case per crash site and mechanism, at the default
  configuration where possible, beside the sibling that differs by one
  ingredient, plus a per-class "must turn green / must stay green" test
  map — that is what a fix PR and an issue need, and the reductions
  themselves refine the producer rules (the aliasing panic turned out not
  to need an early exit at all).
- Configuration-dependent findings need a per-case pin, not only an env
  sweep: the `--guest-debug=0|1|2` harness pseudo-flag exists because the
  release configuration (no guest DWARF) changes which programs compile,
  and every ordinary build is on that side.
- A rebase onto a moved `next` is a campaign of its own (2026-09-17): run
  the whole suite first, then arbitrate every changed verdict by
  rebuilding the guests with the PREVIOUS toolchain
  (`RUSTUP_TOOLCHAIN=<old nightly> <test binary> <filter> --exact`; the
  harness's nested cargo honours it) — a case that passes that way is a
  toolchain-shape shift, not a compiler regression, and gets a
  toolchain-dated ignore reason rather than a new class. Then a
  *fix-uptake sweep*: every ignored reproducer re-run ALONE with
  `--ignored --exact` (a guest build failure exits the whole test process
  — `process::exit` in midenc-compile — so a batch loses every other
  result), un-ignored with its history in the doc comment when the fix
  landed, and every closed class's ladder pushed one rung to the next
  boundary.
- Oracle dimensions the strict corpus cannot see need a harness mode, not
  more cases: trap parity (`run_case_traps`, 2026-09-17) compares
  value-or-trap per input with a panic handler that traps on both targets
  and the host entrypoint in a forked child. Its first campaign found the
  Rust-panic side clean at every level and a real gap only outside the
  language oracle (Miden does not bounds-check linear memory) — expect the
  compiler-sensitive direction of such an oracle to be "traps where native
  returns", since LLVM compiles Rust's own guards into the guest.
- A *declared-effect audit* (2026-09-17) is the cheap half of a
  memory-ordering campaign: list every op implementing the effect
  interface with its declared effects (field-level attributes included —
  a struct-header grep misses them), mark what plain Rust can produce, and
  probe only what could understate; then prove each probe with the
  per-pass IR dump (`-Z print-ir-after-pass=<pass>` AND
  `MIDENC_TRACE='pass:<pass>=trace'`, with `--nocapture` — a passing test
  swallows both) before value-checking it.
- Inner-loop agents may stop only processes they spawned (never
  pattern kills); an agent that dies mid-run (usage limit) leaves unverified
  files — relaunch the same brief with a "reuse, verify, trim the partial
  work" section rather than starting over. Director-side verification of
  an agent's claims is cheap and worth doing every time: re-run every
  ignored twin with `--ignored --exact`, re-trace one pattern claim and
  one spills claim, and sweep the kept guards at the other opt-levels and
  without DWARF — the block above found one wrong cross-reference, one
  hazard note that the source refuted, and one workaround that only holds
  with DWARF on this way.

## Per-iteration subagent prompt skeleton

Compose a fresh prompt per iteration from these blocks (all of them — thin
prompts made agents re-derive known facts):

1. **Target area + context** — what it is, why now, what the last iteration
   learned that aims this one (e.g. a precise `File:line` target list).
2. **Step-0 reads** — `README.md`, `KNOWLEDGE.md`, `AGENT-PROMPT.md`, the
   test modules + existing cases (including the `#[ignore]`d bug
   reproducers), the director journal, prior scratch logs.
3. **Area resolution seed** — starter `FUZZA_AREA` paths plus the instruction
   to verify/refine them against the baseline report.
4. **Operational notes** — env-var prefixing, clean baseline command, long
   build timeouts, the step command, the report re-render trick, the scratch
   log path for this area.
5. **Constructs worth probing** — concrete Rust shapes with their reachability
   caveats, so the agent probes rather than guesses.
6. **Lessons carried over** — the relevant `KNOWLEDGE.md` facts restated
   briefly (probe method, known compile-breakers, LLVM pre-cleaning traps).
7. **Loop overrides** — case budget (8–12 worked well), divergence policy
   (ignore + pinned `<case>_repro` twin + split composite cases), compile-panic
   policy (unsupported-construct ⇒ delete; compiler panic on safe Rust ⇒ keep
   as `#[ignore]`d finding), zero-delta policy, **no git commands**, the
   allowed file set (`cases/`, the test modules under `tests/`, `scratch/`,
   `KNOWLEDGE.md`), style rules.
8. **Deliverable** — a structured result: final `FUZZA_AREA`, baseline/final
   area coverage, every case attempted (kept or deleted, with per-case
   deltas), divergences with exact inputs and error text, stop reason, the
   unreachability analysis, whether the final suite run was green, and notes
   for the director.

If the launching mechanism supports it, enforce the deliverable with a schema,
and make the launcher fail fast when the prompt fails to reach the agent — one
iteration of the 2026-07 campaign received no prompt and silently reconstructed
its task from the journal (it worked, but only because the journal existed).

## Processing an iteration

1. Verify the tree matches the report: `git status` should show only case
   files, test modules, scratch, and `KNOWLEDGE.md`; hunt for stray emit
   dumps.
2. Read the new cases against the constraints (no_std, determinism, guarded
   division, statics restored, signature ≤16 felts).
3. **Re-verify every claimed divergence yourself** by running its pinned twin
   before committing — the ignore reason must reproduce exactly.
4. Keep bug reproducers in-repo as `#[ignore]`d cases — compile-time compiler
   panics included; a repro that only lives in gitignored scratch is lost. The
   test's doc comment and ignore reason are the bug's only documentation —
   make sure they are self-sufficient (failure, exact inputs, bounding,
   un-ignore condition).
5. Commit once per area (`test(fuzza): ...`), body = what the cases cover, any
   divergence with its inputs, and the coverage delta.
6. Update the journal (outcome, lessons) and `KNOWLEDGE.md` (new durable
   facts), then launch the next iteration with those lessons folded into its
   prompt.

## Campaign wrap-up

- Re-check existing `#[ignore]`d cases against upstream fixes; un-ignore what
  now passes (keep `_repro` twins as regression guards).
- Fix any stale case doc-comments the iterations flagged.
- Run the full verification chain once: `cargo make format-rust`,
  `cargo make clippy`, `cargo make test-all`.
- Promote every remaining durable discovery from scratch logs into
  `KNOWLEDGE.md`, and summarize the campaign (areas, cases, findings) for
  whoever triages the bugs.
