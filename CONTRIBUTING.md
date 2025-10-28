# Contributing to Miden Compiler

TBD

## Release Process

### Release of the Miden Compiler

1. Merging to `main` will create a new release PR containing any unreleased changes.
2. Optional. Change the proposed crate version, CHANGELOG edits.
3. The release PR gets merged to `main` when we are ready to publish the release.
4. The crates are published to crates.io, a new git tag is created, as well as a GitHub release
5. A job is run to pre-build the executable for our supported targets and upload them to the created Github release.
6. Merge the `main` branch back to the `next` branch.

### Release of the Miden SDK crates

1. Create a release PR naming the branch with the `release-plz-` prefix (its important to use this prefix to trigger the crate publishing on CI in one of the next steps).
2. Bump the SDK crates versions and update the CHANGELOG.
3. Merge it into the main branch.
4. The CI will automatically run `release-plz release` after the release PR is merged to publish the new versions to crates.io.
5. Set a git tag for the published crates to mark the release.
6. Make a Github release.
7. Merge the `main` branch back to the `next` branch.
