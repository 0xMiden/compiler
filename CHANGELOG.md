# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## `cargo-miden` - [0.7.0](https://github.com/0xMiden/compiler/compare/0.6.0...0.7.0) - 2026-02-11

### Added
- improving compiler tracing infrastructure;
- re-export BuildCommand::exec
- gate on-chain `Felt` behind `cfg(miden)`
- introduce onchain/offchain serialization into felts

### Fixed
- cargo miden test no longer hangs
- gate `sdk` Wasm intrinsics behind `cfg(miden)`

### Other
- update new project git tags (with SDK v0.10)
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- remove `cargo miden example` command
- migrate to VM v0.20
- switch contract template repo to v0.25.0 (`#[note]` +` `#[entrypoint]`)
- remove unneeded `cargo test` flags
- move spawn_cargo to tools/cargo-miden/src/utils.rs
- Merge branch 'next' into fabrizioorsi/custom-test-harness
- rename `miden-felt` crate to `miden-field`
- Merge pull request #808 from 0xMiden/greenhat/i698-typed-note-inputs
- restore basic-wallet support in the `cargo miden example`
- remove basic-wallet from `cargo miden example` #662

## `midenc` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-v0.6.0...midenc-v0.7.0) - 2026-02-11

### Added
- improving compiler tracing infrastructure

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-driver` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-driver-v0.6.0...midenc-driver-v0.7.0) - 2026-02-11

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- Update README.md
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-compile` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-compile-v0.6.0...midenc-compile-v0.7.0) - 2026-02-11

### Added
- implement basic mem2reg-style pass for locals
- *(driver)* support ir filters for printing
- add WAT support in `--emit` option

### Fixed
- improve codegen quality of wasm-translated programs
- *(driver)* broken -Z/-C option handling
- upto linker
- no_std build
- write `hir` and `masm` in `--emit` option and preserve Miden

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- change pass order to improve quality of generated code
- *(compile)* move spills transform before scf lifting
- migrate to VM v0.20
- move Masm component and Wat emission to the codegen and frontend
- remove `OutputType::Masl`
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,
- better error messages, code cleanup
- pre-allocate string for WAT printer

## `midenc-frontend-wasm` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-frontend-wasm-v0.6.0...midenc-frontend-wasm-v0.7.0) - 2026-02-11

