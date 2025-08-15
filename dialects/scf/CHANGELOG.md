# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-scf-v0.1.5...midenc-dialect-scf-v0.4.0) - 2025-08-15

### Fixed

- cast block 345 using `zext` instead of `set_type`
- change v345's type to U32 so that all the variables are of the same type

### Other

- Use the midenc_hir `test` dialect for ops in the new tests.
- Use `Operation::is()` rather than an op name compare.
- Address PR feedback.
- Fix spelling in comment.
- Add a rewriter pass for folding redundant yields from SCF control flow.

## [0.1.5](https://github.com/0xMiden/compiler/compare/midenc-dialect-scf-v0.1.0...midenc-dialect-scf-v0.1.5) - 2025-07-01

### Fixed

- *(scf)* over-eager trivial if-to-select rewrite
- delayed registration of scf dialect causes canonicalizations to be skipped

### Other

- remove `expect-test` in favor of `midenc-expect-test`
