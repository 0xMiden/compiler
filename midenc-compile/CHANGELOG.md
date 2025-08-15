# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-compile-v0.1.5...midenc-compile-v0.4.0) - 2025-08-15

### Added

- implement advice map API in Miden SDK
- add `crypto::hmerge()` in Miden SDK (`hmerge` VM intruction);

### Other

- rename `io` to `advice`, export modules in stdlib SDK

## [0.1.5](https://github.com/0xMiden/compiler/compare/midenc-compile-v0.1.0...midenc-compile-v0.1.5) - 2025-07-01

### Fixed

- allow linking MASM modules without procedures type info to

### Other

- add format for entrypoint option
- remove unused `LiftExportsCrossCtxStage`

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-compile-v0.0.7...midenc-compile-v0.0.8) - 2025-04-24

### Added
- *(frontend)* Low-level account storage API in Miden SDK
- *(frontend)* generate `CanonLower` synthetic functions for
- *(frontend)* generate `CanonLift` synthetic functions for exports
- break out arith, ub, cf, and scf dialects from hir dialect
- *(driver)* improve targeting of logs/tracing
- add `CallConv::CrossCtx` calling convention for cross-context
- implement CanonABI type flattening for app relevant `Type` variants
- draft Wasm CM function type flattening, checks for scalar type
- on Miden CCABI lifting/lowering get import/export function signature from
- store the imported function type in the component import
- rewrite the function calls from the imported function (Miden CCABI) to
- draft lifting function generation in `LiftImportsCrossCtxStage`
- draft `LiftImportsCrossCtxStage` scaffolding
- draft `LowerExportsCrossCtxStage` implementation with a lot of

### Fixed
- missing wasmparser feature causing cargo build failure
- *(driver)* ensure codegen dialect hooks are registered
- use import/export core function's span for the generated

### Other
- treat warnings as compiler errors,
- *(codegen)* implement initial tests for load_sw/load_dw intrinsics
- add some missing log targets
- Move the new Wasm frontend to `frontend/wasm` and remove the old
- update rust toolchain, clean up deps
- rename hir2 crates
- avoid unnecessary recompilation of artifacts
- spills implementation, a variety of bug fixes, work on is_prime test
- expose subset of compiler pipeline for emitting optimized ir in tests
- *(driver)* specify region simplification level in rewrite stage
- switch uses of hir crates to hir2
- switch compiler to hir2
- switch from recognizing intrinsics module by name(substring)
- update to the latest `miden-mast-package` (renamed from
- update the Miden VM with updated `miden-package` crate
- update rust toolchain to 1-16 nightly @ 1.86.0
- add comments for the handling of the lifting/lowering in the future
- replace `CallConv::CrossCtx` with `CanonLift` and `CanonLower`
- rename `LowerExportsCrossCtxStage` -> `LiftExportsCrossCtxStage`
- remove the hard-coded `cross-ctx*` checks in Miden CCABI
- rename `CanonAbiImport::interface_function_ty`
- move `LiftImportsCrossCtxStage`, `LowerExportsCrossCtxStage` to the new `cross_ctx` module
- add stages scaffolding for lifting/lowering cross-context calls
- switch from passing `Module` to `Component` in the compiler stages
- switch to `Package` without rodata,
- [**breaking**] move `Package` to `miden-package` in the VM repo

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-compile-v0.0.5...midenc-compile-v0.0.6) - 2024-09-06

### Fixed
- *(driver)* ensure mast/masl outputs are emitted on request

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-compile-v0.0.0...midenc-compile-v0.0.1) - 2024-07-18

### Added
- enable spills transformation in default pipeline
- implement most i64 ops and intrinsics, fix some 64-bit bugs
- parse Wasm components

### Fixed
- centralize management of compiler rewrite pipeline
- `FileName::as_str` to avoid enclosing virtual filenames in brackets
- link intrinsics modules in the `CodengenStage` of the midenc
- missing diagnostics on parse error in midenc
- tweak wasm frontend and related test infra
- incorrect module name when compiling wasm
- emit Session artifact in CodegenStage, don't require Session.matches in ApplyRewritesStage
- properly handle emitting final artifacts in midenc-compile

### Other
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- ensure all relevant crates are prefixed with `midenc-`
- run clippy on CI, fix all clippy warnings
- use midenc driver for non-cargo-based fixtures in
- use midenc driver to compile cargo-based fixtures
- handle assembler refactoring changes
- add formatter config, format most crates
- Merge pull request [#100](https://github.com/0xPolygonMiden/compiler/pull/100) from 0xPolygonMiden/greenhat/i89-translate-wasm-cm
- a few minor improvements
- *(docs)* fix typos
- set up mdbook deploy
- add guides for compiling rust->masm
- Merge pull request [#61](https://github.com/0xPolygonMiden/compiler/pull/61) from 0xPolygonMiden/greenhat/cargo-ext-i60
- make `WasmTranslationConfig::module_name_fallback` non-optional
- switch from emiting MASM in CodegenStage, and switch to output folder in cargo extension
- split up driver components into separate crates
- provide some initial usage instructions
- Initial commit