### Added
- *(sdk)* [**breaking**] assert `value <= Felt::M` in `Felt::from_u64_unchecked` ([#891](https://github.com/0xMiden/compiler/pull/891))

### Fixed
- remove `miden::active_note::add_assets_to_account` because
- SDK bindings for `*_note::get_metadata`
- improve codegen quality of wasm-translated programs
- rename stdlib's `rpo_falcon512` module to `falcon512_rpo`
- updated protocol v0.13 bindings output_note::create(tag, note_type, recipient),
- rename stdlib Blake3 `hash_1to1` to `hash` and `hash_2to1` to `merge`
- rename stdlib SHA256 `hash_1to1` to `hash` and `hash_2to1` to `merge`
- rename stdlb `hashes::rpo` to  `hashes::rpo256`, symbols cleanup
- rename stdlib `hash_memory` and `hash_memory_words` to `hash_elements` and
- `std` -> `miden::core` rename in stdlib
- convert the rest of the `std` to `miden::core` in the bindings
- upto linker
- path (add `protocol`) in the frontend tx kernel bindings

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- remove `intrinsics::mem::heap_base` special treatment for inlining
- remove unused MODULE_ID
- Merge pull request #895 from walnuthq/feature/improve-source-loc-coverage
- Use effective spans on ub.unreachable
- Merge pull request #843 from walnuthq/pr/inline-stubs
- Inline stubs that are ops only
- inline linker stubs at call sites

## `midenc-hir-eval` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-hir-eval-v0.6.0...midenc-hir-eval-v0.7.0) - 2026-02-11

### Other
- Update README.md
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-expect-test` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-expect-test-v0.6.0...midenc-expect-test-v0.7.0) - 2026-02-11

### Other
- Update README.md
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-log` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-log-v0.6.0...midenc-log-v0.7.0) - 2026-02-11

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements

## `midenc-codegen-masm` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.6.0...midenc-codegen-masm-v0.7.0) - 2026-02-11

### Added
- improving compiler tracing infrastructure
- support variadic limbs in `arith.split`/`arith.join`
- rework `split` and `join` to work on all integer types,

### Fixed
- publishing issue with workspace dev-dependencies
- *(codegen)* implement lowering for load/store of pointer values
- skip core modules in `MasmComponent` display
- remove name sanitizing in the codegen, use quoted symbols,
- prepend `::` for missed symbols in emit::int64 module
- sanitize a Wasm CM full path as a function name.
- convert `::` in the function name to the double underscore.
- prefix `miden::core` symbols with `::`
- upto linker
- relax `OperandStack::rename` to allow >16 deep indices
- exhaust tactics before stopping on `fuel`
- generalize `LinearStackWindow` copy materialization
- increase the default fuel amount in the solver to 40
- import `MASM_STACK_WINDOW_FELTS` in `Linear`
- base `Linear` special-case on felt depth
- reject unsupported solutions without skipping fuel
- avoid unsupported stack access in `Linear`
- *(solver)* reject deeper than 16 element window solutions, update linear tactic
- swapping 64-bit limbs for immediate store_dw
- calculate `HEAP_END` according to the comment and put it below `HEAP_INFO_ADDR`
- move `HEAP_INFO_ADDR` to not clash with procedure locals space

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- better error handling, use `LibraryPath::push_component` in
- remove unused debug-mode plumbing
- formatting
- migrate to VM v0.20
- Merge pull request #897 from VolodymyrBg/fix/truncate-stack-drop-instruction
- fix comment
- add targeted trace logs for `LinearStackWindow`
- add `LinearStackWindow` tests
- move linear tactic proptest to solver to allow the fallback to
- move `LinearStackWindow` tests to solver tests module
- extract `LinearStackWindow` tactic as a wrapper around
- add "executable documentation" tests in Linear tactic
- run rustfmt
- clarify `Linear` regression test invariant
- extract `Linear` pre-move helper
- clarify MASM stack window terminology
- use `MASM_STACK_WINDOW_FELTS` constant
- fix the code comments
- Merge pull request #808 from 0xMiden/greenhat/i698-typed-note-inputs
- move Masm component and Wat emission to the codegen and frontend
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-dialect-scf` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-scf-v0.6.0...midenc-dialect-scf-v0.7.0) - 2026-02-11

### Added
- improving compiler tracing infrastructure

### Fixed
- publishing issue with workspace dev-dependencies
- *(scf)* printing of scf.while with no results
- old empty blocks left in scf.while after while-unused-result
- aliasing violations found while trying different pass orderings

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-dialect-ub` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-ub-v0.6.0...midenc-dialect-ub-v0.7.0) - 2026-02-11

### Other
- Update README.md
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-dialect-hir` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-hir-v0.6.0...midenc-dialect-hir-v0.7.0) - 2026-02-11

### Added
- implement basic mem2reg-style pass for locals
- improving compiler tracing infrastructure

### Fixed
- publishing issue with workspace dev-dependencies
- include `scf.while` `after` region uses in spill rewrite #831
- rewrite uses of spilled values in any nested regions of `op`.

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- *(local2reg)* add local2reg tests
- use `litcheck_filecheck::filecheck!` for spill rewrite test
- add `scf.while` after-region spill rewrite repro #831
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-hir-transform` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-hir-transform-v0.6.0...midenc-hir-transform-v0.7.0) - 2026-02-11

### Added
- *(transform)* implement common subexpression elimination pass
- improving compiler tracing infrastructure

### Fixed
- publishing issue with workspace dev-dependencies
- reduce noise in spill pass instrumentation
- aliasing violations found while trying different pass orderings
- *(analysis)* ensure domtree has dfs numbers on creation
- include `scf.while` `after` region uses in spill rewrite #831
- rewrite uses of spilled values in any nested regions of `op`.

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- *(cse)* add initial common subexpression elimination tests
- rename spill-use helpers and skip `IsolatedFromAbove`
- *(spill)* generalize nested-region use rewriting
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-hir-analysis` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-hir-analysis-v0.6.0...midenc-hir-analysis-v0.7.0) - 2026-02-11

### Added
- improving compiler tracing infrastructure

### Fixed
- publishing issue with workspace dev-dependencies
- reduce noise in spill pass instrumentation
- incorrect block visitation order in spills transform
- aliasing violations found while trying different pass orderings
- *(analysis)* aliasing violation in sparse forward dataflow
- remove unused `mut` in spill usage
- avoid unused over-K usage assignments
- handle over-K `scf.yield` in `SpillAnalysis`
- prefer destination placement for structured spills
- deduplicate `SpillAnalysis` spills and reloads
- preserve `ProgramPoint` in `is_spilled_at`/`is_reloaded_at`
- spill the block args if > K(16)

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- *(compile)* move spills transform before scf lifting
- extract over-K spill helper
- remove `SpillAnalysis` `spills_mut`/`reloads_mut`
- harden spill analysis accounting
- update `auth_component_rpo_falcon512` expected files
- index `SpillAnalysis` spill/reload dedup
- remove redundant `is_spilled_at` checks
- cover over-K entry args in `SpillAnalysis`
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-dialect-cf` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-cf-v0.6.0...midenc-dialect-cf-v0.7.0) - 2026-02-11

### Other
- Update README.md
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-dialect-arith` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-arith-v0.6.0...midenc-dialect-arith-v0.7.0) - 2026-02-11

### Added
- support variadic limbs in `arith.split`/`arith.join`
- rework `split` and `join` to work on all integer types,

### Other
- Update README.md
- document `arith` canonicalization module
- add `join2`/`join4` and `split2`/`split4` helpers
- update `auth_component_rpo_falcon512` expected files
- cover `arith.rotl`/`arith.rotr` by 32 canonicalization
- hoist `Symbol::intern` in rotate canonicalization
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-hir` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-hir-v0.6.0...midenc-hir-v0.7.0) - 2026-02-11

### Added
- implement basic mem2reg-style pass for locals
- improve ergonomics of PostPassStatus
- improve operation apis related to side effects
- surface has_ssa_dominance in region and domtree apis
- improving compiler tracing infrastructure
- *(driver)* support ir filters for printing

### Fixed
- improve codegen quality of wasm-translated programs
- uniqued constants left untracked after inserted by op folder
- old empty blocks left in scf.while after while-unused-result
- *(analysis)* ensure domtree has dfs numbers on creation
- remove name sanitizing in the codegen, use quoted symbols,
- upto linker
- use arena allocation for keyed successor keys

