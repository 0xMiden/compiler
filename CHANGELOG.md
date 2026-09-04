# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Migration and breaking changes

- BREAKING: `#[export_type]` now rejects conflicting registrations for the same WIT type and
  reserves the inherent associated constant name `__MIDEN_EXPORT_TYPE_SHAPE`. See the
  [migration guide](./sdk/sdk/MIGRATION.md) for both required source changes.

## [0.10.0]

### Compiler and `midenc`

- Added Wasm `call_indirect` support for locally defined, statically initialized function tables,
  enabling Rust function pointers and trait objects. Dispatch checks table bounds, null entries,
  and callee signatures; calls may pass up to 15 field elements of arguments.
- Typed foreign procedure calls now support payload-carrying variants, including `Option` and
  `Result`. Invalid core signatures and unsupported payload layouts produce diagnostics.
- Note packages can export constructors alongside their script entrypoint, and constructors can
  obtain the note script's MAST root in the VM to build a recipient committing to that script.
- Added support for the component initialization emitted by `nightly-2026-09-01`. The component
  start runs after memory, globals, and function tables are initialized in each fresh context.
- Manifest-backed Rust builds accept `--stop-after=dependencies` to resolve and assemble dependency
  packages without compiling the consuming crate. Set `MIDENC_PACKAGE_CACHE` to retain the staged
  packages after the command exits. Dependency consumers receive the compiler's selected artifacts,
  including workspace, path, git, and registry dependencies.
- Compilation now supports HIR modules nested inside other modules, including their globals and
  data segments. Invalid memory layouts and unsupported indirect-call targets report errors instead
  of panicking.
- HIR parsing accepts empty symbol tables and reports duplicate symbols and unregistered dialects
  without aborting. Unknown dialects are distinguished from unknown operations in a registered
  dialect.
- Artifact writes are atomic and create missing destination directories, so a failed write no
  longer leaves a truncated replacement for a previously complete file.
- The `local2reg` and `sink-operand-defs` passes are now available in registered pass pipelines,
  including `hir-opt`.

### `cargo-miden`

- Added `cargo miden new --force-download` to require a downloaded and verified released template
  bundle, failing if one cannot be obtained.
- Generated contracts include build-script support for resolving Miden dependencies during plain
  Cargo builds and IDE analysis.

### `miden-objtool`

- `dump debug-info` displays resolved local and global frame-base expressions as readable locations.

### Libraries and public APIs

- Added MASM `disassemble_*_for_lint` APIs that return the procedures they could lift together with
  the paths, spans, and reasons for skipped procedures. Bounded advice-taint analysis can return
  partial results with an explanation when its work limit is reached.
- Added `Operation::reachability` and `reachability_cached` for classifying whether execution can
  reach another operation across blocks, loops, and nested regions.
- HIR builders can define function tables, perform same-context indirect calls with
  `hir.exec_indirect`, and obtain procedure digests with `hir.procedure_root`.
- `cargo_miden::bundle` now exposes released-or-embedded template resolution and the selected
  bundle's source and version.
- `LinkLibrary::precompiles()` provides the separate precompiles package required by the core
  library.

### Migration and breaking changes

- The compiler now targets Miden VM `0.29` and protocol `0.16.0-rc.4`. Update matching host and
  library dependencies and rebuild `.masp` packages; core-library and transaction-kernel changes
  affect package commitments. Building the compiler and Rust projects now uses
  `nightly-2026-09-01` instead of `nightly-2026-04-30`, and compiler crates declare Rust `1.99`
  instead of `1.97`. Apply the related SDK changes in its [migration guide](sdk/sdk/MIGRATION.md).
- Running `midenc` with no input now compiles `miden-project.toml` in the working directory, fixing
  the behavior advertised in the release candidate. Use `midenc --help` to request usage text.
- Default `midenc` outputs now go to `<target-dir>/<profile>` (normally `target/miden/<profile>`)
  instead of the working directory. Use `--output-dir .` to retain the previous location.
  `cargo miden build` now honors the selected output destinations instead of always writing a
  package in the default target directory. `--output-dir`, `--output-file`, and `--stdout` are
  mutually exclusive; remove conflicting combinations.
