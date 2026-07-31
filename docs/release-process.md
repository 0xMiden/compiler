# Release Process

This is the operational guide for releasing the compiler, the Rust SDK, and the
project templates.

Three things are true throughout and explain most of the procedure:

- **Publication to crates.io cannot be undone.** There is no rollback, and a
  version can never be reused. Everything reversible therefore happens before
  anything irreversible, and the approval gate sits exactly on that boundary.
- **A release is resumable.** Every attempt reconciles against live registry
  state first, so a run that dies partway through can be re-run and will publish
  only what is missing. A first attempt and a resume are the same command.
- **Versions are chosen by a person.** The tooling validates a version; it never
  picks one.

---

## 1. Prerequisites

These are one-time setup steps. They require repository-admin and crates.io-owner
access, and none of them can be performed by the tooling.

### 1.1 crates.io

| What | Detail |
| --- | --- |
| Trusted Publisher, per crate | Repository `0xMiden/compiler`, workflow `release.yml`, environment `release-production` |
| Trusted-Publishing-only mode | Enable per crate **after** its publisher is verified working |
| New crates | crates.io cannot configure a publisher for a crate that does not exist. A brand-new crate must be bootstrapped once with a short-lived, narrowly scoped token, then switched to Trusted Publishing |
| Long-lived tokens | Remove `CARGO_REGISTRY_TOKEN` from repository secrets once Trusted Publishing works |

There are **33 publishable crates** (`release-tool package-order` lists them).
Each needs its own publisher configuration.

> **Known limitation.** A crate whose publisher is missing or misconfigured
> cannot be detected in advance: the token crates.io issues carries no list of
> the crates it authorizes. The failure surfaces mid-publication as a 403. This
> is survivable — the release resumes once the configuration is fixed — but it
> is why §1.4's audit matters.

### 1.2 GitHub repository settings

| What | Detail |
| --- | --- |
| Immutable releases | Enable for the repository |
| Environment `release-production` | Required reviewers; self-review disabled; admin bypass disabled where supported; deployment restricted to protected `main` |
| Tag rulesets for `v*`, `sdk/v*`, `templates/v*` | **Restrict deletions and updates only. Do not restrict creations.** |
| Required status check | `release / gate` |
| CODEOWNERS | Release infrastructure paths reviewed by a release owner |

> **Why creations are not restricted.** Ruleset bypass actors are roles, teams,
> apps, and deploy keys — there is no workflow-level actor. `GITHUB_TOKEN` acts
> as the repository-wide GitHub Actions app, so a creation bypass would grant
> *every* workflow the ability to create release tags, which is worse than not
> restricting creation at all. The property that matters is that a published tag
> can never be moved or removed, and deletion/update restrictions give exactly
> that.

### 1.3 Repository state

- Every workspace package classified in `.release/config.toml`
  (`release-tool lint` enforces this).
- No active `[patch]` entries in the root manifest.
- All first-party external dependencies (miden-vm, protocol, and related
  repositories) published at the versions the workspace requires.

### 1.4 Trusted Publishing audit

Because a missing publisher cannot be preflighted, audit the configuration
before each release:

```bash
# Requires a crates.io API token with the `trusted-publishing` endpoint scope,
# crate-scoped to this repository's crates, and a short expiry.
# Create it, run the audit, then revoke it. Do not store it as a repository secret.
release-tool audit-publishers   # not yet implemented; see §6
```

> The `trusted-publishing` scope also permits *creating and deleting* publisher
> configurations, so a token carrying it is privilege-escalating. It must never
> live in CI.

---

## 2. Production release

### Phase A — Prepare the candidate

Performed by a maintainer, on a branch. Nothing here touches production.

1. **Choose the units and versions.** This is a judgement call: what is being
   released, and whether it is a prerelease.

2. **Move the versions.** Once per unit being released:

   ```bash
   cargo make set-version -- --unit sdk 0.14.0
   cargo make set-version -- --unit compiler 0.10.0
   ```

   Omit the version for the next minor. Add `--dry-run` first to review the
   edits. This updates the manifests, every requirement naming them, the
   lockfile, and `.release/release.toml`.

3. **Write the changelog.** `cargo make changelog-prompt -- <unit>` emits a
   prompt; it does not write entries. Pass it to an agent if you like, then
   review and edit what comes back.

4. **Open the release-candidate pull request** with the version, lockfile,
   changelog, and `.release/release.toml` changes.

   > **You must click "Approve and run" on the pull request.** GitHub does not
   > start workflow runs for pull requests opened by `GITHUB_TOKEN`, and pushes
   > to such a branch create no runs at all. Since `release / gate` is a required
   > check, the candidate cannot merge until you do this.

5. **Review and merge** once the gate passes.

6. **Freeze merges to `main`** for the duration of the release. The tooling
   requires the release commit to be `main`'s HEAD, and anything landing after
   the candidate invalidates that.

### Phase B — Plan

7. **Dispatch `release.yml` from `main`.** No version, scope, or unit inputs —
   the scope comes from the committed `.release/release.toml` and nothing else.

8. Automation validates the subject: it must be `main`'s HEAD and the most
   recent commit touching `.release/release.toml`. It then generates the intent
   and presents the scope, stages, versions, and tags.

9. **Review the intent summary.** Cancel now if anything is unexpected. Nothing
   irreversible has happened.

