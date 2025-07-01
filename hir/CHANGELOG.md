# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5](https://github.com/0xMiden/compiler/compare/midenc-hir-v0.1.0...midenc-hir-v0.1.5) - 2025-07-01

### Fixed

- prevent global variables from overlapping with data segments

## [0.0.8](https://github.com/0xMiden/compiler/compare/midenc-hir-v0.0.7...midenc-hir-v0.0.8) - 2025-04-24

### Added
- *(types)* clean up hir-type for use outside the compiler
- *(ir)* provide some useful symbol path helpers
- *(ir)* allow converting exclusive immutable borrow into a mutable one
- *(ir)* provide utility pass for printing ir of specific operations
- *(frontend)* generate `CanonLower` synthetic functions for
- *(frontend)* generate `CanonLift` synthetic functions for exports
- add `CallConv::CrossCtx` calling convention for cross-context
- draft Wasm CM function type flattening, checks for scalar type
- on Miden CCABI lifting/lowering get import/export function signature from
- store the imported function type in the component import
- rewrite the function calls from the imported function (Miden CCABI) to
- draft lifting function generation in `LiftImportsCrossCtxStage`
- draft `LiftImportsCrossCtxStage` scaffolding
- add `ComponentBuilder::load`, `with_exports` to modify
- implement cross-context Call op, rename IR Exec back to Call
- restore module and function names of the intrinsics and Miden

### Fixed
- *(ir)* missing result value for builtin.global_symbol when printed
- *(ir)* resolve symbol paths without leaf
- *(ir)* custom printer for global_symbol, add missing traits
- *(ir)* missed closing paren in function decl signatures
- *(ir)* would_be_trivially_dead_even_if_terminator
- build after rebase
- implement `FunctionBuilderExt::change_branch_destination`
- a few rebasing issues
- populate `Linker::allow_missing` with linked library exports
- refine `Component` imports and exports to reference module imports

### Other
- treat warnings as compiler errors,
- update Miden VM to v0.13.2 and uncomment the Miden package
- *(frontend)* rework handling of symbols in frontend
- update pre-interned symbols
- implement MemoryEffectOpInterface for test.constant
- specify log target during region simplification, pattern rewrites
- update rust toolchain, clean up deps
- move `Context::change_branch_destination` to be a method in `BranchOpInterface`
- use `OpBuilder` in `ComponentBuilder::define_function`
- rename hir2 crates
- remove old contents of hir, hir-analysis, hir-transform
- update the Miden VM with updated `miden-package` crate
- update rust toolchain to 1-16 nightly @ 1.86.0
- normalize use of fxhash-based hash maps
- replace `CallConv::CrossCtx` with `CanonLift` and `CanonLower`
- rename `CanonAbiImport::interface_function_ty`
- switch from passing `Module` to `Component` in the compiler stages
- rename Call IR op to Exec
- add note script compilation test;
- remove digest-in-function-name encoding and `MidenAbiImport::digest`,

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-v0.0.6...midenc-hir-v0.0.7) - 2024-09-17

### Other
- fix up new clippy warnings

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-hir-v0.0.5...midenc-hir-v0.0.6) - 2024-09-06

### Other
- clean up unused deps
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-v0.0.1...midenc-hir-v0.0.2) - 2024-08-28

### Added
- implement packaging prototype

### Fixed
- *(frontend-wasm)* reserve memory allocated for use by rust
- inoperative --print-ir-after-*, add --print-cfg-after-*
- regression in midenc compile
- use less fragile method for rodata segment init

### Other
- remove miden-diagnostics, start making midenc-session no-std-compatible

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-hir-v0.0.0...midenc-hir-v0.0.1) - 2024-07-18

### Added
- implement memset, memcpy, mem_grow, mem_size, and bitcast ops
- support linking against stdlib in codegen tests
- add U32 comparison ops with immediates variants along with
- add cfg printer using mermaid.js syntax
- introduce marker attribute for ABI in text representation of
- introduce `import_miden` component import directive to represent
- introduce TransformStrategy and add the "return-via-pointer"
- draft the Miden ABI adaptor generation in Wasm frontend
- use semicolon as closing delimiter for for the component import
- draft Miden ABI function types encoding and retrieval
- introduce Miden ABI component import
- introduce `CanonicalOptions` in IR and translate Wasm
- `Component::modules` are topologically sorted
- introduce module instantiation arguments
- implement parser for sexpr-based hir format
- implement new sexpr-based format for hir
- add generic pretty printer for use in compiler
- Initial Wasm component translation.
- implement new instruction scheduler/emitter
- implement small{set|ordset|map} data structures
- add attributes to the ir
- support loc_store(w) instructions
- add support for loading masm programs from disk
- implement codegen for i32 ops
- implement compiler driver, update midenc
- implement translation of parser ast to hir
- add `Function::builder` method
- translate Wasm `memory.grow` op
- Wasm globals (`global.get`,`set`) translation
- implement conversion of masm ir to 'real' masm
- add diagnostic! convenience macro
- teach immediate to become a type
- add display impl for callconv
- impl display trait for function
- introduce program builder
- improve ergonomics of a few key hir types
- add assert_eq_imm builder
- masm imports
- rewrite type layout functionality
- implement lowering of global values
- add push2 meta instruction to masm isa
- improve ergonomics of immediate values
- implement stackification analyses
- export inline asm block/op formatting primitives
- refactor type layout primitives
- support converting byte arrays to constant data
- support declaring data segments via module builder
- add getelementptr primitive
- add immediate variants of comparison operators
- add builders for globalvalues representing symbols
- implement program linker
- implement formatting of data segments, globals
- implement support for wasm data segments
- add assert_matches macro
- support parsing function idents from strings
- provide type representation enum
- implement inline assembly
- implement local variables
- add dataflow apis for getting value/inst spans
- distinguish bitwise vs logical boolean operations
- implement select instruction