- `--emit=mast` now emits readable MAST text instead of binary package bytes. Use `--emit=masp`
  for binary packages. `midenc` also prints `Compiled <path>` after emitting a package file; use
  `--verbose=silent` if scripts require no status output.
- Custom `--target-dir` settings now contain nested Cargo artifacts under `<target-dir>/cargo`
  unless `CARGO_TARGET_DIR` or `CARGO_BUILD_TARGET_DIR` overrides that location. Update scripts
  that inspect the previous Cargo target paths. Toolchain selection now reads `MIDENUP_TOOLCHAIN`;
  `MIDEN_SYSROOT` remains the sysroot path and no longer also supplies the toolchain name.
- Dependency packages no longer remain in a persistent shared `target/miden/packages` directory.
  Each build uses a unique directory under `<target-dir>/packages`, removed when its session and
  registries are dropped. Set `MIDENC_PACKAGE_CACHE` before invoking the compiler to use a
  caller-owned directory whose lifetime you manage. Existing plain-Cargo and IDE workflows using
  the SDK should add the contract build-script support described in the
  [migration guide](sdk/sdk/MIGRATION.md).
- Component WIT is carried in the compiled package's `wit` section. Tools should read interfaces
  from `.masp` packages instead of `target/generated-wit/`. Rebuild dependencies and remove explicit
  `wit` keys for packages that embed their interface; those keys are now only a fallback for
  packages without embedded WIT.
- Rust builds now preserve mandatory Miden flags alongside inherited flags and honor
  `CARGO_ENCODED_RUSTFLAGS` over `RUSTFLAGS`, even when the encoded value is empty. Unset an empty
  encoded variable if `RUSTFLAGS` should apply. Native build scripts and proc macros compiled by
  nested Cargo builds now omit debug information to reduce disk use; debug those host tools in a
  separate Cargo build if symbols are needed.
- Default `cargo miden new` templates now come from the newest compatible `templates/v*` release
  newer than the embedded bundle, with the embedded bundle used when no update is available or
  release lookup fails. The old template repositories are no longer cloned. Use `--template-path`
  to select a fixed local template; invalid downloaded checksums fail generation.
- When generating custom templates, `cargo miden new` now uses Cargo's `[lib].path` and treats
  `project-kind = "library"` as a library. It no longer creates a package manifest at a virtual
  Cargo workspace root; workspace templates should provide manifests in their member crates.
  Relative `--compiler-path` values are now stored as absolute paths in generated projects.
- Shipped contract templates now include the required `[lib].path`. Add this key to older generated
  manifests that lack it, pointing to the Rust library source, normally `src/lib.rs`.
- Compiled packages no longer expose public procedures merely because they occur in a private
  component module. Explicitly export procedures through a public module or component interface
  if callers relied on those leaked exports.
- Linear-memory layout now reserves the module's declared memory before allocating compiler globals,
  function tables, and the heap, preventing overlap with zero-initialized Wasm memory. Values kept
  across branches and loop iterations are also preserved correctly when spilled to locals. Layouts
  that leave no representable heap address in the 32-bit address space now fail compilation instead
  of wrapping. Rebuild affected programs and update memory-layout, generated-code, and commitment
  baselines.
- Checked and overflowing integer operations now test the intended result or converted value rather
  than another stack operand, correcting range checks for small unsigned integers. Signed 1-bit
  and unsigned 32-bit conversion masks no longer overflow during compilation. The HIR evaluator's
  checked multiplication now multiplies instead of subtracting. Recheck results and expected traps
  for affected arithmetic.
- Typed FPI imports now reject mismatched core scalar types even when parameter counts match, and
  reject canonical memory offsets that do not fit a signed 32-bit offset. Invalid C-like enum tags
  now trap instead of passing through unchecked; fix mismatched declarations and invalid tags in
  callers or custom bindings.
