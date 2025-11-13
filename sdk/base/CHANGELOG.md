# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/0xMiden/compiler/compare/miden-base-v0.7.0...miden-base-v0.7.1) - 2025-11-13

### Other

- updated the following local packages: miden-base-macros, miden-stdlib-sys, miden-base-sys

## [0.4.0](https://github.com/0xMiden/compiler/compare/miden-base-v0.1.5...miden-base-v0.4.0) - 2025-08-15

### Added

- add `project-kind` with `account`, `note-script` and
- add missing and fix existing tx kernel function bindings
- rename note script rollup target into script

### Fixed

- add `arg: Word` parameter to `script`
- update Miden SDK `AccountId` type and `account::get_id()` for two

### Other

- rename `note-script` and `tx-script` entrypoints to `run`
- formatting
- make `miden_stdlib_sys::Digest` a newtype instead of type alias

## [0.1.0](https://github.com/0xMiden/compiler/releases/tag/miden-base-v0.1.0) - 2025-05-23

### Added

- switch to stable vm, link against real miden-lib
- bundle Miden SDK WIT files with relevant SDK crates
- *(sdk)* introduce `miden-base` with high-level account storage API

### Other

- 0.1.0
- rename `CoreAsset` to `Asset` in Miden SDK #501
- update url
- fixup miden-base facade in sdk
- rename `StorageMapAccess::read` and `write` to `get` and `set`
- make account storage API polymorphic for key and value types
- fix typos ([#243](https://github.com/0xMiden/compiler/pull/243))
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit
