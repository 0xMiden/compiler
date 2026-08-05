# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