- Advice-taint analysis now follows indirect callees and reports unconstrained arguments reaching
  external indirect-call targets. Unresolved indirect calls conservatively taint their results and
  memory, so existing analysis clients may receive additional findings.
- `local2reg` now promotes locals in parsed HIR instead of silently doing nothing. Textual HIR
  preserves layout attributes and debugger parameter metadata, and absolute symbol references are
  verified against the returned root. Signature display now prints results instead of repeating
  parameters. Regenerate affected HIR and diagnostic snapshots.
- Debug information now preserves declaration files and remapped Rust source paths and tracks
  variable lifetimes through optimization. Debuggers show unavailable values instead of stale
  stack locations; missing DWARF line and column information no longer produces misleading source
  locations. Update debug-info and source-location snapshots to reflect these corrections.
- `Session::filesystem_package_cache_dir()` now returns `Result<Option<PathBuf>, Report>`; handle
  cache-creation errors. `cargo_miden::BuildCommand::exec()` now returns `Result<Option<PathBuf>>`;
  handle `None` when compilation stops at a checkpoint or no package file is selected.
- `FrontendOutput`, `MidenComponent`, `CodegenOutput`, and `pipeline::backend::LoweredTarget`
  replace `account_component_metadata_bytes` with a `sections` field of type
  `midenc_frontend_wasm_metadata::PackageSections`. Move account metadata to
  `sections.account_component_metadata` and preserve `sections.component_wit` through custom stages.
- `ComponentId::is_synthetic_wrapper()` was removed. Mark generated wrappers with
  `Component::mark_synthetic_wrapper()` and query `Component::is_synthetic_wrapper()` instead of
  identifying wrappers by their name. `MasmComponent` struct literals must initialize
  `synthetic_wrapper` and `executable_entrypoint_without_init`; preserve the values supplied by
  lowering when rebuilding an artifact.
- Custom `TransformSpillsInterface` implementations must accept the new `value: ValueRef` argument
  in `convert_spill_to_store` and `convert_reload_to_load`. Use that analysis value to identify the
  spill slot while storing the spill operation's current operand.
- Replace `LogPrecompile`, `hir.log_precompile`, and `log_precompile` builder/emitter calls with
  `LogDeferred`, `hir.log_deferred`, and `log_deferred`. The operation now folds a precomputed
  statement digest into the VM's deferred root; adapt inputs to the new VM operation's contract.
- `DataSegmentLayout::next_available_offset()` now returns `Option<u32>`; handle `None` when no
  aligned address fits.
- `Operation::set_attribute` replaces an existing attribute of the same name instead of inserting
  a duplicate.
- Exhaustive `ParserError` matches must handle `UnknownDialect` and `SymbolAlreadyDefined`.
  `DisassembledWorld` struct literals must initialize `skipped_procedures`.
- Debug-expression consumers must replace the frame-base high-bit encoding helpers with
  `FrameBase` and `ResolvedFrameBase`. `ExpressionOp::FrameBase` now uses a typed `base` field
  instead of `global_index`; exhaustive matches must handle `ExpressionOp::ResolvedFrameBase`.
  Update readers of serialized HIR expressions for the typed local and global frame-base encodings;
  legacy expression tag 12 now decodes as a global base. Textual HIR uses
  `DW_OP_fbreg(local, 2+8)` in place of `DW_OP_fbreg(local, 2, 8)`.

## [0.10.0-rc.1]

### Compiler and `midenc`

- `midenc` accepts `--stop-after=CHECKPOINT` (or `MIDENC_STOP_AFTER`) to stop at a frontend's
  `parse`, `analyze`, `transform`, `lower`, or `assemble` checkpoint, or at a fully qualified
  checkpoint such as `hir.initial`. Invalid checkpoints report the names available for the
  selected input, and combining `--stop-after` with a legacy `-C*-only` stop flag is an error.
- Running `midenc` without an input path now compiles `miden-project.toml` from the current
  directory.
- Manifest-backed Rust projects now use the same checkpoint pipeline as standalone Rust, Wasm,
  HIR, and MASM inputs. Requested `--emit` artifacts are written instead of being silently
  discarded, including intermediate WAT, HIR, and MASM output.
