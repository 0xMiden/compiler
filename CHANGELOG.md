# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## `cargo-miden` - [0.5.2](https://github.com/0xMiden/compiler/compare/0.5.1...0.5.2) - 2025-12-23

### Fixed
- use git tag for new Miden project template (`cargo miden new`)
- *(cargo-miden)* allow local override of project-template templates
- rust-lld fails due to divergent definitions of __rdl_alloc_error_handler

### Other
- update git tags in `cargo miden new` command
- bump rust-templates to v0.23.0
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Merge pull request #747 from crStiv/feat/add-workspace-support-to-new-command
- Add tests for new project workspace integration
- Update new_project.rs

## `midenc` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-v0.5.1...midenc-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-driver` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-driver-v0.5.1...midenc-driver-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-compile` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-compile-v0.5.1...midenc-compile-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- bump toolchain to 1.94/nightly-2025-12-10
- Merge pull request #785 from walnuthq/pr/fix-source-loc-resolution
- Fix source locations when trim-paths is being used

## `midenc-frontend-wasm` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-frontend-wasm-v0.5.1...midenc-frontend-wasm-v0.5.2) - 2025-12-23

### Fixed
- remove sorting the targets in `br_table` Wasm op translation

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- Fix source locations when trim-paths is being used

## `midenc-hir-eval` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-hir-eval-v0.5.1...midenc-hir-eval-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- bump toolchain to 1.94/nightly-2025-12-10

## `midenc-expect-test` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-expect-test-v0.5.1...midenc-expect-test-v0.5.2) - 2025-12-23

### Other
- switch to Rust edition 2024

## `midenc-codegen-masm` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.5.1...midenc-codegen-masm-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Optimise the `realign_dw` memory intrinsic slightly.
- Change the memory representation of 64-bit, dual-limbed values to be little-endian.

## `midenc-dialect-scf` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-dialect-scf-v0.5.1...midenc-dialect-scf-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-dialect-ub` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-dialect-ub-v0.5.1...midenc-dialect-ub-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-dialect-hir` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-dialect-hir-v0.5.1...midenc-dialect-hir-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024

## `midenc-hir-transform` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-hir-transform-v0.5.1...midenc-hir-transform-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-hir-analysis` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-hir-analysis-v0.5.1...midenc-hir-analysis-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024

## `midenc-dialect-cf` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-dialect-cf-v0.5.1...midenc-dialect-cf-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024

## `midenc-dialect-arith` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-dialect-arith-v0.5.1...midenc-dialect-arith-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-hir` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-hir-v0.5.1...midenc-hir-v0.5.2) - 2025-12-23

### Fixed
- DomTreeSuccessorIter::next_back bounds handling

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Fix source locations when trim-paths is being used

## `midenc-session` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-session-v0.5.1...midenc-session-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Fix source locations when trim-paths is being used

## `midenc-hir-symbol` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-hir-symbol-v0.5.1...midenc-hir-symbol-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- bump toolchain to 1.94/nightly-2025-12-10

## `midenc-hir-macros` - [0.5.2](https://github.com/0xMiden/compiler/compare/midenc-hir-macros-v0.5.1...midenc-hir-macros-v0.5.2) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
