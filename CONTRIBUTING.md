# Contributing to Miden Compiler

#### First off, thanks for taking the time to contribute!

You can find more detailed explanation of main project concepts in the [docs](https://docs.miden.xyz/core-concepts/compiler/).

We want to make contributing to this project as easy and transparent as possible, whether it's:

- Reporting a [bug](https://github.com/0xMiden/compiler/issues/new)
- Taking part in [discussions](https://github.com/0xMiden/compiler/discussions)
- Submitting a [fix](https://github.com/0xMiden/compiler/pulls)
- Proposing new [features](https://github.com/0xMiden/compiler/issues/new)

&nbsp;

## AI Tool Policy

If you use, or are planning on using AI tools to assist you in contributing to this project, then please read our [AI Tool Policy](docs/ai-tool-policy.md) before getting started.

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

## Pull Requests

We are using [Github Flow](https://docs.github.com/en/get-started/quickstart/github-flow), so all code changes from external contributors must happen through pull requests from a [forked repo](https://docs.github.com/en/get-started/quickstart/fork-a-repo).

### Branching

- The current active branch is `next`. Every branch with a fix/feature must be forked from `next`.

- The branch name should contain a short issue/feature description separated with hyphens [(kebab-case)](https://en.wikipedia.org/wiki/Letter_case#Kebab_case).

    For example, if the issue title is `Fix functionality X in component Y` then the branch name will be something like: `fix-x-in-y`.

- New branch should be rebased on `next` before submitting a PR in case there have been changes, so as to keep the history as clean as possible.
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

We require all commits to be [signed](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification#ssh-commit-signature-verification). If you submit a PR that fails this check, it will not be merged, and will likely be closed for failing to follow our contributing guidelines.


### Commit hygiene

Commit messages should be written in a short, descriptive manner and adhere to the following format: 

```text
<scope>: <summary>

<description>

<optional trailers>
```

Where:

- `<scope>` indicates the scope or component of the compiler affected by the change. This is sometimes a specific crate, e.g. `midenc`, but more often than not a change may modify multiple crates, in which case the scope should indicate what the cross-cutting concern is. For example, `analysis` would be a good scope for a change that implements a new dataflow analysis pass; `arith` would be a good scope for a change that is focused on the `arith` dialect; `build` or `ci` are commonly used for changes that modify build tooling or CI workflows. Don't overthink it - just try to convey to the reader what the scope of the change is, if they were to read the commit without any additional context. Look at existing commits for reference if you are unsure.
- `<summary>` should be a terse description of the change, no more than 80 characters if possible. You may omit the `<scope>: ` prefix if `<summary>` would exceed 80 characters otherwise, and already adequately conveys the scoping of the commit.
- `<description>` should be a more detailed description of what changed, primarily focusing on the _why_. The reader can look at the diff to see the _what_, so your goal is only to summarize that aspect briefly. What the reader can't tell from the diff is the context on _why_ the change was made - focus on that.
- `<optional trailers>` should contain trailers like issue references, `Assisted-by:`, etc. It is not required to specify these, and external contributors should avoid using `Closes:` or `Fixes:` trailers, as issue management is the concern of maintainers alone.

An example of a good commit message is the following:

```
ci: benchmark each revision with its own SDK and examples

The baseline compiler was driven over candidate sources, so SDK changes such as 
allocator fixes were included on both sides and their size differences 
disappeared. Use the baseline checkout for its examples, SDK, and inputs while 
retaining a common benchmark runner and VM executor. Read result revisions from 
their source checkouts and fail on either side’s build errors instead of 
publishing a partial baseline.
```

Contributors should ensure their PRs are comprised of no more than a single commit, but if necessary, multiple commits are permitted so long as they adhere to the following rules:

- Each commit is logically distinct from the others
- Each commit is valid on its own (i.e. tests pass and lints are clean)
- No "oops typo" or "addressing review feedback" commits.

Contributors are encouraged to squash commits and force push on their PR branches to uphold those rules. If you do not do so, a maintainer may choose to manually merge your PR by rewriting your commits themselves, or by squash-merging the PR and collapsing the entire branch into a single commit. If a maintainer does not have time to do this however, then your PR may linger unmerged until such time as someone gets around to doing it for you. 

TIP: If you want your PR merged quickly, then keeping up good commit hygiene makes it much more likely that will happen.

### Code Style and Documentation

We provide `cargo make` tasks for all of our primary checks:

- `cargo make format` to ensure code is formatted
- `cargo make clippy` to ensure code passes the set of lints we have enabled, pass `--fix` to automatically fix issues when possible
- `cargo make unused` to identify any unused dependencies/items that should be removed 
- `cargo make rustdocs` to ensure Rust-based documentation comments build
- `cargo make docs` to ensure our Docusaurus-based documentation pages build

You should run any of these that are relevant before submitting a PR.

For documentation in the code itself, we follow the [rustdoc](https://doc.rust-lang.org/rust-by-example/meta/doc.html) convention with line breaks required if a line exceeds 100 characters. You should ensure that any documentation relevant to a change you are making is updated as part of that change. If a new feature is being added, you should ensure that it is documented appropriately.

### Changelog

Do not manually write entries in any of the `CHANGELOG.md` files in this repo - the maintainers will handle this.

### Versioning

We use [Semantic Versioning](https://semver.org/) in this project, however contributors should not modify the versions of any crates as part of their pull request. If a change is breaking according to SemVer, then that should be indicated in the PR description - but whether it is or not, it will be checked by maintainers before the next release.

&nbsp;

### Pre-submission checklist

Before submitting a PR, run through the following checklist:

1. Repo forked and branch created from `next` according to the naming convention.
2. Every commit is [signed](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification#ssh-commit-signature-verification).
3. Commit messages and code style follow conventions.
4. Tests added for new functionality.
5. Documentation/comments updated for all changes according to our documentation convention.
6. `cargo make format`, `cargo make clippy`, and `cargo make unused` lints produce no errors.
7. Branch is rebased on the latest changes in `next`.

&nbsp;

## Write bug reports with detail, background, and sample code

**Great Bug Reports** tend to have:

- A quick summary and/or background
- Steps to reproduce
- What you expected would happen
- What actually happens
- Notes (possibly including why you think this might be happening, or stuff you tried that didn't work)

&nbsp;

## Contributions are licensed under the project LICENSE

When you submit code changes in a pull request, you are agreeing to license that contribution under the _project's_ dual license ([MIT](./LICENSE-MIT) and [Apache 2.0](./LICENSE-APACHE)). If you do not agree to this, then you must not submit pull requests to this project.
