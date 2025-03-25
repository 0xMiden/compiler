# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.7](https://github.com/0xPolygonMiden/compiler/compare/midenc-frontend-wasm-v0.0.6...midenc-frontend-wasm-v0.0.7) - 2024-09-17

### Other
- *(rustfmt)* disable wrap_comments due to broken behavior

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/midenc-frontend-wasm-v0.0.5...midenc-frontend-wasm-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/midenc-frontend-wasm-v0.0.1...midenc-frontend-wasm-v0.0.2) - 2024-08-30

### Fixed
- *(codegen)* broken return via pointer transformation
- *(frontend-wasm)* do not apply redundant casts
- *(frontend-wasm)* incorrect types applied to certain primops

### Other
- Merge pull request [#284](https://github.com/0xPolygonMiden/compiler/pull/284) from 0xPolygonMiden/bitwalker/abi-transform-test-fixes
- update expect tests due to codegen changes

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/midenc-frontend-wasm-v0.0.0...midenc-frontend-wasm-v0.0.1) - 2024-07-18

### Added
- implement support for wasm typed select
- implement memset, memcpy, mem_grow, mem_size, and bitcast ops
- add workaround for wasm `memory.grow` op translation
- introduce marker attribute for ABI in text representation of
- cut the Miden function digest from the Miden SDK function names
- introduce `import_miden` component import directive to represent
- employ module names in Wasm module imports in Rust bindings for
- add NoTransform strategy for Miden ABI call transformation
- introduce TransformStrategy and add the "return-via-pointer"
- draft the Miden ABI adaptor generation in Wasm frontend
- parse function digest from the Wasm module import
- draft Miden ABI function types encoding and retrieval
- introduce Miden ABI component import
- lay out the Rust Miden SDK structure, the first integration test
- introduce `CanonicalOptions` in IR and translate Wasm
- `Component::modules` are topologically sorted
- introduce module instantiation arguments
- translate Wasm component instance exports;
- Initial Wasm component translation.
- translate Wasm memory.copy op
- Wasm module data segment transtation
- parse Wasm components
- implement compiler driver, update midenc
- translate Wasm `memory.grow` op
- declare data segments in module, remove ptr type casting for `global_set`,
- Wasm data section parsing
- Wasm globals (`global.get`,`set`) translation
- Wasm `i32.wrap_i64` translation
- cast arguments for unsigned Wasm ops, implement signed Wasm ops translation
- Wasm br_table translation
- Wasm `unreachable` op translation
- Wasm `select` op translation
- Wasm `eqz`, `eq`, `ne`, `lt`, `gt`, `le`, `ge` integer and f64 operators
- Wasm integer `lt_u`, `le_u`, `ge_u` and `gt_u` operators translation
- Wasm integer `mul`, `div_u`, `rem_u` and f64 `mul`, `div`, `min`, `max`
- Wasm `f64.add, sub` and integer `sub` translation
- Wasm `shr_u`, `rotl`, `rotr` i32 and i64 instructions translation
- i32 and i64 variants of `shl` and `xor` Wasm ops translation
- Wasm i32.and, i32.or, i64.and, i64.or translation
- add i32.popcnt, i64.extend_i32_s, extend_i32_u Wasm ops translation
- run wasmparser's validator when parsing function bodies
- Wasm memory.grow and memory.size translation
- Wasm memory store/load ops translation
- handle BrTable and CallIndirect usupported Wasm instructions
- add Rust -> Wasm -> Miden IR test pipeline with a simple function call test;
- draft Wasm -> Miden IR translator, handling control flow ops and SSA construction

### Fixed
- improve codegen quality using more precise casts
- properly handle shift operand for bitwise shift/rotate ops
- felt representation mismatch between rust and miden
- use the MASM module paths for the tx kernel module names
- change the `tx_kernel::get_inputs` low-level function signature
- query `ModuleImportInfo::aliases` with module id alias
- strip .wasm extension from parsed wasm binaries
- fix value type in store op in `return_via_pointer` transformation,
- fix build after cherry-pick into temp branch of off main;
- after rebase, add Wasm CM `record` type conversion,
- find the correct core module function for the IR component import
- tweak wasm frontend and related test infra
- swap panics with errors
- parsing Wasm module `memory` section, error handling in `emit_zero`
- improper handling of inlined blocks in inline-blocks transform
- always create the destination branch argument for the sentinel value
- be more explicit about overflow, address some bugs found while testing
- set reduced load/store mem ops ptr type to unsigned,
- cast pointer in memory access ops and br_table selector to U32
- handle missing `Instruction::Switch` in jump destination changes
- cast i64 comparison ops result to i32 to preserve Wasm op semantics
- Cast i1 back to i32/i64 expected by Wasm ops after comparison ops
- cast u32/u64 back to Wasm ops expected i32/i64 after `shr_u`, `div_u`, `rem_u` ops
- skip Wasm element section instead of failing
- set `state.reachable = false` for `Operator::Unreachable`
- make `add`, `mul` and `sub` to use wrapping Miden operations
- handle InvalidFunctionError in ModuleEnviromnment::build
- fix build and tests after rebasing on top of bitwalker/wip branch
- pass SourceSpan in translate_operator

