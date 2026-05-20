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

### 1. Release of the Miden SDK crates

1. Create a release PR against the `next` branch naming the branch with the `release-plz-` prefix (its important to use this prefix to trigger the crate publishing on CI in the later step).
2. Manually bump ALL the SDK crate versions (`sdk` folder) and update the `sdk/sdk/CHANGELOG.md`
3. Review the changes in the release PR,  and merge it into the `next` branch.
4. The CI will automatically run `release-plz release` after the release PR is merged to publish the new versions to crates.io.

### 2. Release of the Miden Compiler

1. Update the contract templates at https://github.com/0xMiden/project-template (see 2.1 below).
2. Update the new project template at https://github.com/0xMiden/project-template (see 2.2 below).
3. Merging to `main` will create a new release PR containing any unreleased changes.
4. Optional. Change the proposed crate version, CHANGELOG edits.
5. The release PR gets merged to `main` when we are ready to publish the release.
6. The crates are published to crates.io, a new git tag is created, as well as a GitHub release
7. A job is run to pre-build the executable for our supported targets and upload them to the created Github release.
8. Merge the `main` branch back to the `next` branch.

### 2.1. Updating the new contract templates

1. Bump the Miden SDK version in the Cargo.toml.
2. Migrate the code in lib.rs.
3. Create a git tag.
4. Make a PR in the compiler repo and set the new git tag (bump the current in `PROJECT_TEMPLATES_REPO_TAG` at tools/cargo-miden/src/commands/new_project.rs).
5. Run the compiler tests, if red then goto 2.

### 2.2. Updating the new project template

1. Bump the Miden SDK, `miden-client` versions in the Cargo.toml files, set the `cargo-miden` version to the `next` branch for now(after the compiler release it'd be the new version).
2. Migrate the code in the contracts, tests and the app.
3. Create a git tag.
4. Make a PR in the compiler repo and set the new git tag (bump the current in `MIDEN_PROJECT_TEMPLATE_REPO_TAG` at tools/cargo-miden/src/commands/new_project.rs).
5. Run the local repo tests, if red then goto 2.
6. Run the compiler tests, if red then goto 2.

### 3. After the Miden Compiler crates are published

1. Change the `cargo-miden` version to the newly published crate in the PR (created in 2.2.4) at https://github.com/0xMiden/project-template.
2. Re-set the same git tag (created in 2.2.3) to the new commit.
3. Merge the PR (created in 2.2.4).
