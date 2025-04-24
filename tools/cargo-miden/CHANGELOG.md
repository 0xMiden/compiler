# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.8](https://github.com/0xMiden/compiler/compare/cargo-miden-v0.0.7...cargo-miden-v0.0.8) - 2025-04-24

### Added
- switch cargo-miden to use v0.7.0 tag in rust-templates repo
- add `--program`, `--account`, `--note` flags
- draft wasm/masm output request in cargo-miden
- *(cargo-miden)* support building Wasm component from a Cargo project

### Fixed
- although we support only `--program` in cargo-miden,
- hide --account and --note options, make --program option default,
- refine `Component` imports and exports to reference module imports
- cast `count` parameter in Wasm `memory.copy` op to U32;

### Other
- treat warnings as compiler errors,
- update rust toolchain, clean up deps
- switch to the published version `v0.21` of the `cargo-component`
- emit optimized ir, rather than initial translated ir during tests
- switch uses of hir crates to hir2
- update `cargo-component` (without `env_vars` parameter)
- set `RUSTFLAGS` env var instead of passing it to
- update wit-bindgen and cargo-component to the latest patched versions
- update `cargo miden new` to use `v0.8.0` of the program template
- update to the latest `miden-mast-package` (renamed from
- optimize codegen for `AccountId::as_felt`;
- add note script compilation test;
- [**breaking**] rename `miden-sdk` crate to `miden` [#338](https://github.com/0xMiden/compiler/pull/338)
- remove `miden:core-import/types` custom types
- clean up todos
- skip Miden SDK function generation in bindings.rs
- add check for the proper artifact file extension in the cargo-miden test

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/cargo-miden-v0.0.6...cargo-miden-v0.0.7) - 2024-09-17

### Fixed
- switch cargo-miden to v0.4.0 of the new project template with

### Other
- update cargo-component to v0.16.0
- add `CompilerTestInputType::CargoMiden` and use `cargo-miden` build it

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/cargo-miden-v0.0.5...cargo-miden-v0.0.6) - 2024-09-06

### Other
- fix mkdocs warnings, move cargo-miden README to docs.
- clean up unused deps
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/cargo-miden-v0.0.1...cargo-miden-v0.0.2) - 2024-08-30

### Other
- update Cargo.lock dependencies

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/cargo-miden-v0.0.0...cargo-miden-v0.0.1) - 2024-07-18

### Added
- *(cargo)* allow configuring compiler version for generated projects
- revamp cargo-miden to pass the unrecognized options to cargo,

### Fixed
- rename `compile` cargo extension command to `build`; imports cleanup;

### Other
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- run clippy on CI, fix all clippy warnings
- Merge pull request [#164](https://github.com/0xPolygonMiden/compiler/pull/164) from 0xPolygonMiden/greenhat/i163-cargo-ext-for-alpha
- remove repetitive words
- add formatter config, format most crates
- a few minor improvements
- sync cargo-component crate version to v0.6 in test apps
- set up mdbook deploy
- add guides for compiling rust->masm
- Merge pull request [#61](https://github.com/0xPolygonMiden/compiler/pull/61) from 0xPolygonMiden/greenhat/cargo-ext-i60
- make `WasmTranslationConfig::module_name_fallback` non-optional
- remove `path-absolutize` dependency
- remove `next_display_order` option in `Command`
- move cargo-ext to tools/cargo-miden
- provide some initial usage instructions
- Initial commit
