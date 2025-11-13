# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/0xMiden/compiler/compare/0.5.0...0.5.1) - 2025-11-13

### Other

- more clippy
- add `--single-branch` when cloning a template git repo
- more formatting and clippy
- clippy and formatting
- migrate to 0.19
- update default project template repo URL

## [0.5.0](https://github.com/0xMiden/compiler/compare/cargo-miden-v0.4.1...cargo-miden-v0.5.0) - 2025-11-05

### Added

- remove explicit `miden.wit` dependency and use the one bundled with Miden SDK
- convert all the `miden-base` functions to use .rlib stub library
- convert stdlib functions to use .rlib stub library
- convert the rest of the `intrinsics` to use .rlib stub library
- convert all `intrinsics::felt` to use .rlib stub library
- convert `add` felt intrinsic to use stub library instead of WIT interface
- convert `hmerge` intrinsic to use stub function instead of WIT interface
- lower function stub to MASM procedure call
- try rlib stub for `miden::account::add_asset`

### Fixed

- `cargo-miden` picks the correct package from cargo metadata
- account for generated WIT in `cargo miden example`
- fix the build after merging #666 and #678 without rebasing
- `cargo miden new` should skip `git init` if already in a git repo
- update new project templates tag to v0.15.0

### Other

- use v0.22.0 git tag for new project templates (SDK v0.7)
- Merge pull request #741 from 0xMiden/greenhat/i737-revamp-new
- Merge pull request #744 from 0xMiden/greenhat/i734-hide-panic-handler-and-allo-boilerplate
- update new project templates git tag to v0.20.0
- Merge pull request #701 from 0xMiden/chore/docusaurus-migration-new
- Merge pull request #721 from 0xMiden/greenhat/cargo-miden-cleanup
- *(cargo-miden)* remove `Color` in favor of `midenc::session::ColorChoice`
- delete commented out non_component.rs
- unify core Wasm and component build process,
- bump Miden SDK version to v0.6.0 and publish it
- `cargo-miden` to handle only it's own commands (new, build, example)
- update the git tag for new project templates
- clean up miden.wit materialization code
- Revert "Merge pull request #692 from 0xMiden/chore/docusaurus-migration-next"
- Merge pull request #692 from 0xMiden/chore/docusaurus-migration-next
- *(README)* add docs section explainer
- Merge pull request #666 from 0xMiden/greenhat/i660-auth-proc
- [**breaking**] version Miden SDK crates separately and set to v0.5.0
- remove function skip list for `wit-bindgen` since we're not doing WIT imports anymore #341
- extract abs path, remove redundant `RUSTFLAGS` `debuginfo` option,
- split and move the rlib stub library compilation and linking from
- move the miden-stdlib-sys stubs into the crate's build.rs
- move the miden-base-sys stubs into the crate's build.rs
- move .rlib stub library compilation closer to rustc flags
- remove all the low-level Miden SDK WIT interface handling
- split .rlib stub library further into submodules
- don't hard-code file names of the .rlib stub library
- extract rlib stub compilation into the separate module

## [0.4.1](https://github.com/0xMiden/compiler/compare/cargo-miden-v0.4.0...cargo-miden-v0.4.1) - 2025-09-03

### Added

- allow `cargo miden build` in a member of a workspace

### Fixed

- disable `debug-assertions` in dev profile
- draft cargo-miden support (disabled for now) for building the crate which is included in the workspace

### Other

- update new project templates tag to v0.14.0
- pass profile options via `--config` flag to `cargo`
- Add 128-bit wide arithmetic support to the compiler.

## [0.4.0](https://github.com/0xMiden/compiler/compare/cargo-miden-v0.1.5...cargo-miden-v0.4.0) - 2025-08-15

### Added

- add basic-wallet-tx-script to the `cargo miden example` command
- add `--tx-script` option to the `cargo miden new` command
- add `project-kind` with `account`, `note-script` and
- add missing and fix existing tx kernel function bindings
- rename note script rollup target into script
- *(cargo-miden)* `example [name]` for paired projects (basic-wallet
- move existing new project templates to `cargo miden example`

### Fixed

- switch to `v0.12.0` tag for new project templates
- new templates path, test `--note` with an account
- remove `add` from the skip list in bindings generator
- improve the detection when to deploy Miden SDK WIT files
- override the binary name for help and error messages to `cargo miden`
- rewrite `ExampleCommand` to fetch from `examples` folder in the

### Other

- rename `note-script` and `tx-script` entrypoints to `run`
- use local compiler path in `ExampleCommand` under tests
- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)
- print detailed error on `cargo-miden` fail
- `ExampleCommand` implementation, add comments
- use `v0.11.0` tag for new project templates
- add comments regarding Cargo.toml processing for example projects
- print the list of the available examples on `cargo miden example --help`
- merge new project and example test building into one test

## [0.1.5](https://github.com/0xMiden/compiler/compare/cargo-miden-v0.1.0...cargo-miden-v0.1.5) - 2025-07-01

### Added

- implement Wasm CM indirect lowering shim+fixup module bypass
- *(cargo-miden)* switch rollup projects to `wasm32-wasip2` target

### Other

- sketch out generic testnet test infrastructure
- add `counter_contract_debug_build` test to reproduce #510,
- remove unused code (componentization) in

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
