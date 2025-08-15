# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-v0.1.5...midenc-v0.4.0) - 2025-08-15

### Other

- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-v0.0.7...midenc-v0.0.8) - 2025-04-24

### Other
- update Cargo.lock dependencies

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-v0.0.6...midenc-v0.0.7) - 2024-09-17

### Other
- update Cargo.lock dependencies

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-v0.0.5...midenc-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-v0.0.1...midenc-v0.0.2) - 2024-08-30

### Other
- update Cargo.lock dependencies

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-v0.0.0...midenc-v0.0.1) - 2024-07-18

### Added
- implement compiler driver, update midenc

### Other
- update deps and min rust version
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- run clippy on CI, fix all clippy warnings
- add formatter config, format most crates
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit
