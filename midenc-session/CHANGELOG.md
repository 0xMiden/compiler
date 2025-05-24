# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-session-v0.0.7...midenc-session-v0.0.8) - 2025-04-24

### Added
- implement pass infrastructure

### Fixed
- return error (instead of panic) when package contain a Program

### Other
- treat warnings as compiler errors,
- add some missing log targets
- update rust toolchain, clean up deps
- enrich Miden package loading error with the file path
- rename hir2 crates
- switch compiler to hir2
- update to the latest `miden-mast-package` (renamed from
- update the Miden VM with updated `miden-package` crate
- update rust toolchain to 1-16 nightly @ 1.86.0
- fix formatting after rebase
- finish initial rewrite of backend using hir2
- [**breaking**] move `Package` to `miden-package` in the VM repo
- add note script compilation test;

## [0.0.7](https://github.com/0xMiden/compiler/compare/midenc-session-v0.0.6...midenc-session-v0.0.7) - 2024-09-17

### Other
- update rust toolchain

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-session-v0.0.5...midenc-session-v0.0.6) - 2024-09-06

### Fixed
- *(driver)* incorrect extension for masl output type

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.4](https://github.com/0xMiden/compiler/compare/midenc-session-v0.0.3...midenc-session-v0.0.4) - 2024-08-30

### Other
- update Cargo.toml dependencies

## [0.0.3](https://github.com/0xMiden/compiler/compare/midenc-session-v0.0.2...midenc-session-v0.0.3) - 2024-08-28

### Added
- implement packaging prototype

### Fixed
- inoperative --print-ir-after-*, add --print-cfg-after-*
- incorrect handling of -C/-Z and --emit=hir options
- regression in midenc compile
- address overhead of deserializing the stdlib

### Other
- remove miden-diagnostics, start making midenc-session no-std-compatible

## [0.0.2](https://github.com/0xMiden/compiler/compare/midenc-session-v0.0.1...midenc-session-v0.0.2) - 2024-08-16

### Added
- *(codegen)* propagate source spans from hir to masm

### Fixed
- add tx kernel library with stubs and link it on `-l miden`
- infer link libraries from target env
- tweak session init handling of outputs
- various tests, cli bugs, vm test executor, test builder api
- *(cli)* improve help output, hide plumbing flags
- clap error formatting, unused deps

### Other
- delete `miden-tx-kernel-sys` crate and move the code to `miden-base-sys`
- rename `midenc-tx-kernel` to `miden-base-sys` and move it to
- update to miden v0.10.3
- update to miden v0.10.2
- improve behavior of frontend config
- clean up driver init and output config
- fix various clippy warnings, bug in wasm br_table lowering, expect output
- move miden-vm deps to latest commit included in 0.10 releasef
- support compiled libraries, linker flags
- update to latest miden vm patchset
- unify diagnostics infa between compiler, assembler, vm
- unify compilation, rodata init, test harness

## [0.0.1](https://github.com/0xMiden/compiler/compare/midenc-session-v0.0.0...midenc-session-v0.0.1) - 2024-07-18

### Added
- implement compiler driver, update midenc

### Fixed
- tweak wasm frontend and related test infra
- properly handle emitting final artifacts in midenc-compile

### Other
- fix typos ([#243](https://github.com/0xMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- ensure all relevant crates are prefixed with `midenc-`
- use midenc driver to compile cargo-based fixtures
- handle assembler refactoring changes
- add formatter config, format most crates
- a few minor improvements
- *(docs)* fix typos
- set up mdbook deploy
- add guides for compiling rust->masm
- remove unused dependencies
- Merge pull request [#61](https://github.com/0xMiden/compiler/pull/61) from 0xMiden/greenhat/cargo-ext-i60
- add mdbook skeleton
- clear up clippy warnings
- split up driver components into separate crates
- finalize pass refactoring, implement driver
- rework pass infrastructure for integration with driver
- provide some initial usage instructions
- Initial commit
