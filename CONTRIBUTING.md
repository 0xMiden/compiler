# Contributing to Miden Compiler

#### First off, thanks for taking the time to contribute!

You can find more detailed explanation of main project concepts in the [docs](https://docs.miden.xyz/core-concepts/compiler/).

We want to make contributing to this project as easy and transparent as possible, whether it's:

- Reporting a [bug](https://github.com/0xMiden/compiler/issues/new)
- Taking part in [discussions](https://github.com/0xMiden/compiler/discussions)
- Submitting a [fix](https://github.com/0xMiden/compiler/pulls)
- Proposing new [features](https://github.com/0xMiden/compiler/issues/new)

&nbsp;

## Contribution Quality

To keep review time focused on meaningful improvements, we generally do not accept:
- Trivial typo fixes
- Minor code or documentation changes that don't materially improve clarity or completeness

Contributions should:
- Include clear reasoning for the change
- Be linked to an issue the author has been assigned to
- Be testable / reviewable without unnecessary overhead
- Pass all CI tests

**We reserve the right to close PRs at our discretion, or batch trivial valid fixes into internal commits.**

## Flow

We are using [Github Flow](https://docs.github.com/en/get-started/quickstart/github-flow), so all code changes happen through pull requests from a [forked repo](https://docs.github.com/en/get-started/quickstart/fork-a-repo).

### Branching

- The current active branch is `next`. Every branch with a fix/feature must be forked from `next`.

- The branch name should contain a short issue/feature description separated with hyphens [(kebab-case)](https://en.wikipedia.org/wiki/Letter_case#Kebab_case).

    For example, if the issue title is `Fix functionality X in component Y` then the branch name will be something like: `fix-x-in-y`.

- New branch should be rebased from `next` before submitting a PR in case there have been changes to avoid merge commits.
i.e. this branches state:
  ```
          A---B---C fix-x-in-y
         /
    D---E---F---G next
            |   |
         (F, G) changes happened after `fix-x-in-y` forked
  ```

  should become this after rebase:


  ```
                  A'--B'--C' fix-x-in-y
                 /
    D---E---F---G next
  ```


  More about rebase [here](https://git-scm.com/docs/git-rebase) and [here](https://www.atlassian.com/git/tutorials/rewriting-history/git-rebase#:~:text=What%20is%20git%20rebase%3F,of%20a%20feature%20branching%20workflow.)

### Signing commits

We require all commits to be [signed](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification#ssh-commit-signature-verification).


### Commit messages
- Commit messages should be written in a short, descriptive manner and be prefixed with tags for the change type and scope (if possible) according to the [semantic commit](https://gist.github.com/joshbuchea/6f47e86d2510bce28f8e7f42ae84c716) scheme.
For example, a new change to the codegen crate might have the following message: `feat(codegen): add lowering for new instruction 'hir.foo'`

- Also squash commits to logically separated, distinguishable stages to keep git log clean:
    ```
    7hgf8978g9... Added A to X \
                                \  (squash)
    gh354354gh... oops, typo --- * ---------> 9fh1f51gh7... feat(X): add A && B
                                /
    85493g2458... Added B to X /


    789fdfffdf... Fixed D in Y \
                                \  (squash)
    787g8fgf78... blah  blah --- * ---------> 4070df6f00... fix(Y): fixed D && C
                                /
    9080gf6567... Fixed C in Y /
    ```

### Code Style and Documentation

- For documentation in the codebase, we follow the [rustdoc](https://doc.rust-lang.org/rust-by-example/meta/doc.html) convention with no more than 100 characters per line.

- [Rustfmt](https://github.com/rust-lang/rustfmt) and [Clippy](https://github.com/rust-lang/rust-clippy) linting is included in CI pipeline. Anyways it's preferable to run linting locally before push:
    ```
    cargo make format && cargo make clippy --fix
    ```

### Versioning

We use [semver](https://semver.org/) naming convention.

&nbsp;

## Pre-PR checklist
1. Repo forked and branch created from `next` according to the naming convention.
2. Every commit is [signed](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification#ssh-commit-signature-verification).
3. Commit messages and code style follow conventions.
4. Tests added for new functionality.
5. Documentation/comments updated for all changes according to our documentation convention.
6. `cargo make format`, `cargo make clippy`, and `cargo make unused` lints produce no errors.
7. New branch rebased from `next`.

&nbsp;

## Write bug reports with detail, background, and sample code

**Great Bug Reports** tend to have:

- A quick summary and/or background
- Steps to reproduce
- What you expected would happen
- What actually happens
- Notes (possibly including why you think this might be happening, or stuff you tried that didn't work)

&nbsp;

## Any contributions you make will be under the MIT Software License

In short, when you submit code changes, your submissions are understood to be under the dual [MIT](./LICENSE-MIT) and [Apache 2.0](./LICENSE-APACHE) license that covers the project. Feel free to contact the maintainers if that's a concern.

## Release Process

Releases are performed by the tooling in `tools/release`, driven by the
`release.yml` workflow. The operational guide is
**[docs/release-process.md](docs/release-process.md)** — follow the checklist
there for the kind of release you are doing.

A few things that used to live here and have changed, because the old procedure
is still in people's heads:

- **Releases happen from `main`, not `next`.** A release starts by promoting
  `next` into `main`; the release candidate then branches from and merges into
  `main`.
- **`release-plz` is gone.** Versions are moved with
  `cargo make release set-version --unit <compiler|sdk|templates> <version>`,
  which updates every manifest, every requirement naming them, the lockfile, and
  `.release/release.toml`. Never hand-edit a version: an SDK bump also rewrites
  the SDK requirement in every template, and picks a different form of that
  requirement for a prerelease than for a stable release.
- **The compiler, the SDK, and the project templates are three independent
  release units**, each with its own version, tag namespace, and changelog.

### Changing the project templates

The templates live in this repository at `extra/templates` and are released as
`templates/v*`. They are no longer maintained in `0xMiden/rust-templates` or
`0xMiden/project-template`, and no git tag is moved to publish them.

1. Edit the templates under `extra/templates`.
2. Regenerate the archive `cargo-miden` embeds:

   ```bash
   cargo make release bundle --output tools/cargo-miden/templates.tar.gz
   ```

   `cargo make release lint` fails if you forget; the archive is built from
   files **tracked by git**, so anything untracked is silently absent from it
   and is reported rather than included.

3. Check that the templates still build:

   ```bash
   cargo make test-templates
   ```

   This scaffolds a project from every template with the compiler from your
   checkout and builds each one for both profiles. It is the only thing that
   compiles the templates — they are outside the workspace, so no other
   `cargo test` reaches them.

4. Release the `templates` unit to put the change in front of users.
   `cargo miden new` resolves the newest released bundle in its minor series at
   runtime and falls back to the copy embedded in the binary, so an installed
   `cargo-miden` picks up template fixes without being reinstalled — but only
   once the bundle is released.

   Use `cargo miden new <name> --force-download` to check what a user will
   actually get: it requires the released bundle and fails rather than quietly
   falling back to the embedded copy.
