# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-hir-transform-v0.0.7...midenc-hir-transform-v0.0.8) - 2025-04-24

### Fixed
- *(ir)* address a few aliasing violations when full tracing is enabled

### Other
- treat warnings as compiler errors,
- rename hir2 crates
- remove old contents of hir, hir-analysis, hir-transform
- normalize use of fxhash-based hash maps
- rename Call IR op to Exec

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-transform-v0.0.6...midenc-hir-transform-v0.0.7) - 2024-09-17

### Other
- fix up new clippy warnings

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-hir-transform-v0.0.5...midenc-hir-transform-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-transform-v0.0.1...midenc-hir-transform-v0.0.2) - 2024-08-28

### Fixed
- inoperative --print-ir-after-*, add --print-cfg-after-*

### Other
- add additional tracing to treeify pass
- remove miden-diagnostics, start making midenc-session no-std-compatible

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-transform-v0.0.0...midenc-hir-transform-v0.0.1) - 2024-07-18

### Added
- enable spills transformation in default pipeline
- *(transform)* implement spill transform
- implement parser for sexpr-based hir format
- implement compiler driver, update midenc
- prettify assertions on textual ir
- implement hir transforms

### Fixed
- *(transform)* address remaining bugs in spills transformation
- *(pass)* refactor treeify pass
- improper handling of inlined blocks in inline-blocks transform
- be more explicit about overflow, address some bugs found while testing
- bug in treeify pass due to shallow instruction cloning
- make dfg.inst return instruction, and dfg.inst_node the node

### Other
- clean up docs and implementation of spills rewrite
- transform tests, fixes
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- Fix descriptions for crates
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- ensure all relevant crates are prefixed with `midenc-`
- add formatter config, format most crates
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- clear up clippy warnings
- finalize pass refactoring, implement driver
- rework pass infrastructure for integration with driver
- update tests broken due to formatter changes
- initial parser implementation
- use workspace deps rather than paths for hir crates
- add support for real spans in tests
- clean up clippy warnings
- treeify pass
- split critical edges pass
- inline blocks pass
- fix clippy warnings/errors
- provide some initial usage instructions
- Initial commit