### Other
- cache stack_size in ValueOrAlias to avoid repeated virtual dispatch
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- remove duplication in erase_block using block.erase()
- correct documentation for notify_block_inserted
- Merge pull request #911 from sashass1315/fix/arena-zst-extend
- Merge pull request #907 from 0xMiden/i899-migrate-vm-v0.20-storage
- better error handling, use `LibraryPath::push_component` in
- update outdated references to hold *EntityHandle types
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-session` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-session-v0.6.0...midenc-session-v0.7.0) - 2026-02-11

### Added
- add `inter` group to `--emit` option (wat, hir, masm)
- add WAT support in `--emit` option

### Fixed
- treat `--emit=all=path` as a directory
- handle relative paths in `OutputFile::Directory`
- write `hir` and `masm` in `--emit` option and preserve Miden

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- Update README.md
- formatting
- migrate to VM v0.20
- remove `OutputType::Masl`
- make `OutputType:all` and `ir` return `&'static [OutputType]`
- make `OutputTypeSpec::Inter` into `OutputTypeSpec::Subset`
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-hir-symbol` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-hir-symbol-v0.6.0...midenc-hir-symbol-v0.7.0) - 2026-02-11

### Fixed
- link name for `falcon512pro::verify` MASM procedure
- rename stdlib's `rpo_falcon512` module to `falcon512_rpo`
- rename stdlb `hashes::rpo` to  `hashes::rpo256`, symbols cleanup
- `std` -> `miden::core` rename in stdlib
- path (add `protocol`) in the frontend tx kernel bindings

### Other
- Merge pull request #922 from 0xMiden/bitwalker/codegen-fixes-and-improvements
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `midenc-hir-macros` - [0.7.0](https://github.com/0xMiden/compiler/compare/midenc-hir-macros-v0.6.0...midenc-hir-macros-v0.7.0) - 2026-02-11

### Other
- Update README.md
- rename `inter` set to `ir`
- rename `MIDENC_EXPAND` env var to `MIDENC_EMIT_MACRO_EXPAND`,

## `cargo-miden` - [0.6.0](https://github.com/0xMiden/compiler/compare/0.5.1...0.6.0) - 2025-12-23

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

## `midenc` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-v0.5.1...midenc-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-driver` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-driver-v0.5.1...midenc-driver-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-compile` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-compile-v0.5.1...midenc-compile-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- bump toolchain to 1.94/nightly-2025-12-10
- Merge pull request #785 from walnuthq/pr/fix-source-loc-resolution
- Fix source locations when trim-paths is being used

## `midenc-frontend-wasm` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-frontend-wasm-v0.5.1...midenc-frontend-wasm-v0.6.0) - 2025-12-23

### Fixed
- remove sorting the targets in `br_table` Wasm op translation

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- Fix source locations when trim-paths is being used

## `midenc-hir-eval` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-hir-eval-v0.5.1...midenc-hir-eval-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- bump toolchain to 1.94/nightly-2025-12-10

## `midenc-expect-test` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-expect-test-v0.5.1...midenc-expect-test-v0.6.0) - 2025-12-23

### Other
- switch to Rust edition 2024

## `midenc-codegen-masm` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.5.1...midenc-codegen-masm-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Optimise the `realign_dw` memory intrinsic slightly.
- Change the memory representation of 64-bit, dual-limbed values to be little-endian.

## `midenc-dialect-scf` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-scf-v0.5.1...midenc-dialect-scf-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-dialect-ub` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-ub-v0.5.1...midenc-dialect-ub-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-dialect-hir` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-hir-v0.5.1...midenc-dialect-hir-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024

## `midenc-hir-transform` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-hir-transform-v0.5.1...midenc-hir-transform-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-hir-analysis` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-hir-analysis-v0.5.1...midenc-hir-analysis-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024

## `midenc-dialect-cf` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-cf-v0.5.1...midenc-dialect-cf-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024

## `midenc-dialect-arith` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-dialect-arith-v0.5.1...midenc-dialect-arith-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition

## `midenc-hir` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-hir-v0.5.1...midenc-hir-v0.6.0) - 2025-12-23

### Fixed
- DomTreeSuccessorIter::next_back bounds handling

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Fix source locations when trim-paths is being used

## `midenc-session` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-session-v0.5.1...midenc-session-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
- bump toolchain to 1.94/nightly-2025-12-10
- Fix source locations when trim-paths is being used

## `midenc-hir-symbol` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-hir-symbol-v0.5.1...midenc-hir-symbol-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- bump toolchain to 1.94/nightly-2025-12-10

## `midenc-hir-macros` - [0.6.0](https://github.com/0xMiden/compiler/compare/midenc-hir-macros-v0.5.1...midenc-hir-macros-v0.6.0) - 2025-12-23

### Other
- run formatter after upgrade to 2024 edition
- switch to Rust edition 2024
