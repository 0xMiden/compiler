# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5](https://github.com/0xMiden/compiler/compare/miden-sdk-alloc-v0.1.0...miden-sdk-alloc-v0.1.5) - 2025-07-01

### Fixed

- `BumpAlloc` to make all allocations minimally VM word-aligned (16 bytes)

## [0.0.8](https://github.com/0xMiden/compiler/compare/miden-sdk-alloc-v0.0.7...miden-sdk-alloc-v0.0.8) - 2025-04-24

### Added
- *(cargo-miden)* support building Wasm component from a Cargo project

### Other
- treat warnings as compiler errors,

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/miden-sdk-alloc-v0.0.5...miden-sdk-alloc-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/miden-sdk-alloc-v0.0.1...miden-sdk-alloc-v0.0.2) - 2024-08-30

### Other
- Merge pull request [#284](https://github.com/0xPolygonMiden/compiler/pull/284) from 0xPolygonMiden/bitwalker/abi-transform-test-fixes

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/miden-sdk-alloc-v0.0.0...miden-sdk-alloc-v0.0.1) - 2024-08-28

### Added
- *(sdk)* introduce miden-sdk-alloc

### Other
- set `miden-sdk-alloc` version to `0.0.0` to be in sync with
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit
