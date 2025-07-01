# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5](https://github.com/0xMiden/compiler/compare/miden-base-sys-v0.1.0...miden-base-sys-v0.1.5) - 2025-07-01

### Added

- add Miden SDK `note::get_assets` Rust bindings

### Fixed

- `note` Miden SDK bindings for element-addressable memory in Miden VM #550
- wasm import module names to be in sync with WIT files (Miden SDK)

## [0.0.8](https://github.com/0xMiden/compiler/compare/miden-base-sys-v0.0.7...miden-base-sys-v0.0.8) - 2025-04-24

### Added
- *(frontend)* Low-level account storage API in Miden SDK
- *(cargo-miden)* support building Wasm component from a Cargo project

### Fixed
- include `account` module in `MidenTxKernelLibrary`;
- fix clippy warnings

### Other
- treat warnings as compiler errors,
- make index `u8` in account storage API in Miden SDK,
- add missing functions in miden::account, miden:note tx kernel stubs
- optimize codegen for `AccountId::as_felt`;
- add note script compilation test;
- [**breaking**] revamp Miden SDK API and expose some modules;
- remove digest-in-function-name encoding and `MidenAbiImport::digest`,

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/miden-base-sys-v0.0.6...miden-base-sys-v0.0.7) - 2024-09-17

### Other
- remove `miden-assembly` dependency from `sdk/base-sys` for `bindings` feature

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/miden-base-sys-v0.0.5...miden-base-sys-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.3](https://github.com/0xPolygonMiden/compiler/compare/miden-base-sys-v0.0.2...miden-base-sys-v0.0.3) - 2024-08-30

### Other
- Merge pull request [#284](https://github.com/0xPolygonMiden/compiler/pull/284) from 0xPolygonMiden/bitwalker/abi-transform-test-fixes

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/miden-base-sys-v0.0.1...miden-base-sys-v0.0.2) - 2024-08-28

### Fixed
- *(sdk)* be more explicit about alignment of felt/word types
- *(sdk)* improper handling of get_inputs vec after return into rust

### Other
- remove miden-diagnostics, start making midenc-session no-std-compatible

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/miden-base-sys-v0.0.0...miden-base-sys-v0.0.1) - 2024-08-16

### Fixed
- fix the build after VM v0.10.3 update

### Other
- delete `miden-tx-kernel-sys` crate and move the code to `miden-base-sys`
- build the MASL for the tx kernel stubs in `build.rs` and
- rename `midenc-tx-kernel` to `miden-base-sys` and move it to
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit
