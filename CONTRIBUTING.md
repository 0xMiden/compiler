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

1. Create a release PR against the `next` branch naming the branch with the `release-plz-` prefix (its important to use this prefix to trigger the crate publishing on CI in the later step).
2. Manually bump ALL the SDK crate versions and update the `sdk/sdk/CHANGELOG.md`
3. Review the changes in the release PR,  and merge it into the `next` branch.
4. The CI will automatically run `release-plz release` after the release PR is merged to publish the new versions to crates.io.
