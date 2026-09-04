# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.32.0]

### Templates

- Rust templates and the full-project scaffold's contracts now resolve compiled Miden dependencies
  during ordinary `cargo build`, `cargo check`, and IDE analysis. Their new build scripts stage
  dependencies without compiling the consuming crate twice, keeping the staged packages under
  Cargo's configured target directory.
- The full-project scaffold's CI installs the Miden toolchain before running integration tests.

### SDK compatibility

- Generated Rust crates use the same SDK version requirement, checkout, branch, or revision for
  `miden-sdk-build-script-support` and their runtime bindings. The program template includes build
  support without requiring a guest SDK dependency.

### Migration and breaking changes

- Update Rust toolchain pins from `nightly-2026-04-30` to `nightly-2026-09-01` and SDK requirements
  from `miden = "0.14.0-rc.1"` to `miden = "0.14"`. The full-project scaffold adds
  `miden-toolchain.toml` selecting Miden toolchain channel `0.16.0`; install that channel when
  adopting the scaffold. See the [SDK release notes](../../sdk/CHANGELOG.md#0140) for guest-code
  migrations.
- To enable plain Cargo and IDE support in existing crates, add
  `miden-sdk-build-script-support = "0.14"` under `[build-dependencies]` and call
  `miden_sdk_build_script_support::prepare_package_cache()` from `build.rs`'s `main`. Use the same
  compiler checkout, branch, or revision for the helper when selecting an unpublished SDK. These
  builds now require an installed `cargo-miden` on `PATH` or selected through `CARGO_MIDEN`;
  dependency-staging failures fail the outer build or check. Compiler-source selections configure
  Cargo dependencies, so select the matching build executable separately.
- Full-project contracts now honor compiler path, branch, and revision selections for their SDK
  dependencies; previously they always used the published SDK. Remove those selections when the
  published SDK is intended.
- In existing `miden-project.toml` files, add `path = "src/lib.rs"` under `[lib]` for account,
  authentication-component, note, and transaction-script crates, including the project contracts.
  For program crates, replace `[[bin]].path = "<virtual>"` with `path = "src/lib.rs"` so the
  compiler can load the target.
- Note, transaction-script, and project increment-note templates now read dependency interfaces
  from compiled packages. Rebuild dependencies with the current SDK/compiler and remove obsolete
  `wit` overrides under `[package.metadata.miden.dependencies]` pointing at `target/generated-wit/`
  when those packages embed WIT. Keep the ordinary `[dependencies]` entries.
- The full-project integration dependencies move from client/protocol `0.15` and MAST `0.23` to the
  client/protocol `0.16` prerelease family, MAST `0.29`, and `rand 0.10`. Update the integration
  manifest, lockfile, helpers, and tests together. Remove `ClientBuilder::in_debug_mode`, replace
  `with_auth_component` with `with_component`, and construct authentication with
  `AuthSingleSig::from_public_key(public_key)` instead of `AuthSingleSig::new(commitment, scheme)`.
- In copied integration tests, replace `build_tx_context(account, &[note_id], &[])` with
  `build_transaction(account).authenticated_input_note(note_id)`, wrap storage-map lookup keys in
  `StorageMapKey::new`, and import `rand::Rng` instead of `rand::RngCore`.
- Integration tests now invoke installed build tools instead of a `cargo-miden` Rust dependency.
  Install the tools before running `cargo test`. The helper uses `miden build` when `MIDENUP_HOME`
  is set, otherwise `CARGO_MIDEN` or `cargo miden build`; unset `MIDENUP_HOME` to use a custom
  `CARGO_MIDEN`. Integration-test artifacts now go to
  `<contract>/target/miden/{debug|release}/out.masp`; update scripts that locate them.
- Add `#[account_procedure]` to the counter account's `get_count` and `increment_count` trait
  methods when updating an existing scaffold. Without these markers, account construction fails
  because the component has no non-authentication account procedures.
- The full-project scaffold no longer ships contract `Cargo.lock` files and now ignores
  `contracts/**/Cargo.lock`, so contract dependencies resolve from their manifests. The integration
  workspace still ships its root lockfile. Generate contract lockfiles before using `--locked`;
  retain existing lockfiles and adjust the ignore rule if your project tracks them.

## [0.32.0-rc.1]

This release migrated the templates from the [rust-templates](https://github.com/0xMiden/rust-templates) and [project-template](https://github.com/0xMiden/project-template) repositories under the compiler monorepo. No significant changes have been made since the v0.31.0 release of `rust-templates` (the `project-template` repo has had no versioned releases, but going forward both sets of templates will share a single release version).
