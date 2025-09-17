# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0]

### BREAKING
- Remove low-level WIT interfaces for Miden standard library and transaction protocol library and link directly using stub library and transforming the stub functions into calls to MASM procedures.

## [0.0.8](https://github.com/0xMiden/compiler/compare/miden-v0.0.7...miden-v0.0.8) - 2025-04-24

### Added
- *(sdk)* introduce miden-sdk-alloc
- introduce TransformStrategy and add the "return-via-pointer"
- lay out the Rust Miden SDK structure, the first integration test

### Fixed
- fix value type in store op in `return_via_pointer` transformation,

### Other
- treat warnings as compiler errors,
- [**breaking**] revamp Miden SDK API and expose some modules;
- [**breaking**] rename `miden-sdk` crate to `miden` [#338](https://github.com/0xMiden/compiler/pull/338)
- release-plz update (bumped to v0.0.7)
- 0.0.6
- switch all crates to a single workspace version (0.0.5)
- bump all crate versions to 0.0.5
- bump all crate versions to 0.0.4 [#296](https://github.com/0xMiden/compiler/pull/296)
- `release-plz update` (bump versions, changelogs)
- `release-plz update` to update crate versions and changelogs
- set `miden-sdk-alloc` version to `0.0.0` to be in sync with
- delete `miden-tx-kernel-sys` crate and move the code to `miden-base-sys`
- `release-plz update` in `sdk` folder (SDK crates)
- fix typos ([#243](https://github.com/0xMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- rename `miden-sdk-tx-kernel` to `miden-tx-kernel-sys`
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- start guides for developing in rust in the book,
- introduce `miden-prelude` crate for intrinsics and stdlib
- remove `dylib` from `crate-type` in Miden SDK crates
- optimize rust Miden SDK for size
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/miden-sdk-v0.0.5...miden-sdk-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/miden-sdk-v0.0.1...miden-sdk-v0.0.2) - 2024-08-30

### Other
- updated the following local packages: miden-base-sys, miden-stdlib-sys, miden-sdk-alloc

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/miden-sdk-v0.0.0...miden-sdk-v0.0.1) - 2024-07-18

### Added
- introduce TransformStrategy and add the "return-via-pointer"
- lay out the Rust Miden SDK structure, the first integration test

### Fixed
- fix value type in store op in `return_via_pointer` transformation,

### Other
- set crates versions to 0.0.0, and `publish = false` for tests
- rename `miden-sdk-tx-kernel` to `miden-tx-kernel-sys`
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- start guides for developing in rust in the book,
- introduce `miden-prelude` crate for intrinsics and stdlib
- remove `dylib` from `crate-type` in Miden SDK crates
- optimize rust Miden SDK for size