### Other
- fix typos ([#243](https://github.com/0xPolygonMiden/compiler/pull/243))
- extend and update integration tests
- Fix descriptions for crates
- set crates versions to 0.0.0, and `publish = false` for tests
- add missing descriptions to all crates
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- ensure all relevant crates are prefixed with `midenc-`
- Merge pull request [#187](https://github.com/0xPolygonMiden/compiler/pull/187) from 0xPolygonMiden/bitwalker/account-compilation-fixes
- check rustfmt on CI, format code with rustfmt
- run clippy on CI, fix all clippy warnings
- use midenc driver for non-cargo-based fixtures in
- use midenc driver to compile cargo-based fixtures
- handle assembler refactoring changes
- Merge pull request [#170](https://github.com/0xPolygonMiden/compiler/pull/170) from 0xPolygonMiden/greenhat/i159-tx-kernel-func-11apr
- Merge pull request [#155](https://github.com/0xPolygonMiden/compiler/pull/155) from 0xPolygonMiden/greenhat/i144-stdlib
- remove repetitive words
- Merge pull request [#151](https://github.com/0xPolygonMiden/compiler/pull/151) from 0xPolygonMiden/greenhat/i144-native-felt
- Merge pull request [#140](https://github.com/0xPolygonMiden/compiler/pull/140) from 0xPolygonMiden/greenhat/i138-rust-miden-sdk
- remove `dylib` from `crate-type` in Miden SDK crates
- do not inline `miden_sdk_function_type` function
- add `FunctionType::abi` and ditch redundant `*FunctionType`
- remove `miden-abi-conversion` crate and move its code to
- assert Miden ABI function result types after the transformation
- add doc comments to the Miden ABI transformation API
- assert that function call results are the same after transformation
- cache parsed digest and stable import names;
- introduce ModuleTranslationState to hold resolved function
- ditch module and function names for import in favor of
- intern module name and all names used in the module
- remove invocation method from component imports/exports
- clean up todos, add comments
- ensure Wasm module name fallback is set as early as possible
- introduce `ComponentTranslator`
- draft basic wallet translation
- add Wasm component translation support to the integration tests;
- update frontend expect tests with format changes
- add formatter config, format most crates
- update rust toolchain to latest nightly
- Merge pull request [#100](https://github.com/0xPolygonMiden/compiler/pull/100) from 0xPolygonMiden/greenhat/i89-translate-wasm-cm
- move `LiftedFunctionType` to `miden-hir-type` crate
- use `digest` name for MAST root hashes;
- remove `MastRootHash` in favor of `RpoDigest`;
- use FxHashMap instead of HashMap in frontend-wasm;
- remove handling of Wasm 64-bit memory
- remove `indices` macro definition duplication;
- add README for Wasm frontend
- fix a comment, comment out debug prints;
- move `MastRootHash` and `Interface*` types to their modules;
- add missing doc comments
- remove `BuildIrComponentInput`;
- handle errors on Wasm component translation;
- rename `mod_info` parameter to `module` in Wasm module
- introduce `Module::global_name`, and make `Module::name_section` private;
- code clean up;
- rename `WasmTranslationConfig::module_name_fallback` to `sourse_name`;
- set up mdbook deploy
- add guides for compiling rust->masm
- extract Wasm component section parsing into separate methods
- convert various `Translator` methods to functions
- extract Wasm core module sections parsing to a separate methods
- remove unused variables
- update wasmparser to v0.118.1
- enable `rust_array` frontend test after block inline pass fixed
- lazy IR compilation in integration tests
- move module specific code to separate module in frontend-wasm
- remove unused dependencies
- Merge pull request [#61](https://github.com/0xPolygonMiden/compiler/pull/61) from 0xPolygonMiden/greenhat/cargo-ext-i60
- make `WasmTranslationConfig::module_name_fallback` non-optional
- switch from emiting MASM in CodegenStage, and switch to output folder in cargo extension
- remove `miden_frontend_wasm::translate_program`
- add integration tests for comparison instructions
- fix build after rebase
- implement `gt_u` instruction semantic test, add `translate_program` to Wasm frontend
- compile rust app (cargo project) to masm, run both and compare results;
- temporarily disable dlmalloc test
- finalize pass refactoring, implement driver
- update tests broken due to formatter changes
- initial parser implementation
- demangle function names in Rust compilation tests
- update expected IR for dlmalloc (switch to *_imm ops)
- add a debug assert to not allow declaring a block predecessor twice
- more readable `mem_total_pages` implementation
- use `*_imm` op variant where possible
- code cleanup
- remove `ValueData::Alias`
- update expected IR for dlmalloc test
- use `ModuleFunctionBuilder` instead of `FunctionBuilder` in `FunctionBuilderExt`
- ignore dlmalloc test because hash part in mangled function names is not stable enough
- fix build after rebase
- use `panic_immediate_abort` std feature to avoid core::fmt (uses `call_indirect`)
- Rust code with `div`, `rem`, `shr` signed and unsigned ops
- Rust code using dlmalloc in no_std
- remove float point ops translation;
- make `translation_utils::block_with_params` a `FunctionBuilderExt` method
- move `module_env` to the root of the crate, remove `environ` module
- move module_translator tests to code_translator test,
- `check_ir_files` variant with expected wat and mir files;
- print types on type mismatch error when defining a variable;
- avoid unnecessary allocation by making `DataSegment::data` a reference
- remove caching for Rust -> Wasm compilation artifacts in tests
- skip `@producers` Wasm section when printing wasm in Rust compilation tests;
- remove `FuncEnvironment` and use `ModuleInfo` directly
- fix build after rebase
- fix the test for unsupported ops
- add BrTable(commented) to unsupported instructions test
- remove todo! for Wasm `select` and `unreachable`
- import `Type::*` and remove repetitive `Type::`;
- add test for unsupported Wasm spec v1 instructions
- re-word unsupported Wasm feature error message;
- silence diagnostic output in tests
- add per-instruction test for every implemented Wasm instruction
- move all unsupported Wasm ops to catch-all case
- cleanup rust compilation test;
- make a temp dir for Rust->Wasm compilation tests artifacts
- add Rust->Wasm->IR Fibonacci example
- update expected module name in tests
- better var names, cleanup comments
- provide some initial usage instructions
- Initial commit