- Fixed target isolation for Rust packages that declare both library and executable targets, so
  one target can no longer reuse another target's compiled output, read-only data, or account
  metadata.
- Fixed compilation of standalone HIR worlds and single-module worlds with no component. Top-level
  supporting modules that contain functions but no globals or data segments are now lowered and
  linked, and bodyless functions produce an input diagnostic instead of panicking.
- Ambiguous executable selection, unknown `--target` values, and unsupported kernel targets are
  now rejected consistently before Cargo runs, with diagnostics specific to the selected project.
- Fixed stdin file-type detection for Rust, `builtin.*` HIR, and additional MASM forms including
  `namespace`, `extern package`, and `begin`.
- Fixed nondeterministic HIR common-subexpression elimination that could make identical input
  produce different MASM, MAST digests, or package bytes depending on allocation order #1257
- Naturally aligned four-byte Wasm scalar loads and stores now lower directly through Miden
  element-space memory operations, reducing address-normalization overhead while preserving traps
  for accesses that violate their declared alignment.
- Account-procedure and transaction-script export metadata is now preserved through Wasm lowering,
  allowing protocol 0.16 tooling to construct account interfaces and locate transaction-script
  entrypoints.
- Assembled packages now preserve source provenance.

### `cargo-miden`

- Cargo-backed Miden builds now publish compiled dependency packages to the shared
  `<project>/target/miden/packages` cache and expose that cache to nested builds, allowing SDK FPI
  macros to resolve matching dependency packages during a normal build.
- Added the public `cargo_miden::bundle` API for accessing the embedded project scaffold and Rust
  templates and their SHA-256 digest, extracting the bundle, and locating a selected template. The
  embedded full-project scaffold includes its Claude Code settings, contract-build hook, and Miden
  SDK skills.

### `miden-objtool`

- `miden-objtool dump debug-info` now displays VM v0.25 package debug information, including the
  unified string, type, function, source-file, source-node, variable, inline-call, and location
  data.

### Libraries and public APIs

- Added the frontend-neutral `midenc_compile::pipeline` API. Custom frontends can register input
  extensions, checkpoint routes, stop aliases, and artifact renderers; callers can observe
  checkpoint artifacts, stop at a named checkpoint, or resume from an existing artifact with
  `Start::At`.
- Added the HIR `ValueEquivalence` strategy trait and matching identity, type-only, and
  ignore-value hasher/equivalence pairs for analyses that key operations by operands.
- Public `MidenComponent` and `CodegenOutput` artifacts now retain source provenance, exposed by
  `CodegenOutput::source_provenance()`. `LinkLibrary::tx_kernel()` provides the separate protocol
  0.16 transaction-kernel package.
- MASM file disassembly now reads the complete module tree rooted at the selected file, rather than
  lifting only that file's module, and project disassembly resolves dependencies through the VM
  v0.25 package model.

### Migration and breaking changes

- The compiler stack now targets Miden VM v0.25 and the protocol 0.16 transaction-kernel API.
  Update matching `miden-*` dependencies and rebuild `.masp` packages for the new package and debug
  information model. SDK projects must also mark callable component methods with
  `#[account_procedure]` and apply the protocol binding changes in the SDK's
  [changelog](sdk/CHANGELOG.md) and [migration guide](sdk/sdk/MIGRATION.md).
- `--release` now selects the release assembler profile; it previously assembled with the `dev`
  profile. Manifest-backed Rust roots and Rust dependencies compiled from source in release builds
  now derive Cargo's `profile.release.opt-level` from `--optimize`: `basic` uses `1`, `max` uses
  `3`, `size` uses `s`, `size-min` uses `z`, and `none` or `balanced` uses `2`. Update size,
  cycle-count, digest, and package-byte baselines where these settings change generated artifacts.
- Compiled dependency packages moved from each dependency's profile-specific target directory to
  `<project>/target/miden/packages`. Update scripts that read the previous paths directly; nested
  builds receive the new location through `MIDENC_PACKAGE_CACHE`.
