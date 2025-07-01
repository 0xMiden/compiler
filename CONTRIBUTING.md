# Contributing to Miden Compiler

TBD

## Release Process

### Prerequisites

Install `release-plz` CLI tool following the instructions [here](https://release-plz.ieni.dev/docs/usage/installation)
Install `cargo-semver-checks` CLI tool [here](https://github.com/obi1kenobi/cargo-semver-checks#installation) to use as an extra check in `release-plz` and bump the major versions on semver violations.

### Release of the Miden Compiler and Miden SDK crates

The release process for the Miden Compiler and Miden SDK is managed using the `release-plz` tool. The following steps outline the process for creating a new release:

1. Run `release-plz update` in the repo root folder to update the crates versions and generate changelogs.
2. Create a release PR naming the branch with the `release-plz-` prefix (its important to use this prefix to trigger the crate publishing on CI in step 4).
3. Review the changes in the release PR, commit edits if needed and merge it into the main branch.
4. The CI will automatically run `release-plz release` after the release PR is merged to publish the new versions to crates.io.
5. Set a git tag for the published crates to mark the release.
