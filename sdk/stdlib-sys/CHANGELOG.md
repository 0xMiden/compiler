# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/0xMiden/compiler/compare/miden-stdlib-sys-v0.1.5...miden-stdlib-sys-v0.4.0) - 2025-08-15

### Added

- implement advice map API in Miden SDK
- add `crypto::hmerge()` in Miden SDK (`hmerge` VM intruction);

### Fixed

- WIT interface for core Wasm module imports for Miden SDK
- change `align(32)` for `Word` to be `align(16)` #596
- `hmerge` function declaration in WIT (invalid argument names),

### Other

- use advice map API in the basic wallet tx script
- rename `io` to `advice`, export modules in stdlib SDK
- Add the test that executes counter contract, basic wallet and p2id note script on the local node ([#555](https://github.com/0xMiden/compiler/pull/555))
- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)
- Merge pull request #603 from 0xMiden/greenhat/i598-hmerge-pass-digest-ptr
- pass the pointer to `[Digest; 2]` to `hmerge` intrinsic #598
- add issue url in the comments
- `hmerge` intrinsic to accept digests as a pointer and load
- rename for readability, add comments
- make `miden_stdlib_sys::Digest` a newtype instead of type alias
- simplify the Rust part of the `hmerge` bindings

## [0.1.5](https://github.com/0xMiden/compiler/compare/miden-stdlib-sys-v0.1.0...miden-stdlib-sys-v0.1.5) - 2025-07-01

### Fixed

- add missing felt intrinsics in Miden SDK WIT file

### Other

- add globals to cross-ctx-account and note test projects
- fix doc comments

## [0.0.8](https://github.com/0xMiden/compiler/compare/miden-stdlib-sys-v0.0.7...miden-stdlib-sys-v0.0.8) - 2025-04-24

### Added
- add custom dependencies to `Executor` resolver,
- restore module and function names of the intrinsics and Miden
- *(cargo-miden)* support building Wasm component from a Cargo project

### Fixed
- refine `Component` imports and exports to reference module imports

### Other
- treat warnings as compiler errors,
- rename `Felt::from_u32_unchecked` to `Felt::from_u32`
- [**breaking**] revamp Miden SDK API and expose some modules;
- remove digest-in-function-name encoding and `MidenAbiImport::digest`,

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/miden-stdlib-sys-v0.0.5...miden-stdlib-sys-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.3](https://github.com/0xPolygonMiden/compiler/compare/miden-stdlib-sys-v0.0.2...miden-stdlib-sys-v0.0.3) - 2024-08-30

### Fixed
- *(codegen)* broken return via pointer transformation

### Other
- Merge pull request [#284](https://github.com/0xPolygonMiden/compiler/pull/284) from 0xPolygonMiden/bitwalker/abi-transform-test-fixes

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/miden-stdlib-sys-v0.0.1...miden-stdlib-sys-v0.0.2) - 2024-08-28

### Fixed
- *(sdk)* be more explicit about alignment of felt/word types

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/miden-stdlib-sys-v0.0.0...miden-stdlib-sys-v0.0.1) - 2024-07-18

### Fixed
- felt representation mismatch between rust and miden

### Other
- set crates versions to 0.0.0, and `publish = false` for tests
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- start guides for developing in rust in the book,
