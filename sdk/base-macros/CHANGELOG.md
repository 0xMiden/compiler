# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/0xMiden/compiler/compare/miden-base-macros-v0.7.0...miden-base-macros-v0.7.1) - 2025-11-13

### Other

- migrate to 0.19

## [0.4.0](https://github.com/0xMiden/compiler/compare/miden-base-macros-v0.1.5...miden-base-macros-v0.4.0) - 2025-08-15

### Fixed

- add empty `struct` support in `#[component]` attribute macro

### Other

- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)

## [0.1.0](https://github.com/0xMiden/compiler/releases/tag/miden-base-macros-v0.1.0) - 2025-05-23

### Added

- add `package.metadata.miden.supported-types`
- *(sdk)* `component` attribute macros parses `name`, `description`
- *(sdk)* store type attribute name, update `miden-objects` to
- *(frontend)* parse AccountComponentMetadata in the frontend
- *(frontend)* store `AccountComponentMetadata` in custom link section
- *(sdk)* implement an account instantiation in `component` macro
- *(sdk)* introduce `miden-base` with high-level account storage API

### Fixed

- handle empty call site span for `component` macro under

### Other

- update dependencies
- 0.1.0
- update url
- fixup miden-base facade in sdk
- formatting and cleanup
- remove the component_macro_test since macro under test
- split `component` attribute macro
- make account storage API polymorphic for key and value types
- add expected TOML test for `component` macro and compiled Miden package
- make `component` macro implement `Default` for the type
- fix clippy warnings
- add macros generated AccountComponentMetadata test
- parse description and implement AccountComponentMetadataBuilder,
- fix typos ([#243](https://github.com/0xMiden/compiler/pull/243))
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit
