# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Rust contracts now use a three-line `build.rs` backed by the version-matched
  `miden-sdk-build-script-support` crate, instead of carrying a private copy of the package-cache
  staging protocol.
- The project scaffold's contract crates no longer ship a `Cargo.lock`, matching the
  single-contract templates. They are `cdylib` libraries, and the committed lockfiles could only
  go stale against the SDK requirement in their own manifests — the ones that shipped were
  already pinned to an SDK the manifests could not resolve.

## [0.32.0-rc.1]

This release migrated the templates from the [rust-templates](https://github.com/0xMiden/rust-templates) and [project-template](https://github.com/0xMiden/project-template) repositories under the compiler monorepo. No significant changes have been made since the v0.31.0 release of `rust-templates` (the `project-template` repo has had no versioned releases, but going forward both sets of templates will share a single release version).
