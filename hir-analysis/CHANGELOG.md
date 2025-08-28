# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/0xMiden/compiler/compare/midenc-hir-analysis-v0.4.0...midenc-hir-analysis-v0.4.1) - 2025-08-28

### Other

- switch to `expect_file!` in spills tests
- increase stack pressure in the `spills_loop_nest` test and make it spill
- move `tests` module in spill analysis into a separate file
- rewrite spill analysis tests, add logging

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-hir-analysis-v0.1.5...midenc-hir-analysis-v0.4.0) - 2025-08-15

### Other

- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-hir-analysis-v0.0.7...midenc-hir-analysis-v0.0.8) - 2025-04-24

### Added
- implement cross-context Call op, rename IR Exec back to Call

### Fixed
- *(codegen)* incomplete global/data segment lowering

### Other
- treat warnings as compiler errors,
- update rust toolchain, clean up deps
- rename hir2 crates
- remove old contents of hir, hir-analysis, hir-transform
- update rust toolchain to 1-16 nightly @ 1.86.0
- normalize use of fxhash-based hash maps
- rename Call IR op to Exec

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-analysis-v0.0.6...midenc-hir-analysis-v0.0.7) - 2024-09-17

### Other
- fix up new clippy warnings

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-hir-analysis-v0.0.5...midenc-hir-analysis-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.3](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-analysis-v0.0.2...midenc-hir-analysis-v0.0.3) - 2024-08-30

### Other
- Merge pull request [#284](https://github.com/0xPolygonMiden/compiler/pull/284) from 0xPolygonMiden/bitwalker/abi-transform-test-fixes

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-analysis-v0.0.1...midenc-hir-analysis-v0.0.2) - 2024-08-28

### Fixed
- *(frontend-wasm)* reserve memory allocated for use by rust

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-analysis-v0.0.0...midenc-hir-analysis-v0.0.1) - 2024-07-18

### Added
- *(analysis)* implement iterated dominance frontier queries
- *(analysis)* support querying strict dominance, improve docs
- *(analysis)* implement spills analysis
- *(analysis)* implement def-use analysis
- *(ir)* add spill/reload pseudo-ops
- *(ir)* add support for declaring function-local variables
- *(ir)* add support for 128-bit immediates
- *(analysis)* add support for querying the dominance frontier of a value
- *(analysis)* improve liveness precision, track operand stack presssure
- implement memset, memcpy, mem_grow, mem_size, and bitcast ops
- implement ilog2/clz/ctz/clo/cto instructions
- implement parser for sexpr-based hir format
- implement compiler driver, update midenc
- implement validation pass and supporting infrastructure
- improve ergonomics of a few key hir types

### Fixed
- *(analysis)* choose better data structure for dominance frontier
- move add_data_dependency to dependency graph
- use more descriptive names for tag constants in depgraph
- address some bugs in liveness analysis
- improper handling of inlined blocks in inline-blocks transform
- be more explicit about overflow, address some bugs found while testing
- missing liveness data for branch arguments
- hir and hir-analysis tests

### Other
- Merge pull request [#244](https://github.com/0xPolygonMiden/compiler/pull/244) from 0xPolygonMiden/bitwalker/load-store-reordering
- clean up docs and implementation of spills rewrite
- *(analysis)* improve dominance frontier docs
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- Fix descriptions for crates
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- ensure all relevant crates are prefixed with `midenc-`
- run clippy on CI, fix all clippy warnings
- add formatter config, format most crates
- update rust toolchain to latest nightly
- a few minor improvements
- *(docs)* fix typos
- set up mdbook deploy
- add guides for compiling rust->masm
- improve errors produced by various assertions, remove/tweak others
- grammar and other assorted tweaks/improvements
- extract codegen analyses, tweak module structure
- add mdbook skeleton
- clear up clippy warnings
- finalize pass refactoring, implement driver
- rework pass infrastructure for integration with driver
- use workspace deps rather than paths for hir crates
- clean up clippy warnings
- add analysis tests
- fix clippy warnings/errors
- rework the ir to better suit wasm->masm
- split up hir crate
- provide some initial usage instructions
- Initial commit
