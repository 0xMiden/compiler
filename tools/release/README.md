# `release-tool`

A release driver for Cargo repositories. It takes a repository from "a
maintainer has decided to release something" to "the crates are on the registry
and the GitHub releases are immutable", in steps ordered so that everything
reversible happens before anything irreversible.

Everything it knows about a repository comes from one file, `.release/config.toml`.
The tool contains no repository-specific names: units, tags, changelogs, version
domains, artifacts, and asset routing are all declared there. Copying `.release/`
and the release workflows into another Cargo repository, and editing the
configuration, is the intended way to reuse it — see
[Adapting it to a new repository](#adapting-it-to-a-new-repository).

- Crate: `midenc-release`. Binary: `release-tool`.
- In this repository: `cargo make release <command>`.
- Elsewhere: `cargo run -p midenc-release --bin release-tool -- <command>`, or
  build it once and put it on `PATH`.

Every command takes two global flags:

| Flag | Default | Meaning |
| --- | --- | --- |
| `--config <PATH>` | `.release/config.toml` | The release configuration to load. |
| `--manifest-dir <PATH>` | the current directory | The workspace root to operate on. |

For this repository's own release procedure — its units, its workflow jobs, its
approval gate, and its recovery playbook — see
[`docs/release-process.md`](../../docs/release-process.md).

---

## What the tool does

A release moves through six steps. The commands are usable one at a time from a
terminal, but in production they are driven by a workflow, and each step's
output is the next step's input.

### 1. Candidate

A maintainer decides what is being released and at what version — the tooling
validates a version, it never picks one. `set-version` moves a version domain,
rewriting every manifest that carries or requires it, rewriting any embedded
requirement that names it, refreshing the lockfile, and recording the decision
in `.release/release.toml`. `changelog-prompt` emits a prompt for writing the
entries; a human writes them. `bundle` rebuilds any committed artifact archive
that the sources have moved out from under. `lint` checks the preconditions.
The whole step lands as an ordinary reviewed pull request.

### 2. Intent

`plan` reads the merged `.release/release.toml` and derives the *intent*: which
units are released, at what versions, which tags will be created, which crates
each stage publishes and in what order, and which release — if any — claims the
repository's "latest" slot. The intent is a pure function of the candidate, the
configuration, and workspace metadata: identical inputs produce byte-identical
JSON, so the intent a maintainer reviews and the intent the executor acts on are
provably the same document.

### 3. Seal

`seal` packages every crate the intent names and binds the reviewed scope to the
exact bytes that will be published, producing a *plan*: the intent plus a digest
and size per crate. Sealed plans are never edited. Anything that would change one
requires a new intent. `verify-closure` is the separate, stronger check that the
packaged archives actually resolve and build from a registry; production
publishes with `--no-verify`, so this is the only thing standing between that and
a broken published crate.

### 4. Stage

`stage` creates a draft GitHub release per unit, uploads the assets each unit's
globs claim, reads every asset back, and attaches the sealed plan and a
`SHA256SUMS` manifest. Drafts are invisible to anyone without write access, no
tag exists, and nothing has been published. `discard` deletes them.

### 5. Publish

`publish` executes the sealed plan stage by stage. Per stage: create that unit's
tag, reconcile against live registry state, publish only what is absent, and
verify the result from the registry before the next stage begins. Reconciliation
first means a first attempt and a resume are literally the same run — a run that
dies partway through can be re-run and will do only what is missing.

### 6. Finalize

`finalize` verifies every staged draft — the tag exists and points at the
released commit, the draft carries this run's sealed plan byte for byte, every
asset still hashes to what staging recorded, and nothing unplanned is attached —
and only then publishes them. All units are verified before any is published,
because publication makes a release immutable.

### The reversibility boundary

**Everything through step 4 can be undone. Nothing after it can.**

| Step | Reversible | How to undo it |
| --- | --- | --- |
| 1. Candidate | yes | Close the pull request. |
| 2. Intent | yes | Discard the intent; nothing was built. |
| 3. Seal | yes | Discard the plan; nothing left the machine. |
| 4. Stage | yes | `discard --plan <plan>` deletes the drafts. |
| — approval gate — | | |
| 5. Publish | **no** | A tag cannot be moved or deleted. A crates.io version can never be reused, republished, or unyanked. |
| 6. Finalize | **no** | A published GitHub release is immutable. |

Put the approval gate exactly on that line and the reversible steps become a
rehearsal of the irreversible ones, at no risk. One caveat: if the workflow
produces build attestations, those are written to a public transparency log
during staging and cannot be withdrawn. Provenance for an artifact that was
never released is harmless, but it is permanent.

---

## Command reference

| Command | Step | What it does |
| --- | --- | --- |
| [`lint`](#lint) | candidate | Check release-candidate preconditions. |
| [`package-order`](#package-order) | any | Print the publication order for a unit. |
| [`set-version`](#set-version) | candidate | Move a version domain. |
| [`changelog-prompt`](#changelog-prompt) | candidate | Emit a prompt for writing changelog entries. |
| [`bundle`](#bundle) | candidate | Build an artifact unit's archive. |
| [`plan`](#plan) | intent | Generate the intent from the committed candidate. |
| [`verify-closure`](#verify-closure) | seal | Prove the packaged crates build from a registry. |
| [`seal`](#seal) | seal | Bind an intent to the bytes that will be published. |
| [`stage`](#stage) | stage | Create and populate the draft releases. |
| [`discard`](#discard) | stage | Delete a plan's still-draft releases. |
| [`archive-binary`](#archive-binary) | stage | Package an executable into a deterministic archive. |
| [`reconcile`](#reconcile) | publish | Report what still needs publishing. |
| [`publish`](#publish) | publish | Publish a sealed plan. |
| [`finalize`](#finalize) | finalize | Verify every staged draft and publish it. |
| [`fake-registry`](#fake-registry) | rehearsal | Run a local registry to rehearse against. |

### `lint`

```
release-tool lint [--candidate <PATH>]
```

Checks release-candidate preconditions. `--candidate` defaults to
`.release/release.toml`; when that file does not exist the candidate-dependent
checks are skipped rather than failing, so `lint` is meaningful on any branch.

It reports: unclassified workspace members, packages classified but absent from
the workspace, a classification that disagrees with the manifest's own `publish`
field, a private package that has drifted off `private-version`, a publishable
crate depending on a private one, an active `[patch]` entry in the root
manifest, a committed artifact archive that no longer matches its sources (and,
if any, the untracked files under those sources that explain the difference),
and a tracked requirement that cannot resolve the version this candidate
publishes for the unit it tracks.

### `package-order`

```
release-tool package-order [--unit <UNIT>] [--cargo-args]
```

Prints packages in dependency order, one per line. `--unit` restricts it to one
unit's packages; omit it for every publishable package. `--cargo-args` emits the
order as a `-p NAME -p NAME …` string ready to paste after `cargo publish` or
`cargo package`.

Cargo's own packaging order is not reliable at scale, so every `cargo package`
and `cargo publish` invocation should take its `-p` list from here.

### `set-version`

```
release-tool set-version --unit <UNIT> [VERSION] [--dry-run] [--force]
```

Moves a version domain and every requirement that names it. `VERSION` is
optional; omitted, it bumps to the next minor. `--dry-run` prints the edits
without writing them.

For a unit that publishes crates, it moves every package in that unit's
[version domain](#version-source), rewrites every intra-workspace requirement
naming one of them, refreshes `Cargo.lock`, rewrites any tracking unit's
embedded requirement, and records the unit and version in `.release/release.toml`.
For an artifact unit it writes the version to the unit's version file or source
manifest and records the same declaration.

`--force` allows a move that does not increase the version. SemVer orders a
prerelease below its release, so replacing an already-bumped `0.32.0` with
`0.32.0-rc.1` is a backwards move and refused without it. Forcing is safe only
while the target version is unpublished; a published version can never be reused,
flag or no flag.

Always use this rather than hand-editing manifests. Picking the right *form* of
a tracked requirement — `major.minor` for a stable version, the exact version for
a prerelease, because a caret requirement never matches a prerelease — is part of
what it does, and skipping it produces a set of files that agree with each other
and cannot resolve.

### `changelog-prompt`

```
release-tool changelog-prompt <UNIT> [RANGE] [--version <VERSION>]
```

Emits a prompt for writing a unit's changelog entries, and nothing else: it never
writes entries. What changed and why it matters to a reader is a judgement, and
prose generated here would look reviewed without having been.

`RANGE` overrides the revision range, which defaults to the unit's last release
tag through `HEAD`, filtered to the paths that unit publishes. `--version` sets
the section heading; omit it for an `[Unreleased]` section.

> A unit whose tag namespace has never been released has no baseline, so the
> default range is its entire history. Pass a range explicitly the first time.

### `bundle`

```
release-tool bundle [--unit <UNIT>] [--output <PATH>]
```

Builds an artifact unit's archive from git-tracked files, deterministically: the
same commit always produces the same bytes, and the digest is what a committed
copy is checked against. It prints the version, file count, size, and sha256;
`--output` also writes the archive.

`--unit` is required only when the repository declares more than one artifact
unit; with exactly one, it is inferred. Before archiving, it checks every
requirement the unit's sources embed against what the unit's manifest declares,
and warns about untracked files under the included paths — those are absent from
the archive, and are the usual reason two checkouts of one commit disagree.

> `bundle` requires the unit's source to be a `directory`; a `file` source is
> attached as-is and has nothing to archive. Both forms of include list work.
> An inline `include` unit has no manifest to hold its version, so it needs a
> `version-file`, and its archive carries no manifest entry.

### `plan`

```
release-tool plan --subject <SHA> [--candidate <PATH>] [--output <PATH>]
```

Generates the intent from the committed candidate. `--subject` is the commit
whose source would be packaged. `--candidate` defaults to
`.release/release.toml`. Without `--output` the intent goes to stdout; with it,
the file is written and the intent's digest printed.

The candidate is fully validated first, so an invalid candidate fails here rather
than later. Output is canonical JSON and deterministic by construction.

### `verify-closure`

```
release-tool verify-closure [--unit <UNIT>] [--no-build] [--cache-dir <DIR>]
```

Packages the selected crates, publishes them to a throwaway registry, and builds
a consumer that resolves *only* through it. That last part is the point:
resolution alone cannot prove an archive contains every file it needs.

`--unit` restricts the selection; omit it for every publishable package.
`--no-build` skips the consumer build — much faster, and much weaker.
`--cache-dir` caches upstream index responses between runs.

It also runs a second, narrower check: whether the release scope is
self-contained, i.e. whether publishing it would leave a requirement that cannot
resolve. With `--unit`, the scope is that unit's packages. Without it, the scope
comes from `.release/release.toml` — the candidate's units plus the transitive
closure of the `library` crates they depend on — because selecting *everything*
makes every dependency internal and the check vacuous.

This is what justifies publishing with `--no-verify` in production, where
skipping Cargo's verification keeps build scripts from running beside a live
token. It is required, not optional.

### `seal`

```
release-tool seal --intent <PATH> [--output <PATH>] [--no-build] [--cache-dir <DIR>]
```

Packages the intent's closure and records each crate's exact digest and size into
a sealed plan, so what gets published is pinned to what was inspected. `--output`
writes the plan and prints its digest; otherwise it goes to stdout. `--no-build`
and `--cache-dir` behave as they do for `verify-closure`.

### `stage`

```
release-tool stage --plan <PATH> [--artifacts <DIR>] [--api-base <URL>]
```

Creates a draft release per unit in the plan and uploads its assets.
`--artifacts` is a flat directory of built files; each file is routed to a unit
by that unit's `assets` globs. Every upload is read back and compared.
`--api-base` points at a different GitHub API, for tests; production takes it
from `GITHUB_API_URL`.

Routing is strict, and all three ways it can go wrong are errors rather than
defaults: a file matching **no** unit's globs, a file matching **more than one**,
and a file belonging to a unit **not in this plan** — which would be uploaded
nowhere and silently dropped. A unit's `required-assets` globs must each match
something staged.

### `discard`

```
release-tool discard --plan <PATH> [--api-base <URL>]
```

Deletes the still-draft releases belonging to a plan, and prints each tag it
deleted. A published release is left alone: it cannot be removed, and pretending
otherwise would hide the problem.

### `archive-binary`

```
release-tool archive-binary --binary <PATH> --name <NAME> --output <PATH>
```

Packages a built executable into a deterministic `.tar.gz`. `--name` is the
executable's name inside the archive, and therefore on `PATH` after extraction.
Prints the size and sha256.

### `reconcile`

```
release-tool reconcile [--index <URL>] [--unit <UNIT>] [--plan <PATH>]
```

Reports, against live registry state, what still needs publishing: `publish`,
`skip`, or `CONFLICT` per crate, then the `-p` list for what remains. Exits
non-zero if there are conflicts.

`--index` defaults to `sparse+https://index.crates.io/`; point it at a rehearsal
registry to reconcile against one. `--unit` restricts the selection. `--plan`
reconciles against a sealed plan, which is the stronger form: with the plan's
digests in hand, an existing version whose content differs is reported as a
conflict rather than skipped as already-done.

This runs identically on a first attempt and on a resume. The only difference is
what the registry already contains.

### `publish`

```
release-tool publish --plan <PATH> [--rehearsal-index <URL>] [--dry-run]
                     [--journal <PATH>] [--api-base <URL>]
```

Publishes a sealed plan. Stages run in order; each is reconciled before it starts
and verified from the registry before the next begins. `--dry-run` reports what
would be published and stops. `--journal` writes a record of every decision.
`--api-base` points at a different GitHub API, for tests.

`--rehearsal-index` publishes to a rehearsal registry instead of crates.io. A
rehearsal is given a GitHub client that refuses every call: there is no throwaway
GitHub, so a tag created during a rehearsal would be a real, permanent tag naming
a version that was never released. Tag creation is the one part of this step a
rehearsal cannot exercise.

### `finalize`

```
release-tool finalize --plan <PATH> [--api-base <URL>]
```

Verifies every staged draft and then publishes them, printing each tag, whether it
was newly published or already published, and which claimed the "latest" slot.
Every unit is verified before the first is published, because publication is
irreversible.

### `fake-registry`

```
release-tool fake-registry [--port <PORT>] [--cache-dir <DIR>] [--offline]
```

Runs a local registry until interrupted, serving crates published to it and
proxying the real index for everything else. `--port` defaults to `8732`; `0`
selects an ephemeral port. `--cache-dir` keeps upstream index lookups between
runs. `--offline` serves only locally published crates and never contacts
crates.io.

It prints the two pieces of configuration a rehearsal needs — source replacement
in `$CARGO_HOME/config.toml` to redirect resolution, and `--index` to redirect
the upload target. Both are required: `--index` alone cannot resolve
interdependent unpublished crates.

This is the only command that needs neither a release configuration nor a
workspace.

---

## `.release/config.toml`

The authoritative release policy. Every workspace package must be classified
here exactly once. Classification is explicit rather than inferred from directory
layout, so adding a package forces a deliberate decision about whether it is
published.

Validation is front-loaded: the whole file is checked when it is loaded, by every
command. A unit naming a peer that does not exist, or a cycle in publish order,
is a mistake that would otherwise surface phases later, possibly after something
irreversible has happened.

Unknown keys are rejected everywhere.

### Top level

| Key | Required | Meaning |
| --- | --- | --- |
| `schema-version` | yes | Must be `2`. A mismatch is a hard error, not a best-effort parse. |
| `private-version` | no | The version every private package is frozen at. Absent means the check does not run. |
| `[units.<name>]` | yes | The release units, keyed by name. |
| `[[packages]]` | no | Package-to-unit classification. Defaults to empty. |

### Unit kinds

Four kinds, on two independent axes — whether a unit publishes crates to a
registry, and whether it has a tag, a changelog, and a GitHub release of its own.
Conflating those axes is what leaves a shared library crate with nowhere to go.

| `kind` | publishes crates | tag / changelog / release | What it is |
| --- | --- | --- | --- |
| `crates` | yes | yes | Crates published under this unit's tag and version. |
| `library` | yes | **no** | A shared crate several units depend on, published but never released on its own. |
| `artifact` | no | yes | A single file attached to a GitHub release. |
| `private` | no | no | Repository infrastructure that is never published. |

A `library` unit's crates join the publish stage of each releasable unit that
depends on them, transitively, deduplicated so that a crate two units share is
claimed by the first stage only. A `library` unit is never named in a candidate,
never gets a tag, and has no changelog. Without it, a crate that everything
depends on and nobody wants to tag has to be forced into one of the units that
uses it, which then owns it for versioning purposes and drags the others along.

### Unit fields

`✓` allowed, `●` required, blank forbidden — declaring a forbidden field is a
load error, not a silently ignored key.

| Key | `crates` | `library` | `artifact` | `private` | Meaning |
| --- | :-: | :-: | :-: | :-: | --- |
| `kind` | ● | ● | ● | ● | One of the four kinds above. |
| `tag` | ● | | ● | | Tag template. `{version}` is substituted. |
| `changelog` | ● | | ● | | Path to the unit's changelog, relative to the workspace root. |
| `changelog-headings` | ✓ | | ✓ | | Section headings for `changelog-prompt`. Defaults to `["Added", "Changed", "Fixed"]`. |
| `version-source` | ● | ● | | | Where the version lives: `"workspace"` or `"own"`. |
| `after` | ✓ | | ✓ | | Units that must publish before this one. |
| `release-when` | ✓ | | ✓ | | Units whose release forces this one into the same candidate. |
| `tracks` | | | ✓ | | Version requirements this unit's sources embed on another unit's packages. |
| `latest` | ✓ | | ✓ | | Whether this unit may claim the repository's "latest release" slot. Defaults to `false`. |
| `assets` | ✓ | | ✓ | | Globs routing `stage --artifacts` files to this unit. |
| `required-assets` | ✓ | | ✓ | | Globs that must match at least one staged file when this unit is in the plan. |
| `source` | | | ● | | How the artifact is produced. See [`[units.<name>.source]`](#unitsnamesource). |

A `private` unit declares nothing but its `kind`.

#### `version-source`

`"workspace"` is the root `[workspace.package]` version, inherited by members —
or, in a single-crate repository with no workspace table, the root
`[package].version` itself. `"own"` is the consensus of the unit's own member
manifest versions, which must already agree.

This names a *domain*: the set of packages that move together. The two values
name domains of different shapes.

`"workspace"` is one domain shared by every unit that declares it. They inherit
a single root version, so `set-version --unit X` on such a unit moves every
package in every unit that also names `"workspace"`. That is why two releasable
units may not both use it: bumping one would silently move the other's crates to
an unpublished version, and two releasable units sharing a version domain are
one unit. Give one of them `"own"`, or merge them. A `library` unit may share
the workspace domain, because it is never released on its own and rides the
version of whatever publishes it.

`"own"` is a domain of one — the declaring unit and nothing else, since the
versions in question are that unit's own member manifests. `set-version --unit X`
on such a unit moves that unit's packages and no others, so any number of units
may declare `"own"` and each keeps an independent version. A `library` unit
declaring `"own"` has a version of its own too, moved only by naming it.

#### `latest`

At most one unit may set it. The decision is sealed into the intent rather than
derived at finalization, so configuration cannot move between the review and the
publication. A prerelease never claims the slot whatever the configuration says —
handing GitHub's default download to something explicitly not recommended is the
failure this guards.

#### `assets` and `required-assets`

Globs are matched against the file *name* only, since `stage --artifacts` reads a
flat directory. `*` matches any run of bytes including none; `?` matches exactly
one. There are no character classes and no path separators. Globs must not
overlap between units. See [`stage`](#stage) for what routing refuses.

### `[units.<name>.source]`

Only on an `artifact` unit, where it is required. An artifact is *either*
archived from sources *or* attached as-is, never both.

| Key | Meaning |
| --- | --- |
| `directory` | Archive git-tracked files beneath this directory, relative to the workspace root. |
| `file` | Attach an existing file, produced by whatever built it. |
| `include` | Paths relative to `directory`, files or directories, each enumerated with `git ls-files`. The inline include list. |
| `manifest` | A TOML file inside `directory` whose entries supply the include list, and which also holds the unit's version. The named include list. |
| `asset` | The default `--output` name for `bundle`. Not a routing mechanism — the file reaches its release through `stage --artifacts` and this unit's `assets` globs, because CI builds it in one job and stages it in another. |
| `embedded-copy` | A committed copy of the archive that `lint` verifies against freshly built bytes. Drift here is otherwise invisible. |
| `version-file` | Where the version lives, when it is not the manifest's `version` key. A table: `path` (relative to the workspace root) and `key` (defaults to `"version"`). |

A `directory` source needs exactly one of `include` or `manifest`. It is
deliberately not a whole-subtree walk: repository furniture beside the sources —
READMEs, inherited CI, a test harness depending on a local crate by path — would
either confuse a consumer or fail to resolve outside the repository.

`version-file.key` is a single literal key, not a dotted path. `set-version`
refuses to create a key that is not already present, rather than silently
inserting a top-level `package.version` that nothing reads.

### `[units.<name>.tracks.<tracked>]`

Only on an `artifact` unit. Declares that this unit's sources embed a version
requirement on some of `<tracked>`'s packages.

| Key | Required | Meaning |
| --- | --- | --- |
| `packages` | yes | Packages of `<tracked>` whose requirements appear in these sources. A subset, not necessarily the whole unit. |
| `requirement-key` | no | The key in the unit's `source.manifest` holding the declared requirement. Defaults to `"<tracked>-requirement"`. |

`set-version --unit <tracked>` rewrites that key and every source manifest under
the include list that requires one of `packages`. `bundle` refuses to build an
archive whose sources disagree with the declared key. `lint` asks the sharper
question: whether what they agree on can resolve the version this candidate
actually publishes.

### `[[packages]]`

An array of `{ name, unit }` tables classifying every workspace member.

| Key | Meaning |
| --- | --- |
| `name` | The package name, as Cargo knows it. |
| `unit` | The unit it belongs to. |

Whether a package is published is derived from its unit's kind, not declared per
package, and `lint` requires that derivation to agree with the manifest's own
`publish` field.

### Relations between units

Three, and they are independent.

| Relation | Declared on | Means |
| --- | --- | --- |
| `after` | the later unit | Publish order. |
| `release-when` | the unit that gets dragged in | Explicit co-release, for reasons unrelated to version requirements. |
| `tracks` | the tracking unit | This unit's sources embed a requirement on the tracked unit's packages. |

**`after` and `release-when` point in opposite directions**, and that is
deliberate. Ordering is a property of the unit that must wait, so it names what it
waits for. Co-release is a property of the unit that gets dragged in, so it names
what drags it.

**`tracks` implies `after`** — a unit embedding a requirement on another cannot
resolve until that other unit is on the registry — but it deliberately does not
imply co-release. Whether a tracked bump forces a release is decided from
*content*, not from structure, because the requirement is written as
`major.minor` for a stable version:

| Tracked unit moves | Requirement | Tracking unit forced into the release? |
| --- | --- | --- |
| `0.14.0` → `0.14.1` | `"0.14"` → `"0.14"` | no — nothing changed |
| `0.14.0` → `0.15.0` | `"0.14"` → `"0.15"` | yes |
| `0.14.0` → `0.15.0-rc.1` | `"0.14"` → `"0.15.0-rc.1"` | yes — a caret requirement never matches a prerelease |

A structural rule would force a release on every patch bump, which means a version
bump with nothing to describe and a changelog entry that says nothing. The content
rule forces one exactly when the embedded requirement would otherwise be wrong.

### Validation performed at load

Every command loads the configuration, so all of this is checked on every
invocation, including `lint`.

**Schema**

1. `schema-version` must equal `2`.
2. Unknown keys are rejected, at every level.

**Units**

3. A unit may not declare a field its kind forbids — see the
   [field table](#unit-fields).
4. A `library` unit must declare `version-source`.
5. A `crates` unit must declare `tag`, `changelog`, and `version-source`.
6. An `artifact` unit must declare `tag`, `changelog`, and `source`.
7. A `source` must declare exactly one of `directory` and `file` — neither is an
   error, both is an error.
8. A `directory` source must declare exactly one of `include` and `manifest`.
9. A `file` source may declare neither `include` nor `embedded-copy`; there is
   nothing to rebuild it from, so nothing to compare.
10. At most one unit may set `latest`.
11. At most one *releasable* unit may use `version-source = "workspace"`.

**Packages**

12. A package may be classified at most once.
13. A package's `unit` must be a declared unit.
14. A `crates` or `library` unit must have at least one package.
15. An `artifact` unit must have none.

**Relations**

16. A unit may not name itself in `after`, `release-when`, or `tracks`.
17. Every unit named in a relation must be declared.
18. Every unit named in a relation must be releasable — a `library` or `private`
    unit is never released on its own, so nothing can order against it or be
    dragged in by it.
19. A tracked unit must be of kind `crates`; an artifact publishes no crates
    under its own version, so there are no requirements to embed.
20. A unit that tracks anything must declare `source.manifest`, or the declared
    requirement has nowhere to live.
21. `tracks.<unit>.packages` must be non-empty, and every name in it must belong
    to that unit.

**Order**

22. `after`, including the edges `tracks` implies, must be acyclic over the
    releasable units. The resulting order breaks ties by name, so it is a
    function of the configuration and nothing else — which is what makes the
    intent byte-reproducible.

---

## Worked example: a single-crate repository

One crate, one unit, one tag namespace.

```toml
schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
latest = true

[[packages]]
name = "mytool"
unit = "main"
```

`version-source = "workspace"` is right even though there is no workspace: with
no `[workspace.package]` table, the domain resolves to the root `[package].version`,
which in a single-crate repository is the only version there is.

`latest = true` makes each stable release the repository's "Latest release"; a
prerelease still will not claim it.

`cargo run --bin release-tool -- set-version --unit main 1.4.0` moves the version
and writes the candidate:

```toml
schema-version = 1
units = ["main"]

[main]
version = "1.4.0"
tag = "v1.4.0"
prerelease = false
```

That file is `.release/release.toml`, and it is the only thing production reads
to decide what a release contains — workflow inputs cannot widen or narrow it.
It deliberately restates versions that are already in the manifest: the
duplication is double-entry bookkeeping, and validation rejects a candidate whose
declaration and manifests disagree.

---

## Worked example: two binaries, two units, one shared library

Two tools released on independent schedules, sharing a support crate that nobody
wants to tag.

```toml
schema-version = 2

# Released on its own schedule; rides the workspace version domain.
[units.tool-a]
kind = "crates"
version-source = "workspace"
tag = "tool-a/v{version}"
changelog = "a/CHANGELOG.md"
latest = true
assets = ["tool-a-*.tar.gz"]

# Released on its own schedule, with its own version domain.
[units.tool-b]
kind = "crates"
version-source = "own"
tag = "tool-b/v{version}"
changelog = "b/CHANGELOG.md"
assets = ["tool-b-*.tar.gz"]

# Published, never released. No tag, no changelog, no candidate entry.
[units.common]
kind = "library"
version-source = "workspace"

[[packages]]
name = "tool-a"
unit = "tool-a"

[[packages]]
name = "tool-b"
unit = "tool-b"

[[packages]]
name = "common"
unit = "common"
```

`common` has nowhere else to go. It is not `private`, because it must be on the
registry for either tool to resolve. It cannot belong to `tool-a`, because then
`tool-b` would depend on a crate owned by a unit it is not released with, and
`verify-closure` would report the scope as not self-contained. Making it its own
`crates` unit would give it a tag and a changelog nobody writes, and a third
version to argue about. `library` is exactly the missing cell: publishes, does
not release.

What each release publishes:

| Candidate | Stages | Crates published |
| --- | --- | --- |
| `units = ["tool-a"]` | `tool-a` | `common`, `tool-a` |
| `units = ["tool-b"]` | `tool-b` | `common`, `tool-b` |
| `units = ["tool-a", "tool-b"]` | `tool-a`, then `tool-b` | `common` and `tool-a` in the first stage; `tool-b` in the second |

A library crate joins the stage of every releasable unit that depends on it,
transitively — and is deduplicated across stages, so the third row publishes
`common` once rather than naming it twice. On a subsequent release,
reconciliation sees it already on the registry at that version and skips it.

Two things to watch in this shape. `tool-b` uses `"own"` so that bumping `tool-a`
does not move it, since two releasable units may not share the `"workspace"`
domain. And `common` shares `"workspace"` with `tool-a`, which is allowed and is
what makes the support crate's version move whenever `tool-a`'s does. There is
no way to make it ride `tool-b` instead: `"own"` is a domain of one, so giving
`common` that would leave it with an independent version, moved only by
`set-version --unit common`.

---

## Worked example: this repository

Three releasable units and one private one. Abridged: the real file classifies
every workspace member, forty-two of them.

```toml
schema-version = 2

# Private crates are frozen here. A private crate carrying a plausible version
# invites the reader to assume it ships with the release.
private-version = "0.1.0"

# The Rust SDK. Its own version domain, so a compiler release does not move it.
[units.sdk]
kind = "crates"
version-source = "own"
tag = "sdk/v{version}"
changelog = "sdk/CHANGELOG.md"
changelog-headings = ["Added", "Changed", "Fixed", "Migration and breaking changes"]

# The project templates. Publishes an archive rather than crates, so no package
# is ever assigned to it, and its version lives in extra/templates/bundle.toml.
[units.templates]
kind = "artifact"
tag = "templates/v{version}"
changelog = "extra/templates/CHANGELOG.md"
changelog-headings = ["Templates", "SDK compatibility"]
assets = ["templates.tar.gz"]

[units.templates.source]
directory = "extra/templates"
# The manifest is the include list: the archive must contain everything
# `cargo miden new` renders from and nothing else.
manifest = "bundle.toml"
asset = "templates.tar.gz"
# cargo-miden ships a copy of the archive, because a .crate cannot contain files
# from outside its own directory. lint compares it against freshly built bytes.
embedded-copy = "tools/cargo-miden/templates.tar.gz"

# The templates hardcode an SDK requirement, so an SDK bump must carry them along
# or generated projects stay pinned to the previous SDK. Silent when missed: the
# templates still render and still build.
[units.templates.tracks.sdk]
packages = ["miden", "miden-sdk-build-script-support"]

# The compiler and its tools, on the workspace version domain.
[units.compiler]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
changelog-headings = [
    "Compiler and `midenc`",
    "`cargo-miden`",
    "`miden-objtool`",
    "Libraries and public APIs",
    "Migration and breaking changes",
]
# cargo-miden resolves the template bundle it was released beside, and midenc
# depends on the SDK's metadata crate, so both must be on the registry first.
after = ["sdk", "templates"]
# The compiler is what a user means by "the latest release".
latest = true
# The release matrix builds ${binary}-${target}.tar.gz for three binaries across
# two targets. All six route here.
assets = ["midenc-*.tar.gz", "cargo-miden-*.tar.gz", "miden-objtool-*.tar.gz"]
required-assets = ["midenc-*.tar.gz"]

# Repository infrastructure, never published.
[units.private]
kind = "private"

[[packages]]
name = "midenc"
unit = "compiler"

# … forty-one more classifications …
```

Reading it out: publish order is `sdk`, `templates`, `compiler`. `sdk` and
`templates` have no ordering between them and are ordered by name.
`templates` tracks `sdk`, which gives it an `after` edge on `sdk` without forcing
it into every SDK release. `compiler` waits for both. Only `compiler` can be
"latest". There is no `library` unit and no `release-when`: the SDK crate the
compiler needs at build time, `midenc-frontend-wasm-metadata`, is classified into
the `sdk` unit and published in the SDK's stage, which the compiler's stage
already comes after.

---

## Adapting it to a new repository

1. **Copy `.release/` and `tools/release/`.** The tool has no repository-specific
   names in it. Replace `.release/config.toml` with your own; delete
   `.release/release.toml`, which is generated.

2. **Copy the workflows.** `release-verify.yml` holds the substantive checks and
   no credentials; `release-ci.yml` calls it on pull requests and exposes a
   single required `gate` check; `release.yml` is the production pipeline, with
   its approval gate between staging and publishing. Adjust the artifact matrix
   to your binaries, and the branch names to yours.
   *Proves nothing on its own* — but this is where the reversibility boundary is
   enforced, so a repository that copies the tool without the gate has the tool
   and not the property.

3. **Write the configuration.** Declare one unit per thing that gets a tag, a
   `library` unit for any crate several of them share, and a `private` unit for
   everything that is never published. Then classify every workspace member.
   Start from the [single-crate example](#worked-example-a-single-crate-repository);
   most repositories are that plus one or two more units.

4. **Run `release-tool lint`.** *Proves*: every workspace member is classified
   exactly once, each classification agrees with its manifest's `publish` field,
   no publishable crate depends on a private one, no `[patch]` entry is active,
   private crates are all frozen, and any committed artifact copy matches its
   sources. Expect `release lint: N packages classified, no findings`.

5. **Run `release-tool package-order --cargo-args`.** *Proves*: the publish order
   is derivable and acyclic, and gives you the `-p` list every `cargo publish`
   should use.

6. **Bump a version and generate an intent.**
   `set-version --unit <unit> <version>`, then
   `plan --subject "$(git rev-parse HEAD)"`. *Proves*: the candidate validates,
   versions and manifests agree, tags render from their templates, stages are
   ordered as you intended, and the `latest` decision is what you expect. Run
   `plan` twice and diff the output — it must be byte-identical.

7. **Rehearse the publish path.** Start `fake-registry` in one terminal, then
   `verify-closure`, `seal`, and `publish --rehearsal-index`. *Proves*: the
   packaged archives are complete and resolve from a registry, and the whole
   publish path works — everything except tag creation, which no rehearsal can
   exercise.

8. **Rehearse the hosted path.** Run the production workflow and stop at the
   approval gate. *Proves*: real drafts, real asset upload and readback, real
   runners. Then `discard --plan` to delete the drafts.

Per-registry setup is outside the tool: on crates.io, each crate needs its own
Trusted Publisher entry naming your repository, workflow, and environment, and a
brand-new crate has to be bootstrapped once with a token before a publisher can
be configured for it. On GitHub, the release environment needs required
reviewers, and the tag namespaces need rulesets restricting deletions and
updates — but not creations, since the workflow creates them.
