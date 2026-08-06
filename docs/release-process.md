# Release Process

The operational guide for releasing the compiler, the Rust SDK, and the project
templates. Follow the checklist for the kind of release you wish to perform.

Three things are true throughout and explain most of the procedure:

- **Publication to crates.io cannot be undone.** There is no rollback, and a
  version can never be reused. Everything reversible therefore happens before
  anything irreversible, and the approval gate sits exactly on that boundary.
- **A release is resumable.** Every attempt reconciles against live state first,
  so a run that dies partway through can be re-run and will do only what is
  missing. A first attempt and a resume are the same run.
- **Versions are chosen by a person.** The tooling validates a version; it never
  picks one.

## Which checklist do I want?

| I want to… | Go to | Touches production? |
| --- | --- | --- |
| Release for real | [§3 Production release](#3-production-release) | Yes, after the approval gate |
| Try the publish path with no risk, on my machine | [§4 Local rehearsal](#4-local-rehearsal) | No |
| Try the real workflow on real runners before committing | [§5 Hosted rehearsal](#5-hosted-rehearsal) | No crates, no tags - but real drafts and real attestations |
| Ship something consumers cannot accidentally pick up | [§6 Prereleases](#6-prereleases) | Yes, but invisible to default requirements |

Every step below says what to expect and what to do if it fails. Failure modes
link to [§7 Troubleshooting](#7-troubleshooting).

---

## 1. Branches

| Branch | Role |
| --- | --- |
| `next` | Development. All ordinary work merges here. |
| `main` | Releases. Only ever released from here. |

Releases happen from `main`, and `main` lags `next`. A release therefore starts
by promoting `next` into `main`, and the release candidate is then branched from
and merged into **`main`** - not `next`.

The release workflow enforces this and will refuse to run otherwise: the commit
it releases must be `main`'s tip *and* must be the most recent commit to have
changed `.release/release.toml`.

---

## 2. Prerequisites

One-time setup. Requires repository-admin and crates.io-owner access; none of it
can be performed by the tooling.

### 2.1 crates.io

| What | Detail |
| --- | --- |
| Trusted Publisher, per crate | Repository `0xMiden/compiler`, workflow `release.yml`, environment `release` |
| Trusted-Publishing-only mode | Enable per crate **after** its publisher is verified working |
| New crates | crates.io cannot configure a publisher for a crate that does not exist. A brand-new crate must be bootstrapped once with a short-lived, narrowly scoped token, then switched to Trusted Publishing |
| Long-lived tokens | Remove `CARGO_REGISTRY_TOKEN` from repository secrets once Trusted Publishing works |

There are currently **33 publishable crates** (`cargo make package-order` lists them). Each needs its own publisher configuration.

> **Known limitation.** A crate whose publisher is missing or misconfigured
> cannot be detected in advance: the token crates.io issues carries no list of
> the crates it authorizes. The failure surfaces mid-publication as a 403. It is
> survivable - see [T7](#t7-403-from-cratesio-during-publication).

### 2.2 GitHub repository settings

| What | Detail |
| --- | --- |
| Immutable releases | Enable for the repository |
| Environment `release` | Required reviewers; self-review disabled; admin bypass disabled where supported; deployment restricted to protected `main` |
| Tag rulesets for `v*`, `sdk/v*`, `templates/v*` | **Restrict deletions and updates only. Do not restrict creations.** |
| Required status check | `release / gate` |
| CODEOWNERS | Release infrastructure paths reviewed by a release owner |

> **Why creations are not restricted.** Ruleset bypass actors are roles, teams,
> apps, and deploy keys - there is no workflow-level actor. `GITHUB_TOKEN` acts
> as the repository-wide GitHub Actions app, so a creation bypass would grant
> *every* workflow the ability to create release tags, which is worse than not
> restricting creation at all. The property that matters is that a published tag
> can never be moved or removed, and deletion/update restrictions give exactly
> that.

### 2.3 Repository state

- Every workspace package classified in `.release/config.toml`
  (`cargo make release lint` enforces this).
- No active `[patch]` entries in the root manifest.
- All first-party external dependencies (miden-vm, protocol, and related
  repositories) published at the versions the workspace requires.

---

## 3. Production release

### Phase A - Prepare the candidate

Everything in Phase A happens on a branch and in pull requests. Nothing here
touches crates.io, creates a tag, or publishes anything.

---

**A1. Promote `next` into `main`.**

Open a pull request from `next` to `main`, titled for the release, and merge it
once CI passes. Skip only if `main` is already at the commit you intend to
release.

*Expect:* `main` contains everything going into this release. Nothing else will
be allowed to land until the release finishes.

*If it fails:* ordinary CI failure - fix on `next` and repeat.

---

**A2. Freeze `main`.**

Tell the team that nothing may merge to `main` until the release completes. The
workflow requires the release commit to be `main`'s tip, and anything landing
after your candidate invalidates it ([T5](#t5-subject-is-not-mains-tip)).

---

**A3. Branch from `main`.**

```bash
git fetch origin
git switch --detach origin/main
git switch -c release/v0.10.0
```

The candidate branch must be based on `main`, not `next`.

---

**A4. Choose the units and versions.**

A judgement call: which of compiler, SDK, and templates are being released, and
whether this is a prerelease. If a compiler change requires a template change,
the templates must be released with it.

---

**A5. Move the versions.** Once per unit being released:

```bash
cargo make release set-version --unit sdk 0.14.0
cargo make release set-version --unit compiler 0.10.0
```

Omit the version to automatically bump to the next minor. Add `--dry-run` first to review the edits.

*Expect:* modifications to the crate manifests, every requirement naming them,
`Cargo.lock`, `.release/release.toml`, and - for an SDK bump - the template
manifests and `extra/templates/bundle.toml`.

> **Always use `set-version`; never hand-edit versions.** An SDK bump has to
> rewrite the `miden` requirement in `bundle.toml` and in every template
> manifest, and it has to pick the *right form* of that requirement: a minor
> requirement for a stable release, the exact version for a prerelease, because
> a caret requirement never matches a prerelease. Editing manifests by hand
> skips all of it and produces templates that cannot resolve the SDK they ship
> beside ([T14](#t14-the-templates-cannot-resolve-the-sdk-being-released)).
>
> To change your mind about a version you have already bumped - releasing
> `0.32.0-rc.1` after bumping to `0.32.0`, say - re-run it with `--force`.
> SemVer orders a prerelease *below* its release, so this is a backwards move
> and refused by default. Forcing is safe while nothing has been published;
> once a version is on crates.io it can never be reused, flag or no flag.

*If it fails:* [T1](#t1-set-version-reports-disagreeing-versions).

---

**A6. Write the changelog.**

```bash
cargo make release changelog-prompt compiler --version 0.10.0
```

This emits a *prompt*; it never writes entries. Pass it to an agent if you like,
then review and edit what comes back. Edit the changelog for each unit being
released:

| Unit | File |
| --- | --- |
| compiler | `CHANGELOG.md` |
| sdk | `sdk/CHANGELOG.md` |
| templates | `extra/templates/CHANGELOG.md` |

The range defaults to the unit's last release tag through `HEAD`, filtered to
the paths that unit publishes. Override it with a second argument:

```bash
cargo make release changelog-prompt sdk v0.9.2..HEAD
```

> **First SDK and template releases.** The `sdk/v*` and `templates/v*` tag
> namespaces are new, so until each has been released once there is no baseline
> and the default range is the unit's entire history - hundreds of commits.
> Pass a range explicitly for those.

---

**A7. Regenerate the template bundle** - only if this release includes templates
or any template file changed:

```bash
cargo make release bundle --output tools/cargo-miden/templates.tar.gz
```

*If it fails, or if the gate later reports bundle drift:*
[T2](#t2-the-embedded-template-bundle-is-stale).

---

**A8. Check it locally before pushing.**

```bash
cargo make release lint
```

*Expect:* `release lint: N packages classified, no findings`.

*If it reports findings:* [T2](#t2-the-embedded-template-bundle-is-stale),
[T3](#t3-release-lint-findings).

---

**A9. Open the release-candidate pull request into `main`,** containing the
version, lockfile, changelog, `.release/release.toml`, and (if applicable)
template bundle changes.

> **You must click "Approve and run" on the pull request.** GitHub does not
> start workflow runs for pull requests opened by `GITHUB_TOKEN`, and pushes to
> such a branch create no runs at all. Since `release / gate` is a required
> check, the candidate cannot merge until you do.

*Expect:* the `release / gate` check runs. It takes roughly 8 minutes when the
package closure is in scope, which it will be for any candidate.

*If the gate fails:* [T3](#t3-release-lint-findings),
[T4](#t4-package-closure-verification-fails).

---

**A10. Review and merge** once the gate passes. Use a merge that makes the
candidate `main`'s tip.

*Expect:* `main`'s tip is your candidate commit, and it is the most recent
commit to have touched `.release/release.toml`. The workflow checks both.

---

### Phase B - Plan

**B1. Dispatch the release workflow from `main`.**

From the CLI:

```bash
gh workflow run release.yml --ref main
```

Or in the browser: **Actions** -> **release** (left sidebar) -> **Run workflow** ->
set *Use workflow from* to **main** -> **Run workflow**. There are no inputs; the
scope comes from the reviewed `.release/release.toml` and nothing else.

*Expect:* a new run appears under Actions within a few seconds. Watch it with:

```bash
gh run watch "$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

*If the run fails immediately in the `plan` job:*
[T5](#t5-subject-is-not-mains-tip),
[T6](#t6-the-release-declaration-was-changed-in-a-different-commit).

---

**B2. Review the intent.**

Open the run -> the **plan** job -> the **Generate the intent** step. It prints
the full intent: the units being released, their versions, whether each is a
prerelease, the crates in each stage, and the tags that will be created. The
same content is attached to the run as the `release-intent` artifact.

Read it and confirm the versions and units are what you decided in A4.

*If anything is unexpected:* cancel the run. Nothing irreversible has happened.
Fix it in a new candidate and start again from A3.

---

### Phase C - Build and stage

Fully automated. Nothing to do but watch. Automation verifies the release
configuration and package closure, seals the plan, builds and attests the six
executables, builds the template bundle, creates draft releases, uploads every
asset, and reads each one back.

*Expect:* the `verify`, `stage`, `artifacts`, `attest`, and `stage-artifacts`
jobs succeed, and draft releases appear under **Releases** in the repository.
They are drafts - not visible to anyone without write access, and deletable.

*Still reversible here:* no tag exists, no crate is published, and every draft
can be deleted.

> **One exception: attestations are permanent.** They are recorded in a public
> transparency log during this phase, so a cancelled release leaves provenance
> for artifacts that were never released. This is harmless - nothing consumes an
> attestation for an artifact with no release - but it cannot be undone.

*If a job fails:* [T8](#t8-an-asset-does-not-match-what-was-uploaded), or treat
it as an ordinary build failure. Either way, nothing is published; fix it in a
new candidate.

---

### Phase D - Approve and publish

**D1. Approve the deployment.** This is the point of no return.

The `publish` job pauses and the run shows a yellow **Review deployments**
button near the top of the run page. Required reviewers also receive a
notification.

Before approving, check on the run page:

- the intent from B2 is still what you want,
- the draft releases exist and carry the expected assets,
- no earlier job reported a warning you have not read.

Then: **Review deployments** -> tick **release** -> **Approve and deploy**.

> You cannot approve your own deployment if self-review is disabled, which it
> should be. Another maintainer with the reviewer role must click it.

*If you decide not to proceed:* do not approve. Cancel the run and follow
[§5.3 Cleaning up](#53-cleaning-up) to delete the drafts. Nothing will have been
published.

---

**D2. Publication runs.** Per unit, in dependency order (SDK -> templates ->
compiler): create that unit's tag, obtain a short-lived Trusted Publishing
token, reconcile against crates.io, publish only what is absent, and verify the
result from the registry.

*Expect:* the `publish` job succeeds and the tags appear under **Tags**. The
`release-journal` artifact records every decision.

> **The token lives 30 minutes.** Cargo publishes in dependency waves and waits
> for index confirmation between them - roughly a dozen waves for 33 crates.

*If it fails:* [T7](#t7-403-from-cratesio-during-publication),
[T9](#t9-a-tag-already-exists-at-a-different-commit),
[T10](#t10-a-planned-version-conflicts-or-is-yanked),
[T11](#t11-the-publishing-token-expired-mid-stage).

---

### Phase E - Finalize

**E1. Finalization runs.** Automation verifies every staged draft and then
publishes them, in order: SDK, templates, compiler. Publication makes a release
immutable, so *all* units are verified before *any* is published.

Per unit, before anything is published:

- The tag exists and points at the released commit. Read from the tag **ref**,
  not the release's `target_commitish` - GitHub stops honouring that field once
  the tag exists, so it reports what was requested rather than what is true.
- The draft carries this run's sealed plan, compared by bytes, so finalizing
  another run's draft is caught.
- Every asset still hashes to what `SHA256SUMS` recorded at staging.
- Nothing unplanned is attached, because after publication it can never be
  removed.

Only a **stable compiler** release becomes the repository's "Latest release".
The SDK, the template bundle, and every prerelease publish with
`make_latest=false`.

*If it fails:* [T9](#t9-a-tag-already-exists-at-a-different-commit),
[T8](#t8-an-asset-does-not-match-what-was-uploaded),
[T12](#t12-finalization-reports-an-unplanned-asset).

---

**E2. Review the final report.**

Open the run -> go to the **finalize** job -> go to the **Verify and publish the drafts** step. It lists each tag, whether it was newly published or already published, and which one claimed the "latest" slot, ending with a count.

Then confirm, outside the tooling:

- the releases appear under **Releases** and are no longer drafts,
- the crates appear on crates.io at the expected versions,
- `cargo install cargo-miden` (or the equivalent for what you released) resolves
  the new version.

---

**E3. Unfreeze `main`** and handle announcements and downstream coordination.

---

## 4. Local rehearsal

Exercises packaging and the publish path on your machine, against a local
registry that serves crates published to it and proxies crates.io for
everything else. Nothing reaches crates.io or GitHub.

**Requires:** versions that are *not* already published. The registry proxies
crates.io, so an already-published version is visible and Cargo will refuse to
republish it. Do this on a candidate branch after A5.

```bash
# 1. Start the local registry. Leave this running in its own terminal.
#    --cache-dir keeps upstream index lookups between runs; without it, every
#    rehearsal re-fetches the third-party closure.
cargo make release fake-registry --port 8732 --cache-dir /tmp/rehearsal-cache
```

In a second terminal, from the candidate workspace:

```bash
# 2. Prove the packaged crates are usable. This packages every selected crate,
#    publishes it to a throwaway registry, and builds a consumer that resolves
#    ONLY through that registry -- which is what proves the archives contain
#    everything they need. Production publishes with --no-verify, so this is
#    the only thing that checks it. Takes several minutes.
cargo make release verify-closure

# 3. Generate the intent: what would be released, at what versions, with what
#    tags. Reads .release/release.toml; writes intent.json.
cargo make release plan --subject "$(git rev-parse HEAD)" --output intent.json

# 4. Seal the intent: package every crate and record its exact digest and size
#    into the plan, so what gets published is pinned to what was inspected.
cargo make release seal --intent intent.json --output plan.json \
    --cache-dir /tmp/rehearsal-cache

# 5. Publish the sealed plan to the local registry, stage by stage, reconciling
#    and verifying each stage from the registry before starting the next.
cargo make release publish --plan plan.json \
    --rehearsal-index sparse+http://127.0.0.1:8732/
```

Step 2 is separate on purpose: `seal` packages the crates but does not build a
consumer against them. `verify-closure` is the step that proves they are
usable, and it is what the pull-request gate runs.

*Expect:* step 5 reports each stage publishing and then verifying, ending with
every planned crate published to the local registry.

**A local rehearsal creates no tags.** There is no throwaway GitHub, so a tag
created here would be a real, permanent tag naming a version that was never
released. The publish path is given a GitHub client that refuses every call, so
tagging is the one part of Phase D a local rehearsal cannot exercise.

*If it fails:* [T4](#t4-package-closure-verification-fails),
[T13](#t13-crate-already-exists-during-a-local-rehearsal).

---

## 5. Hosted rehearsal

### 5.1 What it is

**A hosted rehearsal is a production run that you stop at the approval gate.**
There is no separate rehearsal workflow. The design puts everything reversible
before the gate, so every step up to it *is* the rehearsal:

| Phase | Runs in a hosted rehearsal? | Reversible? |
| --- | --- | --- |
| A - candidate | Yes | Yes |
| B - plan | Yes | Yes |
| C - build, attest, stage drafts | Yes | Yes, except attestations |
| **approval gate** | **You do not approve** | - |
| D - tag and publish | No | - |
| E - finalize | No | - |

This is the only way to exercise the real GitHub path - draft creation, asset
upload and readback, real runners, real artifacts - which has never run against
real GitHub. Treat that as the point of the exercise.

### 5.2 Doing it

Follow **Phase A and Phase B exactly as written** in §3, then let Phase C run.

At **D1, do not approve.** That is the only difference between a hosted
rehearsal and a production release.

*Expect, when Phase C completes:*

- draft releases under **Releases**, carrying the executables, the template
  bundle, `SHA256SUMS`, and the sealed plan,
- build attestations for the six executables,
- **no tags**, and **nothing on crates.io**.

*What it still does not prove:* that crates.io accepts the upload, that Trusted
Publishing is configured per crate, production rate limits, tag creation, and
immutable finalization. The first three are only provable by a real publish; the
last two by a real release. A prerelease (§6) is the intended way to exercise
them.

### 5.3 Cleaning up

Cancel the run (it will sit waiting for approval indefinitely), then delete the
drafts. Either delete each draft in the **Releases** UI, or:

```bash
# Download the sealed plan from the run's artifacts, then:
GITHUB_TOKEN=$(gh auth token) cargo make release discard --plan plan.json
```

`discard` deletes only *still-draft* releases and never touches a published one.

*Leave behind:* the attestations, which cannot be withdrawn. This is expected
and harmless.

---

## 6. Prereleases

A prerelease is selected by giving a version with a prerelease identifier, e.g.
`0.10.0-rc.1`, at step A5. It is the intended first exercise of the full
production path, because **a prerelease cannot disturb existing consumers**: a
default requirement such as `midenc = "0.9"` never matches a prerelease.

Follow §3 unchanged. The differences are automatic:

- GitHub releases are marked prerelease and never become "Latest".
- **Templates can be part of a prerelease**, and must be when a compiler change
  requires a template change. The bundle takes a prerelease version and stable
  clients never see it - template resolution selects the highest *stable*
  compatible release, so a prerelease bundle is reachable only from a prerelease
  `cargo-miden` or an explicit `--template-version`.

---

## 7. Troubleshooting

### T1: `set-version` reports disagreeing versions

**Symptom:** `set-version` refuses, reporting that packages in a version domain
are not all at the same version.

**Cause:** every crate in a unit shares one version, and one has drifted -
usually hand-edited.

**Remedy:** set every crate in that unit to the same version manually, then
re-run `set-version`. `cargo make release-lint` will confirm.

---

### T2: The embedded template bundle is stale

**Symptom:** lint reports `the embedded template bundle is stale: … has sha256
X, but the sources produce Y`.

**Cause:** `tools/cargo-miden/templates.tar.gz` no longer matches the template
sources under `extra/templates`.

**Remedy:**

```bash
cargo make release bundle --output tools/cargo-miden/templates.tar.gz
```

Commit the result.

**If the error also lists untracked files:** those files are in your working
tree but not in git, so they are not in the bundle, and the archive you build
locally will differ from the one CI builds. Commit them (`git add -f` if
something is ignoring them) or delete them. This is a real cause of "it works on
my machine": the bundle is built from tracked files precisely so it depends on
the commit and not on your checkout.

---

### T3: `release-lint` findings

**Symptom:** the `release / gate` check fails in the `release lint` job.

| Finding | Cause | Remedy |
| --- | --- | --- |
| `package '…' is not classified` | A new workspace member | Add it to `.release/config.toml` with an explicit unit and `publish` setting |
| `… is publishable in its manifest but classified as private` (or the reverse) | The manifest and the config disagree | Make them agree; the config is the policy, the manifest is what Cargo obeys |
| `private package '…' is at X but private packages are frozen at 0.1.0` | A private crate's version moved | Set it back to `0.1.0`; private crates are never published, so a version tracking a release domain is misleading |
| `publishable package '…' depends on private package '…'` | A published crate would be unresolvable for consumers | Either publish the dependency or remove the dependency |
| `an active [patch] entry` | The root manifest patches a dependency | Comment it out. This is the most likely way to publish a broken crate: the workspace builds, the manifest looks right, and the published crate resolves to a registry version without the patched behaviour |

---

### T4: Package closure verification fails

**Symptom:** the `package closure` job fails, reporting either a packaging
failure or `the packaged crates do not build when resolved from a registry`.

**Cause:** a packaged crate is missing something it needs - a file excluded from
the archive, a dependency that only resolves by workspace path, or an active
`[patch]`. Production publishes with `--no-verify`, so this check is the only
thing standing between that and a broken published crate.

**Remedy:** reproduce locally with `cargo make verify-closure` and read the
build error. Common causes: a file needed at build time not covered by the
package's `include`, or a dependency without a version requirement (path-only
dependencies are stripped when packaging).

---

### T5: Subject is not `main`'s tip

**Symptom:** the `plan` job fails with `subject … is not main's tip (…); refresh
the candidate`.

**Cause:** something merged to `main` after your candidate.

**Remedy:** you cannot release this commit. Rebase or re-land the candidate so
it is `main`'s tip again - in practice, open a fresh candidate from the current
`main` (A3) - and re-dispatch. Ensure the freeze from A2 is actually being
observed.

---

### T6: The release declaration was changed in a different commit

**Symptom:** the `plan` job fails with `the release declaration was last changed
in <sha>, not <sha>; something landed on main after the candidate merged`.

**Cause:** `.release/release.toml` was not last modified by the commit being
released. Either something landed afterwards, or the candidate did not actually
change the declaration.

**Remedy:** confirm `set-version` was run and its `.release/release.toml` change
was committed, then open a fresh candidate from the current `main`.

---

### T7: 403 from crates.io during publication

**Symptom:** the `publish` job fails partway through a stage with a 403 naming a
crate.

**Cause:** that crate has no Trusted Publisher configured, or its configuration
does not match this repository, workflow, and the `release` environment. This
cannot be detected in advance - the token carries no list of the crates it
authorizes.

**Remedy:**

1. Fix the publisher configuration on crates.io for the named crate (§2.1).
2. Re-dispatch the workflow (B1). Reconciliation publishes only what is missing,
   so already-published crates are skipped.

Do **not** change versions. Everything already published stays.

---

### T8: An asset does not match what was uploaded

**Symptom:** staging or finalization fails with `does not match what was
uploaded` or `hashes to … but was staged as …`.

**Cause:** an asset's bytes changed between upload and readback, or between
staging and finalization.

**Remedy:** do not publish. Cancel the run, delete the drafts (§5.3), and
re-dispatch. If it recurs with the same asset, treat it as an integrity
incident and investigate before releasing anything.

---

### T9: A tag already exists at a different commit

**Symptom:** `tag '…' already exists at <sha>, not <sha>; it cannot be moved or
deleted, so this version must be abandoned and a new one released`, or during
finalization, `points at … not at the subject`.

**Cause:** the tag was created by an earlier attempt at a different commit, or
by something else entirely. Tag rulesets prevent moving or deleting it, by
design.

**Remedy:** that version is burnt. You cannot recover it.

1. Do not attempt to move or delete the tag.
2. Delete any still-draft releases for the plan (§5.3).
3. Open a new candidate with a **new version** (A3) and release that. The
   leftover tag is accepted as debris.

---

### T10: A planned version conflicts or is yanked

**Symptom:** the `publish` job fails with `stage '…' has N conflict(s)`.

**Cause:** a planned version already exists on crates.io with *different* bytes,
or has been yanked. A yanked version can never be republished.

**Remedy:** the affected versions cannot be published as planned. Open a new
candidate with new versions. If you need to yank something as part of an
incident, finish or abandon the in-flight release first - yanking a planned
version mid-release strands it, because versions cannot change during a resume.

---

### T11: The publishing token expired mid-stage

**Symptom:** the `publish` job fails partway through a stage with an
authentication error, after roughly 30 minutes in that stage.

**Cause:** Trusted Publishing tokens live 30 minutes. Cargo publishes in
dependency waves and waits for index confirmation between them.

**Remedy:** re-dispatch (B1). A fresh token is obtained per stage, and
reconciliation resumes from what is already published. If a single stage
repeatedly cannot finish inside the budget, it needs splitting - raise it rather
than retrying indefinitely.

---

### T12: Finalization reports an unplanned asset

**Symptom:** `the draft for '…' carries N asset(s) the plan does not describe`.

**Cause:** something was attached to a draft that the release plan does not
account for.

**Remedy:** do not publish - after publication an asset can never be removed.
Establish where it came from. If it was attached by hand, remove it from the
draft and re-dispatch. If you cannot account for it, treat it as an intrusion
and stop.

---

### T13: `crate already exists` during a local rehearsal

**Symptom:** `error: crate <name>@<version> already exists on … index`.

**Cause:** the version you are rehearsing is already published on crates.io, and
the local registry proxies crates.io for anything it has not been told it owns.

**Remedy:** rehearse with unpublished versions - run A5 first so the workspace
carries the versions you intend to release.

---

### T14: The templates cannot resolve the SDK being released

**Symptom:** lint reports `the templates require \`miden = "X"\`, which cannot
resolve the SDK version this release publishes (Y)`.

**Cause:** the templates' `miden` requirement cannot match the SDK version in
`.release/release.toml`. Almost always a hand-edited version: a prerelease needs
the exact version, because `"0.14"` never matches `0.14.0-rc.1`, and a stable
release wants the minor so a later patch needs no template change.

**Why it matters:** nothing else catches it. `bundle.toml` and the template
manifests agree with each other, so they look consistent - but a project
generated from those templates cannot resolve `miden` at all, and if the SDK
version is a prerelease with no stable counterpart, it will not build.

**Remedy:** re-run the bump rather than editing by hand, then regenerate the
bundle because its contents changed:

```bash
cargo make release set-version --unit sdk 0.14.0-rc.1 --force
cargo make release bundle --output tools/cargo-miden/templates.tar.gz
```

`--force` is needed when the version is not moving forward, which includes
re-applying the version already in place.

---

## 8. What is not yet exercised, and what is missing

### The largest unvalidated risk

**Nothing here has run against real GitHub.** Tag creation, draft creation,
asset upload and readback, and finalization are exercised only against an
in-memory double and a local stub HTTP server. The wire format has been tested;
GitHub's acceptance of it has not. A hosted rehearsal (§5) is what validates
this, and it should be treated as the point of the exercise rather than a
formality.

The same applies to crates.io: Trusted Publishing is configured, but no upload
has been attempted through it.

### Not implemented

- **`audit-publishers`.** Trusted Publishing configuration cannot be
  preflighted, so a missing publisher surfaces mid-publication as
  [T7](#t7-403-from-cratesio-during-publication).
- **An abandon command.** Recovering from a stuck release today means deleting
  drafts with `discard` (§5.3) and releasing a new version.
- **Consumer smoke tests in Phase E.** The design calls for resolving
  representative consumers from crates.io before the drafts are published.
  Finalization verifies assets, tags, and the plan, but installs nothing.
  `verify-closure` covers the equivalent question before publication, against
  packaged rather than published crates.
- **A dedicated rehearsal workflow.** §5 uses the production workflow stopped at
  the gate, which exercises the same code.

### Implemented and tested end to end

`lint`, `package-order`, `set-version`, `changelog-prompt`, `plan`,
`verify-closure`, `seal`, `reconcile`, `bundle`, `archive-binary`, `stage`,
`discard`, `publish`, `finalize`, and `fake-registry` - plus the production
workflow and the pull-request gate.

"Tested" means against the rehearsal registry and the GitHub double, which is as
close to production as anything gets without publishing for real.
