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
- Read `KNOWLEDGE.md` and the `#[ignore]`d tests in `tests.rs` first —
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

## Per-iteration subagent prompt skeleton

Compose a fresh prompt per iteration from these blocks (all of them — thin
prompts made agents re-derive known facts):

1. **Target area + context** — what it is, why now, what the last iteration
   learned that aims this one (e.g. a precise `File:line` target list).
2. **Step-0 reads** — `README.md`, `KNOWLEDGE.md`, `AGENT-PROMPT.md`,
   `tests.rs` + existing cases (including the `#[ignore]`d bug reproducers),
   the director journal, prior scratch logs.
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
   allowed file set (`cases/`, `tests.rs`, `scratch/`, `KNOWLEDGE.md`), style
   rules.
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
   files, `tests.rs`, scratch, and `KNOWLEDGE.md`; hunt for stray emit dumps.
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
