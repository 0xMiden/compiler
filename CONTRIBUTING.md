# Contributing to Miden Compiler

TBD

## Release Process

### 1. Release of the Miden SDK crates

1. Create a release PR against the `next` branch naming the branch with the `release-plz-` prefix (its important to use this prefix to trigger the crate publishing on CI in the later step).
2. Manually bump ALL the SDK crate versions and update the `sdk/sdk/CHANGELOG.md`
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