### Fixed
- properly handle shift operand for bitwise shift/rotate ops
- properly handle fully-qualified procedure names
- felt representation mismatch between rust and miden
- query `ModuleImportInfo::aliases` with module id alias
- *(analysis)* ensure analyses are invalidated by rewrites unless explicitly preserved
- improper rendering of identifiers in masm text output
- fix build after cherry-pick into temp branch of off main;
- *(warnings)* a few useful functions are now detected unused
- handle stabilization of trait_upcasting in 1.76
- align hir and masm operand order
- improve masm text output
- improper handling of inlined blocks in inline-blocks transform
- clean up display format for masm
- ensure intrinsic modules are linked to program
- be more explicit about overflow, address some bugs found while testing
- fix build after rebase
- set reduced load/store mem ops ptr type to unsigned,
- handle missing `Instruction::Switch` in jump destination changes
- fix build and tests after rebasing on top of bitwalker/wip branch
- bug in construction of sum_matrix test function
- bug in treeify pass due to shallow instruction cloning
- items in arenamap must remain addressable even when removed
- ret instruction was incorrectly given a result type
- make store/memcpy primops
- make dfg.inst return instruction, and dfg.inst_node the node
- use smallvec for masm block contents
- improve ergonomics of import_function on module function builder
- make select builder return the selected value
- incorrect controlling type for load instruction
- make symbol_addr(_relative) take a type
- address discrepancies between types and immediates
- address a couple clippy warnings
- validate repeat loops in inline assembly
- reimplement builders for inline asm control flow
- validate exec/syscall in inline asm
- missing stack adjustments in multiple masm op builders
- improve handling of multi-value results in inline asm
- hir and hir-analysis tests

### Other
- Merge pull request [#237](https://github.com/0xPolygonMiden/compiler/pull/237) from 0xPolygonMiden/greenhat/emu-print-stack-option
- use `BTreeSet` for "dirty" memory addresses(ordered) in the emulator
- add `Emulator::print_trace` option to print the current stack
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- update the Miden VM deps to the `ddf536c` commit with `if.true` empty
- tx_kernel::get_inputs MASM compilation test
- ensure all relevant crates are prefixed with `midenc-`
- run clippy on CI, fix all clippy warnings
- use midenc driver for non-cargo-based fixtures in
- use midenc driver to compile cargo-based fixtures
- update expect tests due to formatting changes in assembler
- handle assembler refactoring changes
- remove formatter
- draft a layout for intrinsics semantic tests,
- since all the Miden ABI transformation happens in the frontend
- Merge pull request [#140](https://github.com/0xPolygonMiden/compiler/pull/140) from 0xPolygonMiden/greenhat/i138-rust-miden-sdk
- add `FunctionType::abi` and ditch redundant `*FunctionType`
- skip printing the import name for `MidenAbiImport` component import
- intern module name and all names used in the module
- remove invocation method from component imports/exports
- add expected imports/exports check in basic wallet test
- draft basic wallet translation
- add Wasm component translation support to the integration tests;
- add formatter config, format most crates
- update rust toolchain to latest nightly
- Merge pull request [#100](https://github.com/0xPolygonMiden/compiler/pull/100) from 0xPolygonMiden/greenhat/i89-translate-wasm-cm
- move `LiftedFunctionType` to `miden-hir-type` crate
- use `digest` name for MAST root hashes;
- remove `MastRootHash` in favor of `RpoDigest`;
- move `MastRootHash` and `Interface*` types to their modules;
- add missing doc comments
- Merge pull request [#99](https://github.com/0xPolygonMiden/compiler/pull/99) from 0xPolygonMiden/bitwalker/book
- set up mdbook deploy
- add guides for compiling rust->masm
- remove unused dependencies
- clarify distinction between smallordset and smallset
- add test for issue 56
- add mdbook skeleton
- update miden-assembly to next
- clear up clippy warnings
- *(emulator)* prepare emulator for use in interactive debugger
- finalize pass refactoring, implement driver
- rework pass infrastructure for integration with driver
- update tests broken due to formatter changes
- clean up parser code and prepare for merge
- initial parser implementation
- remove `ValueData::Alias`
- use `ModuleFunctionBuilder` instead of `FunctionBuilder` in `FunctionBuilderExt`
- fix build after rebase
- add per-instruction test for every implemented Wasm instruction
- use workspace deps rather than paths for hir crates
- add support for real spans in tests
- add block id to block data
- clean up emulator dispatch loop code
- clean up clippy warnings
- add analysis tests
- clean up hir tests
- fix clippy warnings/errors
- add tests for inline-asm stack, fix bugs
- note tracking issue for link-time gc
- implement multi-module linker test
- rework data segment struct
- rework the global variable table api
- improve handling of stack effects in inline assembly
- implement integration test for inline-asm builders
- improve docs related to inline asm
- split up asm module
- rework the ir to better suit wasm->masm
- split up hir crate
- move ir to hir
- provide some initial usage instructions
- Initial commit