- Note and transaction-script packages now embed the transaction-kernel package when it is not
  already present. This changes package dependencies, bytes, and digests; update artifact baselines
  and package-dependency inspection accordingly.
- `midenc-compile` removed the public `Stage` trait and `stages` module, along with
  `compile_to_memory_with_pre_assembly_stage`, `compile_to_optimized_hir`,
  `compile_to_unoptimized_hir`, and `compile_link_output_to_masm`. Use `pipeline::Pipeline`,
  `CompilationRequest`, checkpoint observers, and `Start::At`; `compile_to_memory` now returns
  `CompiledArtifact`. `Options` struct literals must also initialize `stop_after`, normally to
  `None`.
- `compile_link_output_to_masm_with_pre_assembly_stage` now takes an owned `'static` callback of
  type `FnMut(&pipeline::backend::LoweredTarget) -> CompilerResult<()>`. The callback may observe
  lowered output or fail the build, but it can no longer replace `CodegenOutput`.
- `midenc_session::Session` no longer owns or exposes a loaded `project`, and
  `Session::new_project` no longer accepts one. Use `pipeline::prepare_project` and
  `PreparedProject` when project access is required.
- `midenc_frontend_masm::disassemble_project_target` was replaced by
  `disassemble_project_target_with_sources`, which accepts `ProjectSourceInputs` and a
  `ProjectDependencyGraph`; `disassemble_module` now takes ownership of `Box<Module>`. Prefer
  `ProjectTargetInput::new` and `ExternalMetadata::default` over struct literals, which must now
  account for package and kernel inputs. MASM disassembly now rejects recursive call graphs; remove
  recursion before disassembling those procedures.
- `Session`, `DiagnosticsHandler`, and `Context::source_manager()` now expose
  `Arc<dyn SourceManager>` without a `Send + Sync` promise. Code that needs to send the source
  manager between threads must retain an appropriately typed handle itself.
- `MasmComponent` no longer has a `kernel` field or the `assemble` and `assemble_with_registry`
  methods. Use `source_inputs(target, session)` with the VM project assembler, or use the
  high-level compilation pipeline so metadata, read-only data, provenance, and kernel embedding
  are retained. Direct `MidenComponent` and `CodegenOutput` struct literals must initialize the
  new `source_provenance` field.
- Compiler events moved from raw trace codes to VM v0.25 event IDs: replace `TraceEvent` with
  `Event`, `TRACE_FRAME_START`/`TRACE_FRAME_END`/`TRACE_PRINT_LN` with
  `FRAME_START_EVENT`/`FRAME_END_EVENT`/`PRINT_LN_EVENT`, and `as_u32()` with `as_event_id()`.
  `Unknown` now contains `EventId`, and `AssertionFailed` was removed in favor of VM/core-library
  event handlers.
- Compiler intrinsics are now one preassembled package. Replace
  `intrinsics::load(name, source_manager)` and the removed intrinsic-module-name constants with
  `intrinsics::load()`, then link the returned package statically. `midenc_session::STDLIB` and
  `CompiledLibrary` were removed; use `CoreLibrary::default().package()` when the core package is
  needed. `LinkLibrary::load` now returns `Arc<Package>`, and the
  `midenc_codegen_masm::masm::{Library, KernelLibrary}` re-exports were removed in favor of the
  VM v0.25 package and project types.
- HIR value-equivalence strategies now decide type compatibility as well as identity. Replace
  `exact_value_match` with `DefaultValueEquivalence`; replace `ignore_value_equivalence` with
  `ValueTypeEquivalence` to retain its previous type-checking behavior. Use
  `IgnoreValueEquivalence` only when value identity and type should both be ignored, and add an
  explicit type comparison to custom closures that relied on the old implicit check.
- `miden-objtool decorators` and the public `miden_objtool::decorators` module were removed without
  replacement. The textual `dump debug-info` format changed to the v0.25 source-node model, and
  exhaustive matches on `DumpError` must handle the new `InvalidDebugInfo` variant.
