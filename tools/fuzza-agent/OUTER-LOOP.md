# fuzza outer loop: director playbook

`AGENT-PROMPT.md` is the inner loop: one agent runs one campaign in one area.
This file is the outer loop: a director session that chooses campaigns,
launches one inner-loop agent per campaign, verifies and commits each result,
and keeps the fact base current (`KNOWLEDGE.md`, `PIPELINE-FACTS.md`, and
the issue tags on the `#[ignore]` attributes). Recipes and commands live in `KNOWLEDGE.md`; this file only
says who does what, in which order, and how to choose.

## Roles

- **Director** (the main session): owns the campaign queue, launches exactly
  one subagent per campaign, verifies every claim, commits, keeps the journal
  and the fact base. The director is the only one who touches git, and the
  only one who root-causes a live miscompile.
- **Inner-loop subagent** (one per campaign): follows `AGENT-PROMPT.md` with
  the director's brief; leaves its changes in the working tree; reports in
  the structured form the brief asks for. It never runs git and never kills
  processes it did not spawn.

## Campaign setup

- Keep a director journal at the repo root as `work_log.md` and a findings
  ledger as `findings.md` (both untracked, never committed). Before the first
  campaign record the stop condition, the divergence policy (ignore + pinned
  twin; root-cause now or defer), the commit granularity (one commit per
  campaign) and the verification policy. The journal is what survives
  context loss.
- Read the three fact files and the `#[ignore]`d tests first: together they
  say what is known-unreachable, what is bug-blocked and by which issue, and
  which shapes must not be re-reported.

## Choosing the next campaign

Heuristics:
- Pick by bug yield, not by cold-region count: emulation-heavy surface
  (signed ops, wide integers, division), transform boundaries (spills,
  cfg-to-scf, lifting) and anything the corpus has never executed at runtime
  beat large-but-unreachable cold lists.
- Feed forward: every report names outside-area surface it warmed
  incidentally; queue it.
- Save well-exercised areas for late gap-check passes with a small case
  budget and a bias toward the unreachability exit.
- An area blocked by a known class is a re-run candidate once the fix lands,
  not a dead area; its issue tag on the `#[ignore]` attributes lists the
  reproducers to un-ignore (`KNOWLEDGE.md`, "Corpus map on demand").

Campaign types, in the order they tend to pay off:
1. **Pressure ladders** per fragile subsystem: a parametric family climbed
   until it breaks, one passing boundary guard per ladder, every panic
   classified by trace before it is reported.
2. **Closure audit**: take every "this can never fire" claim in the fact
   base and check it with the relevant trace over the corpus; closures turn
   into producers more often than not.
3. **Compositions**: the freight of one campaign loaded onto the producers of
   another, with per-pass IR dumps proving the interaction happened before
   value-checking it.
4. **Realistic programs written for the cliff shapes**: they turn a synthetic
   panic into "N user programs do not compile", which is what a fix priority
   needs. For these the "known class means no new twin" rule is suspended:
   the ignored `prog_*` beside its largest compiling `_guard` is the
   deliverable.
5. **Workaround map** for the programs that do not compile: it yields a user
   answer and often a new panic site.
6. **Configuration sweep** of the whole corpus (every `--optimize` level,
   `FUZZA_GUEST_DEBUG=0`, deep input pairs) after a block of campaigns: known
   classes move with the level and some shapes surface only there.
7. **Minimal-reproducer suite** as a closing campaign: the smallest runnable
   case per crash site and mechanism beside the sibling that differs by one
   ingredient; the reductions refine the producer rules.
8. **Harness-mode campaign** when an oracle dimension the strict corpus
   cannot see needs a harness lever rather than more cases (trap parity,
   per-case flags, guest debug level); then a fallout sweep, since every
   existing case probes the new mode for free.
9. **Declared-effect audit** as the cheap half of a memory-ordering campaign:
   list every op's declared effects, probe only what could understate, prove
   each probe with a per-pass dump.
10. **Rebase as a campaign** when `next` has moved: run the whole suite,
    arbitrate every changed verdict with the previous guest toolchain, then a
    fix-uptake sweep (every ignored reproducer alone, un-ignored with its
    history when the fix landed, every closed ladder pushed one rung).

## Briefing a campaign

Write the brief as a file the agent reads first (`scratch/c<N>-prompt.md`)
and keep it complete; thin briefs make agents re-derive known facts. Blocks:
1. **Mission**: the mechanism under test, why now, what the director already
   verified in the source (with paths), and what counts as a finding.
2. **Step-0 reads**: `README.md`, `AGENT-PROMPT.md`, the three fact files,
   the relevant test modules and cases, the journal section for this block.
3. **Work items**: numbered, each with the shapes to build, the evidence to
   take (which trace or dump) and the pinned grid it needs.
4. **What to keep**: module and case naming, the `_edges`/`_repro`/`_guard`
   conventions, doc-comment content, which fact file gets which durable
   fact (bug detail stays at the test site).
5. **Operational notes**: the hard rules (no git, no pattern kills, one
   filter per invocation), the allowed file set, the scratch log path, the budget.
6. **Deliverable**: fixed section names, every claim marked verified or not.

## Processing a campaign

1. Check the tree against the report: only case files, the test module,
   `tests/mod.rs`, `scratch/` and the fact files changed.
2. Read the new cases against the constraints in `KNOWLEDGE.md` ("Writing
   cases"): plain Rust, `//` comments, signature at most 16 felts, no slice
   or `str` `==`, statics restored, deterministic.
3. Re-run every ignored twin alone (`--ignored --exact` with the full test
   path); the ignore text must reproduce verbatim.
4. Re-take one trace or dump per claimed mechanism (one pattern claim, one
   spills claim, one IR dump) with the recipes in `KNOWLEDGE.md`. Classify
   every compile-time panic by trace, never by crash site; agents mislabel
   twins by site often enough that this step is not optional.
5. Arbitrate every runtime divergence with wasmtime on the harness-built
   wasm. Then the director owns the root cause of a live miscompile: read
   the MASM and the pass source, budget an hour, and never hand it back to
   an agent for "classification".
6. Sweep the kept guards at the other optimization levels and without guest
   DWARF before calling them guards.
7. Commit once per campaign with explicit paths (`test(fuzza): ...`, body =
   what the cases cover and any divergence with its inputs). Never commit
   the journal, the ledger, the handoff note or `scratch/`.
8. Update the journal and the ledger; rewrite changed facts in place in the
   fact files (every fact keeps its `module::test` proof); fold the lessons
   into the next brief.
9. An agent that dies mid-run leaves unverified files: relaunch the same
   brief with a "reuse, verify, trim the partial work" section rather than
   starting over.

## Block wrap-up

- Run the block-end chain once: `cargo make format-rust`, `cargo make
  clippy`, `cargo make test-all`.
- Refresh the ledger's decisions for the user (what to file, what to push);
  the director never files, pushes or opens a PR on its own.
- Promote every remaining durable discovery from scratch logs into the fact
  files, rewriting in place, and run the completeness check of
  `KNOWLEDGE.md`, "Corpus map on demand" (every `#[ignore]` carries a tag).
- Summarize the block for whoever triages: campaigns, cases, findings with
  their reproducers, what moved between classes.
