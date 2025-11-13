# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.5.0...midenc-codegen-masm-v0.5.1) - 2025-11-13

### Other

- migrate to 0.19
- update vm to 0.18

## [0.5.0](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.4.1...midenc-codegen-masm-v0.5.0) - 2025-11-05

### Fixed

- support --test-harness with no/multiple initializers
- incorrect lowering of >= 128-bit bnot ([#725](https://github.com/0xMiden/compiler/pull/725))
- move data segments merging to the codegen, preserving `readonly` attribute in IR #634

### Other

- Merge pull request #701 from 0xMiden/chore/docusaurus-migration-new
- Fix codegen for `cf.select` of 64bit values.
- Merge pull request #713 from radik878/chore/remove-petgraph-midenc-codegen-masm-447
- *(deps)* update to no-std compatible petgraph
- Revert "Merge pull request #692 from 0xMiden/chore/docusaurus-migration-next"
- Merge pull request #692 from 0xMiden/chore/docusaurus-migration-next
- *(README)* add docs section explainer
- Merge pull request #678 from 0xMiden/greenhat/i534-auth-rpo-falcon
- Merge pull request #666 from 0xMiden/greenhat/i660-auth-proc
- Merge branch 'next' into fabrizioorsi/update-vm-0.17

## [0.4.1](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.4.0...midenc-codegen-masm-v0.4.1) - 2025-09-03

### Fixed

- reverse only non-intrinsic modules when assembling a Program
- use `Assembler::add_module` for intrinsics when assembling a `Library`
- remove intrinsics exports from compiled MASM library #637

### Other

- Add 128-bit wide arithmetic support to the compiler.
- Fix bug with argument scheduler for binary ops.

## [0.4.0](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.1.5...midenc-codegen-masm-v0.4.0) - 2025-08-15

### Added

- implement advice map API in Miden SDK
- add `crypto::hmerge()` in Miden SDK (`hmerge` VM intruction);

### Fixed

- add an extra memory page after the data segments to accommodate for
- start `Linker::reserved_memory_pages` on the next page after rustc
- *(codegen)* missing result renaming in ptrtoint lowering
- two-operand optimized stack scheduling when only one value is on the stack #606
- fix the manual two-operand ops stack scheduling, remove `BinaryOp` gate
- do binary stack scheduling optimizations only for the binary ops (implementing
- `Rodata::bytes_to_elements()` check if `into_remainder()` returns

### Other

- rename `io` to `advice`, export modules in stdlib SDK
- Add the test that executes counter contract, basic wallet and p2id note script on the local node ([#555](https://github.com/0xMiden/compiler/pull/555))
- make `DEFAULT_RESERVATION` include the extra page to
- add comments with stack state in `heap_init`
- update Rust toolchain nightly-2025-07-20 (1.90.0-nightly)
- clean up
- simplify `test_arg_order` test and move it to `misc` module
- `hmerge` intrinsic to accept digests as a pointer and load
- make `bytes_to_elements` infallible

## [0.1.5](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.1.0...midenc-codegen-masm-v0.1.5) - 2025-07-01

### Added

- small integers in `struct` support in result in cross-context calls

### Fixed

- add immediate pointer to `OpEmitter::load_small`
- call `truncate_stack` before returning from a `call`-able procedure
- mask calculation in `trunc_int32` function (128 -> 255 for u8);

### Other

- Merge pull request #572 from 0xMiden/greenhat/i560-init-account-note
- add `store_small_imm` function doc and assert
- don't delegate in `store_small_imm` to `store_word_imm`,
- add comments
- remove redundant `store` tests
- reduce stack manipulation in `store_small`
- assert that type to be loaded and the pointee type have the
- use `NativePtr::is_word_aligned`
- remove `expect-test` in favor of `midenc-expect-test`

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-codegen-masm-v0.0.7...midenc-codegen-masm-v0.0.8) - 2025-04-24

### Added
- *(types)* clean up hir-type for use outside the compiler
- *(codegen)* migrate to element-addressable vm
- implement cross-context Call op, rename IR Exec back to Call

### Fixed
- *(codegen)* incorrect conversion of u64 literal to miden repr
- *(codegen)* incomplete global/data segment lowering
- *(codegen)* lowering of builtin.global_symbol
- *(codegen)* bitcasts should allow pointer-to-pointer casts
- [#406](https://github.com/0xMiden/compiler/pull/406) order the program modules with intrinsics being the first when passed
- relax the module name validation in ConvertHirToMasm to support the Wasm CM naming
- skip the assembler registration for the already registered module
- recover Wasm CM interfaces as module names in exports after
- skip registering already registered modules in Assembler
- fallback to `LibraryPath` unchecked ctor (Wasm CM styles names)
- sort module imports and functions in PrettyPrint impl to stabilize

### Other
- treat warnings as compiler errors,
- update expected masm code
- update Miden VM to v0.13.2 and uncomment the Miden package
- *(codegen)* improve tracing/debug output in a few places
- update rust toolchain, clean up deps
- rename hir2 crates
- remove old contents of hir, hir-analysis, hir-transform
- switch from recognizing intrinsics module by name(substring)
- update to the latest `miden-mast-package` (renamed from
- update the Miden VM with updated `miden-package` crate
- update rust toolchain to 1-16 nightly @ 1.86.0
- normalize use of fxhash-based hash maps
- rename Call IR op to Exec
- switch to `Package` without rodata,
- [**breaking**] move `Package` to `miden-package` in the VM repo

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-codegen-masm-v0.0.6...midenc-codegen-masm-v0.0.7) - 2024-09-17

### Other
- fix up new clippy warnings
- update rust toolchain

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-codegen-masm-v0.0.5...midenc-codegen-masm-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.3](https://github.com/0xPolygonMiden/compiler/compare/midenc-codegen-masm-v0.0.2...midenc-codegen-masm-v0.0.3) - 2024-08-30

### Fixed
- *(codegen)* broken return via pointer transformation
- swap the lo and mid parts in the most shifted case in `load_dw`
- *(codegen)* incorrect order of elements for word-oriented loads/stores
- *(codegen)* make sure we always drop unused instruction results
- *(codegen)* incorrect lowering of global.{load,iadd,symbol}
- *(codegen)* incorrect handling of multi-result instructions
- *(codegen)* ensure global initializers are set during rodata init
- *(codegen)* ensure callee results are pushed on stack in correct order

### Other
- fix clippy warnings in tests
- Merge pull request [#290](https://github.com/0xPolygonMiden/compiler/pull/290) from 0xPolygonMiden/greenhat/i263-mem-intrinsics-felts-tests
- Merge pull request [#284](https://github.com/0xPolygonMiden/compiler/pull/284) from 0xPolygonMiden/bitwalker/abi-transform-test-fixes
- *(codegen)* clippy suggested some improvements
- *(codegen)* be consistent about the way in which we push to stack

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-codegen-masm-v0.0.1...midenc-codegen-masm-v0.0.2) - 2024-08-28

### Added
- implement packaging prototype

### Fixed
- *(frontend-wasm)* reserve memory allocated for use by rust
- regression in midenc compile
- use less fragile method for rodata segment init

### Other
- Merge pull request [#269](https://github.com/0xPolygonMiden/compiler/pull/269) from 0xPolygonMiden/greenhat/i267-store-load-dw

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-codegen-masm-v0.0.0...midenc-codegen-masm-v0.0.1) - 2024-07-18

### Added
- *(codegen)* implement lowering for local var ops
- *(ir)* add support for declaring function-local variables
- *(ir)* add support for 128-bit immediates
- implement most i64 ops and intrinsics, fix some 64-bit bugs
- implement memset, memcpy, mem_grow, mem_size, and bitcast ops
- implement small stores
- support linking against stdlib in codegen tests
- add workaround for wasm `memory.grow` op translation
- implement ilog2/clz/ctz/clo/cto instructions
- implement new sexpr-based format for hir
- implement new instruction scheduler/emitter
- add attributes to the ir
- support loc_store(w) instructions
- add support for loading masm programs from disk
- implement codegen for i32 ops
- implement compiler driver, update midenc
- implement translation of parser ast to hir
- implement conversion of masm ir to 'real' masm
- prettify assertions on textual ir
- improve debugging facilities of the test emulator
- add debugging utilities to test emulator harness
- implement masm ir emulator for testing
- add nativeptr type to masm ir
- masm imports
- implement lowering of inline assembly
- implement lowering of global values
- impelement most of the stackification pass
- extend masm ir for use in codegen
- improve ergonomics of immediate values
- implement stackification analyses

### Fixed
- centralize management of compiler rewrite pipeline
- issue with i1 widening casts
- improve error reporting when parsing masm fails
- properly handle fully-qualified procedure names
- *(codegen)* various issues with lowering of large integral types
- prepend emitted `intrinsics::*` calls with `::` as absolute
- operand solver improperly tracking aliases
- improper rendering of identifiers in masm text output
- address some missing functionality needed by test suite
- *(warnings)* a few useful functions are now detected unused
- use zero-based indices for all instruction positions in depgraph
- promote field to explicit parameter in block scheduler
- remove unused field from scheduler
- use more descriptive name for block argument count
- extract control dependency assignment into separate function
- move add_data_dependency to dependency graph
- improve masm text output
- binary emitter
- properly handle emitting final artifacts in midenc-compile
- clean up display format for masm
- improper generation of procedure name/id when lowering entrypoint
- fix build after rebase
- *(emulator)* incorrect handling of loopback edges in control stack
- broken intrinsics::i32::checked_div
- broken intrinsics::i32::overflowing_mul
- broken intrinsics::i32::overflowing_sub
- i32::overflowing_add
- ensure MasmCompiler runs basic rewrites on programs
- incorrect masm syntax in intrinsics::mem
- ensure intrinsic modules are linked to program
- unreachable raises assertion during stackification
- be more explicit about overflow, address some bugs found while testing
- a variety of operand stack bugs
- bug in depgraph node index oracle
- don't emit redundant u32.assert for known u32 values
- mishandling of mem_loadw/mem_storew semantics
- stackify bug in loop exit edge handling
- a few load related bugs
- *(stackify)* update pass, fix various bugs uncovered in testing

### Other
- Merge pull request [#238](https://github.com/0xPolygonMiden/compiler/pull/238) from 0xPolygonMiden/greenhat/i230-get-inputs-clk451-assert
- Merge pull request [#237](https://github.com/0xPolygonMiden/compiler/pull/237) from 0xPolygonMiden/greenhat/emu-print-stack-option
- Merge pull request [#244](https://github.com/0xPolygonMiden/compiler/pull/244) from 0xPolygonMiden/bitwalker/load-store-reordering
- Merge pull request [#241](https://github.com/0xPolygonMiden/compiler/pull/241) from 0xPolygonMiden/bitwalker/operand-stack-overflow
- clean up docs and implementation of spills rewrite
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- restore running MASM on the emulator along the VM in integration tests
- update VM to the commit in next branch after the merge
- workaround for calling the absolute path functions in the emulator
- Fix descriptions for crates
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- Merge pull request [#203](https://github.com/0xPolygonMiden/compiler/pull/203) from 0xPolygonMiden/greenhat/get-inputs-compile-succ
- add duplicated stack operands test for the stack operand
- ensure all relevant crates are prefixed with `midenc-`
- fix clippy warning
- draft abi transform test for stdlib blake3 hash function
- Merge pull request [#182](https://github.com/0xPolygonMiden/compiler/pull/182) from 0xPolygonMiden/bitwalker/emit-stores
- Merge pull request [#187](https://github.com/0xPolygonMiden/compiler/pull/187) from 0xPolygonMiden/bitwalker/account-compilation-fixes
- check rustfmt on CI, format code with rustfmt
- run clippy on CI, fix all clippy warnings
- Merge pull request [#179](https://github.com/0xPolygonMiden/compiler/pull/179) from 0xPolygonMiden/greenhat/inttoptr-for-gv-store
- add explanatory text regarding choice of data structure in codegen entities
- update expect tests due to formatting changes in assembler
- handle assembler refactoring changes
- remove repetitive words
- add formatter config, format most crates
- update rust toolchain to latest nightly
- a few minor improvements
- *(docs)* fix typos
- Merge pull request [#99](https://github.com/0xPolygonMiden/compiler/pull/99) from 0xPolygonMiden/bitwalker/book
- set up mdbook deploy
- add guides for compiling rust->masm
- remove unused dependencies
- rename Value to ValueOrAlias in miden-codegen-masm
- improve errors produced by various assertions, remove/tweak others
- grammar and other assorted tweaks/improvements
- extract codegen analyses, tweak module structure
- add mdbook skeleton
- update miden-assembly to next
- clear up clippy warnings
- fix build after rebase
- run MASM on VM vs native Rust under `proptest` in integration tests
- compile rust app (cargo project) to masm, run both and compare results;
- add tests for i32 checked_shr intrinsic
- add tests for i32 pow2/ipow intrinsics
- introduce proptest for use in codegen tests
- i32 (un)checked_neg
- add tests for, and address bugs with i32 icmp
- *(emulator)* prepare emulator for use in interactive debugger
- finalize pass refactoring, implement driver
- rework pass infrastructure for integration with driver
- remove `ValueData::Alias`
- fix build after rebase
- use workspace deps rather than paths for hir crates
- add support for real spans in tests
- clean up emulator dispatch loop code
- clean up clippy warnings
- implement codegen frontend and tests
- *(stackify)* implement lowering of instructions, with helpers
- rewrite operand stack impl for codegen
- update masm ir program/module/function items
- provide some initial usage instructions
- Initial commit