### Phase C — Build and stage

Automation packages the closure, builds and attests the artifacts, seals the
plan, creates draft releases, uploads assets, and reads every one back.

Failure here is reversible: no tag exists, no crate is published, drafts can be
deleted.

> One exception: **attestations are permanent.** They are recorded in a public
> transparency log during this phase, so a cancelled release leaves provenance
> for artifacts that were never released. This is harmless — nothing consumes an
> attestation for an artifact with no release — but it is not reversible.

### Phase D — Approve and publish

10. **Approve the `release-production` environment.** This is the point of no
    return. Before approving, check the plan digest, the crate list, the asset
    list, and any warnings.

11. Automation then, per unit in dependency order (SDK → templates → compiler):
    creates that unit's tag, obtains a short-lived Trusted Publishing token,
    reconciles against crates.io, publishes only what is absent, and verifies
    the result from the registry.

> **The token lives 30 minutes.** Cargo publishes in dependency waves and waits
> for index confirmation between them — measured at roughly a dozen waves for 33
> crates. If a stage approaches the budget, split it.

### Phase E — Finalize

Automation runs consumer smoke tests against the still-draft releases, reverifies
every asset and attestation, then publishes the drafts in order: SDK, templates,
compiler. Publication makes them immutable.

12. **Review the final report.** Handle announcements and downstream
    coordination outside the tooling.

---

## 3. When something goes wrong

### Before the first crate is published

Stop. Nothing is irreversible except attestations. Fix the cause in a new
candidate and start again.

### After a partial publication

Prefer **resuming**: re-dispatch with the same plan. Reconciliation will publish
only what is missing and skip what already landed.

Do not, while a release is incomplete:

- Move or delete a release tag.
- Delete a draft holding the sealed plan.
- **Yank a planned version.** A yanked version is a conflict, can never be
  republished, and versions cannot change during a resume — yanking mid-incident
  strands the release. Abandon it first, then yank.

### Abandoning

If resumption is blocked for any reason, abandon rather than fight the plan:

1. Cancel the stuck workflow run and confirm it is terminal.
2. Dispatch `release.yml` in abandon mode with the plan digest.
3. Automation exports the sealed plan and an inventory of what cannot be
   withdrawn — published crates, created tags, finalized releases — then deletes
   the still-draft releases.
4. Yank stranded crate versions if warranted.
5. Open a new candidate with **new versions**. A newer release supersedes a stuck
   one; leftover tags and releases are accepted as debris.

### After finalization

An immutable release cannot be corrected. Release a new version. For templates,
the same applies and the deny list (see the design, §12.4) prevents clients from
using the bad version.

---

## 4. Rehearsal

A rehearsal exercises the real publication path without touching crates.io. It
uses a local registry that serves crates published to it and proxies crates.io
for everything else, so the full third-party dependency closure resolves.

### 4.1 Locally

```bash
# 1. Start the registry (leave running).
release-tool fake-registry --port 8732 --cache-dir /tmp/rehearsal-cache

# 2. In a candidate workspace with bumped, unpublished versions:
release-tool plan --subject $(git rev-parse HEAD) --output intent.json
release-tool seal --intent intent.json --output plan.json
release-tool publish --plan plan.json --rehearsal-index sparse+http://127.0.0.1:8732/
```

`verify-closure` does the same packaging and additionally builds a consumer that
resolves only through the registry — which is what proves the published archives
are usable, and is why production may publish with `--no-verify`.

> Versions must be unpublished. The registry proxies crates.io, so an
> already-published version is visible and Cargo will refuse to republish it.

### 4.2 Hosted

The hosted rehearsal runs the same flow on real runners, builds and attests the
real artifacts, and creates disposable draft releases under a rehearsal-only tag
namespace, deleting them afterwards.

**What a rehearsal does not prove:** that crates.io accepts the upload, that
Trusted Publishing is configured for each crate, production rate limits, and
immutable finalization. The first three are only provable by a real publish; the
last is exercised by a real prerelease.

---

## 5. Prereleases

A prerelease is selected by giving a version with a prerelease identifier, e.g.
`0.10.0-rc.1`. It is the intended first exercise of the production path, because
**a prerelease cannot disturb existing consumers**: a default requirement such as
`midenc = "0.9"` never matches a prerelease.

- GitHub releases are marked prerelease and never become `latest`.
- **Templates can be part of a prerelease**, and must be when a compiler change
  requires a template change. The bundle takes a prerelease version, its release
  is marked prerelease, and stable clients never see it — template resolution
  selects the highest *stable* compatible release, so a prerelease bundle is
  reachable only from a prerelease `cargo-miden` or an explicit
  `--template-version`.

---

## 6. Not yet implemented

Stated plainly so this document is not read as describing more than exists:

- GitHub draft creation, asset upload, and finalization are implemented as a
  client with an in-memory test double. **The REST implementation against real
  GitHub is unexercised** and the first rehearsal is what validates it.
- Artifact building, archiving, and attestation.
- Template bundle release.
- `audit-publishers`, the changelog prompt, and the abandon command.
- The workflows themselves.

The commands that do exist and are tested end to end: `lint`, `package-order`,
`set-version`, `plan`, `verify-closure`, `seal`, `reconcile`, `publish`, and
`fake-registry`.
