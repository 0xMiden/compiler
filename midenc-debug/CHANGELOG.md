# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/0xMiden/compiler/compare/midenc-debug-v0.4.0...midenc-debug-v0.4.1) - 2025-09-03

### Other

- Add 128-bit wide arithmetic support to the compiler.

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-debug-v0.1.5...midenc-debug-v0.4.0) - 2025-08-15

### Fixed

- handle empty iterator returned by `into_remainder()`
- remove incorrect(order) `FromMidenRepr::from_words()` for `[Felt; 4]`

### Other

- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)
- add `test_hmerge` integration test for `hmerge` Rust API

## [0.1.5](https://github.com/0xMiden/compiler/compare/midenc-debug-v0.1.0...midenc-debug-v0.1.5) - 2025-07-01

### Fixed

- invoke `init` in the lifting function prologue, load the advice

### Other

- add format for entrypoint option

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-debug-v0.0.7...midenc-debug-v0.0.8) - 2025-04-24

### Added
- *(types)* clean up hir-type for use outside the compiler
- *(codegen)* migrate to element-addressable vm
- add custom dependencies to `Executor` resolver,
- *(cargo-miden)* support building Wasm component from a Cargo project

### Fixed
- *(codegen)* incomplete global/data segment lowering

### Other
- *(codegen)* implement initial tests for load_sw/load_dw intrinsics
- update rust toolchain, clean up deps
- enrich Miden package loading error with the file path
- rename hir2 crates
- disable compilation of old hir crates, clean up deps
- switch uses of hir crates to hir2
- update to the latest `miden-mast-package` (renamed from
- update the Miden VM with updated `miden-package` crate
- update rust toolchain to 1-16 nightly @ 1.86.0
- Update midenc-debug/src/exec/executor.rs
- fix doc test false positive
- switch to `Package` without rodata,
- [**breaking**] move `Package` to `miden-package` in the VM repo

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-debug-v0.0.6...midenc-debug-v0.0.7) - 2024-09-17

### Other
- update rust toolchain

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-debug-v0.0.5...midenc-debug-v0.0.6) - 2024-09-06

### Added
- implement 'midenc run' command

### Other
- revisit/update documentation and guides
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-debug-v0.0.1...midenc-debug-v0.0.2) - 2024-08-30

### Fixed
- *(codegen)* broken return via pointer transformation
- *(debugger)* infinite loop in breakpoint id computation

### Other
- fix clippy warnings in tests

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-debug-v0.0.0...midenc-debug-v0.0.1) - 2024-08-16

### Other
- set `midenc-debug` version to `0.0.0` to be in sync with crates.io
- clean up naming in midenc-debug
- rename midenc-runner to midenc-debug
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit
