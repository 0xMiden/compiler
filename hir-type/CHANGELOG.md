# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-hir-type-v0.0.7...midenc-hir-type-v0.0.8) - 2025-04-24

### Added
- *(types)* clean up hir-type for use outside the compiler
- implement pretty-print trait for Symbol/Type

### Other
- treat warnings as compiler errors,
- update rust toolchain, clean up deps
- implement hir dialect ops, flesh out remaining core ir infra

## [0.0.7](https://github.com/0xMiden/compiler/compare/midenc-hir-type-v0.0.6...midenc-hir-type-v0.0.7) - 2024-09-17

### Other
- fix up new clippy warnings

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-hir-type-v0.0.5...midenc-hir-type-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.3](https://github.com/0xMiden/compiler/compare/midenc-hir-type-v0.0.2...midenc-hir-type-v0.0.3) - 2024-08-30

### Fixed
- *(codegen)* broken return via pointer transformation

### Other
- Merge pull request [#284](https://github.com/0xMiden/compiler/pull/284) from 0xMiden/bitwalker/abi-transform-test-fixes

## [0.0.2](https://github.com/0xMiden/compiler/compare/midenc-hir-type-v0.0.1...midenc-hir-type-v0.0.2) - 2024-08-28

### Added
- implement packaging prototype

## [0.0.1](https://github.com/0xMiden/compiler/compare/midenc-hir-type-v0.0.0...midenc-hir-type-v0.0.1) - 2024-07-18

### Added
- draft Miden ABI function types encoding and retrieval
- introduce Miden ABI component import
- introduce `CanonicalOptions` in IR and translate Wasm
- implement new sexpr-based format for hir
- rewrite type layout functionality
- refactor type layout primitives
- define type compatibility for operators
- provide type representation enum
- implement inline assembly
- distinguish signed/unsigned types, native/emulated pointers

### Fixed
- issue with i1 widening casts
- felt representation mismatch between rust and miden
- *(ir)* incorrect entries in operand compatibility matrix
- use stabilized next_multiple_of in alignable impls
- switch text representation of the `MidenAbiFunctionType` to s-exp;
- rewrite incorrect type layout code

### Other
- fix typos ([#243](https://github.com/0xMiden/compiler/pull/243))
- Fix descriptions for crates
- set crates versions to 0.0.0, and `publish = false` for tests
- add a description for miden-hir-type crate
- ensure all relevant crates are prefixed with `midenc-`
- since all the Miden ABI transformation happens in the frontend
- add `FunctionType::abi` and ditch redundant `*FunctionType`
- add Wasm component translation support to the integration tests;
- add formatter config, format most crates
- update rust toolchain to latest nightly
- Merge pull request [#100](https://github.com/0xMiden/compiler/pull/100) from 0xMiden/greenhat/i89-translate-wasm-cm
- move `LiftedFunctionType` to `miden-hir-type` crate
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- rework the ir to better suit wasm->masm
- split up hir crate
- provide some initial usage instructions
- Initial commit
