# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/0xMiden/compiler/compare/midenc-dialect-hir-v0.4.0...midenc-dialect-hir-v0.4.1) - 2025-09-03

### Fixed

- only keep spills that feed a live reload dominated by this spill

### Other

- remove explicit symbol management
- switch to `expect_file!` in spills tests
- formatting
- add asserts for materialized spills and reloads in
- add tests for the `TransformSpills` pass

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-hir-v0.1.5...midenc-dialect-hir-v0.4.0) - 2025-08-15

### Other

- Add $ParentTrait pattern to verify macro + Add SameTypeOperands as a explicit dependency

## [0.1.5](https://github.com/0xMiden/compiler/compare/midenc-dialect-hir-v0.1.0...midenc-dialect-hir-v0.1.5) - 2025-07-01

### Fixed

- delayed registration of scf dialect causes canonicalizations to be skipped
