//! The frontends for targets written in Rust.
//!
//! There are two, and the difference between them is how the WebAssembly is produced:
//!
//! - [`RUST_FRONTEND`] is the **project** frontend. Its targets are declared by a manifest, and
//!   building one means running `cargo`. This is what the registry holds under `rs`, because it
//!   is what a Rust *dependency* of any project is.
//! - [`RUST_STANDALONE_FRONTEND`] is the entry point for `midenc foo.rs`. One file is compiled
//!   to WebAssembly in this process — by `rustc` directly, or through a temporary Cargo project
//!   when the source declares dependencies.
//!
//! Past that point neither is Rust-specific at all: both hand their WebAssembly to
//! [`WasmFrontend::compile_wasm`](super::wasm::WasmFrontend), so both run
//! [`WASM_FRONTEND`](super::wasm::WASM_FRONTEND)'s route, publish its checkpoints and write its
//! inline `--emit` documents. That is why all three registrations declare the same route.
//!
//! Both claim the `rs` extension and no registry may hold both, which is exactly why the
//! standalone one is installed per request rather than registered; see
//! [`RUST_STANDALONE_FRONTEND`].

use alloc::{
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::cell::RefCell;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs, SourceFileProvenance};
use miden_mast_package::Package as MastPackage;
use midenc_hir::{Context, formatter::DisplayMany};
use midenc_session::{FileType, InputFile, InputType, Options, Session, diagnostics::Report};

use super::wasm::{WasmFrontend, WasmSource};
use crate::{
    CodegenOutput, CompilerResult,
    pipeline::{
        Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
        FrontendRegistration, TargetContext, TargetKey,
    },
};

/// Declares the frontend that handles targets rooted at a `.rs` file.
///
/// # A route describes what a frontend reaches *as the root*, and this one reaches all of it
///
/// A route is what `--stop-after`, the `-C` stop flags and `--emit` are validated against, so it
/// has to describe what the frontend actually publishes. It is consulted for the **selected root
/// target and nothing else**: `apply_stop_flags`, [`resolve_goal`](crate::pipeline::resolve_goal)
/// and the driver's emit observer all read `PreparedProject::frontend`, which preparation derives
/// from the *root* target's extension, and the observer additionally renders only for
/// [`TargetRole::Root`](crate::pipeline::TargetRole). Nothing anywhere asks a route what a
/// dependency reaches.
///
/// That is what makes one registration able to describe [`RustProjectFrontend`] honestly even
/// though its two arms differ. As the root, a manifest-backed Rust target is built by running
/// `cargo` to WebAssembly and then handing that WebAssembly to
/// [`WasmFrontend::compile_wasm`](super::wasm::WasmFrontend) — so it publishes exactly what a
/// `.wasm` target publishes, because it is run by that code. As a *dependency* it publishes
/// nothing, and nothing asks it to.
///
/// The route is therefore [`WASM_FRONTEND`](super::wasm::WASM_FRONTEND)'s, held to that by
/// `the_project_route_is_the_wasm_route`.
///
/// # Why there are still two Rust registrations
///
/// Not because the routes differ any more — they are identical — but because the two entry
/// points build different things: this one runs `cargo` over a manifest, and
/// [`RUST_STANDALONE_FRONTEND`] compiles one file in this process. A registration binds an
/// extension to *one* frontend constructor, and a registry may hold only one claim on `rs`; see
/// [`RUST_STANDALONE_FRONTEND`] for how the choice is made per request.
///
/// # Nothing on this route is rendered
///
/// Each document is written by the phase that produces it, reached through the shared
/// WebAssembly tail, so a renderer here would be a second writer. Each declaration below names
/// its writer, and each declares the private `unrendered` renderer, whose own documentation
/// explains why a second writer is not a harmless duplicate.
pub const RUST_FRONTEND: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("rust"),
    &["rs"],
    &[
        CheckpointId::WASM_PARSED,
        CheckpointId::HIR_INITIAL,
        CheckpointId::HIR_ANALYZED,
        CheckpointId::HIR_TRANSFORMED,
        CheckpointId::MASM_LOWERED,
        CheckpointId::PACKAGE_ASSEMBLED,
    ],
    &[
        ("parse", CheckpointId::WASM_PARSED),
        ("analyze", CheckpointId::HIR_ANALYZED),
        ("transform", CheckpointId::HIR_TRANSFORMED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    &[
        // `--emit=wat` is written by `WasmFrontend::emit_wat`, which this route reaches through
        // [`WasmFrontend::compile_wasm`](super::wasm::WasmFrontend). See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::WASM_PARSED,
            id: ArtifactId::WASM,
            render: unrendered,
        },
        // The *pre*-rewrite `--emit=hir` document is written by `WasmFrontend::translate`,
        // likewise reached through the shared tail. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_INITIAL,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // Nothing writes here, and nothing may: `backend::analyze` emits no document of its own,
        // and the HIR it publishes is the very same world `hir.initial` already wrote. See
        // [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_ANALYZED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // The *post*-rewrite `--emit=hir` document is written by
        // [`backend::apply_rewrites`](crate::pipeline::backend::apply_rewrites). See
        // [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_TRANSFORMED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // `--emit=masm` is written by [`backend::codegen`](crate::pipeline::backend::codegen),
        // the only point at which the lowered component exists in a form the session can write.
        // See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::MASM_LOWERED,
            id: ArtifactId::MASM,
            render: unrendered,
        },
        // The assembled package is written by [`crate::compile`]; see [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
            id: ArtifactId::PACKAGE,
            render: unrendered,
        },
    ],
    make_rust,
);

/// Build the frontend this registration declares.
fn make_rust(session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(RustProjectFrontend::new(session))
}

/// A renderer for the checkpoints on these two routes, every one of which something else
/// writes.
///
/// Every use of it names its writer at the declaration site. A second emission would not be a
/// harmless duplicate: [`Session::emit`] resolves an output type's destination once, so two
/// writers of one artifact either race for the same file or, when that destination is stdout,
/// print the artifact twice.
fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
    Ok(())
}

/// Declares the frontend that handles a **standalone** `.rs` file — `midenc foo.rs`.
///
/// # Why this is a second registration, given that the routes are now identical
///
/// It was once the route: a manifest-backed target published nothing this request could see, so
/// one route covering both would have offered stop points a `cargo` build never reached. That is
/// no longer true — [`RustProjectFrontend`] runs the same shared tail — and the reason is now
/// simply that the two entry points build *different things*. A standalone `.rs` is compiled by
/// `rustc`, or by a temporary Cargo project synthesized around it; a manifest-backed target is
/// built by `cargo` over the manifest the project declares. A [`FrontendRegistration`] binds an
/// extension to one constructor, so two constructors mean two registrations.
///
/// # This is not registered, and must not be
///
/// [`FrontendRegistry::register`](crate::pipeline::FrontendRegistry::register) rejects a second
/// claim on an extension, and the registry's `rs` entry stays [`RUST_FRONTEND`] — a Rust
/// *dependency* of any project is a cargo build, whatever the root of that project is. So a
/// standalone request carries this registration in
/// [`PreparedProject::frontend`](crate::pipeline::PreparedProject), which is what
/// `resolve_goal` and the driver's emit observer read, and installs a provider built from it
/// for the `rs` extension for that request alone — the request-scoped override
/// [`Pipeline::providers`](crate::pipeline::Pipeline) takes. That cannot reach a dependency: a
/// synthesized project has no Rust source dependencies, only the `miden-core` registry one.
///
/// Preparation answers with this registration, and the driver installs the override built from
/// it, for a standalone request's root target alone. The registry's own `rs` entry is untouched,
/// so every other Rust target in the graph is still built by [`RUST_FRONTEND`].
///
/// # Nothing on this route is rendered
///
/// Past `wasm.parsed` this *is* [`WASM_FRONTEND`](super::wasm::WASM_FRONTEND)'s route, run by
/// the same code, so it inherits that route's inline emissions and declares the same
/// `unrendered` table for the same reason: each document is written by the phase that produces
/// it, and a renderer here would be a second writer. Each declaration below names its writer.
pub const RUST_STANDALONE_FRONTEND: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("rust-standalone"),
    &["rs"],
    &[
        CheckpointId::WASM_PARSED,
        CheckpointId::HIR_INITIAL,
        CheckpointId::HIR_ANALYZED,
        CheckpointId::HIR_TRANSFORMED,
        CheckpointId::MASM_LOWERED,
        CheckpointId::PACKAGE_ASSEMBLED,
    ],
    &[
        ("parse", CheckpointId::WASM_PARSED),
        ("analyze", CheckpointId::HIR_ANALYZED),
        ("transform", CheckpointId::HIR_TRANSFORMED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    &[
        // `--emit=wat` is written by `WasmFrontend::emit_wat`, which this route reaches
        // through [`WasmFrontend::compile_wasm`]. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::WASM_PARSED,
            id: ArtifactId::WASM,
            render: unrendered,
        },
        // The *pre*-rewrite `--emit=hir` document is written by `WasmFrontend::translate`,
        // likewise reached through the shared tail. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_INITIAL,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // Nothing writes here, and nothing may: `backend::analyze` emits no document of its
        // own, and the HIR it publishes is the very same world `hir.initial` already wrote.
        // See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_ANALYZED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // The *post*-rewrite `--emit=hir` document is written by
        // [`backend::apply_rewrites`](crate::pipeline::backend::apply_rewrites). See
        // [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_TRANSFORMED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // `--emit=masm` is written by [`backend::codegen`](crate::pipeline::backend::codegen),
        // the only point at which the lowered component exists in a form the session can
        // write. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::MASM_LOWERED,
            id: ArtifactId::MASM,
            render: unrendered,
        },
        // The assembled package is written by [`crate::compile`], which emits it in both
        // `mast` and `masp` form once the pipeline hands it back. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
            id: ArtifactId::PACKAGE,
            render: unrendered,
        },
    ],
    make_rust_standalone,
);

/// Build the frontend the standalone registration declares.
fn make_rust_standalone(_session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(RustStandaloneFrontend::default())
}

/// Build the project the manifest at `manifest_path` declares, and lower it to Miden Assembly.
///
/// This is the **dependency** Rust entry point, and the counterpart of [`compile_rust_to_wasm`]:
/// where that one compiles a single source file in this process, this one runs `cargo` over a
/// whole project — which runs `midenc` again, nested, once per Rust crate. It is not the only
/// caller of that build: [`build_manifest_to_wasm`] stops one step earlier, and both delegate
/// to the same `manifest::cargo_build`.
///
/// `manifest_path` may name either a `miden-project.toml` or the `Cargo.toml` beside it; see
/// `manifest::cargo_build`, which does the resolution. `filesystem_cache_dir`, when given, is
/// where the nested build publishes and looks for compiled dependency packages; see
/// `manifest::cargo_env`.
///
/// # Why a [`CodegenOutput`] rather than an assembled package
///
/// The caller is `crate::cargo::cargo_build`, which builds a *source dependency* of some
/// other project. What that project's assembly needs from this one is the lowered component —
/// its Miden Assembly, its rodata, and its account-component metadata — not a package of its
/// own, so this stops at codegen.
///
/// # And why it publishes nothing
///
/// It has no [`TargetContext`], because a source dependency built this way is not a target of
/// the assembly in progress — there is no assembler callback to build one from. That is not a
/// gap: a dependency's checkpoints are precisely what must *not* reach this request's observers
/// or its goal, so the type system holding it here is the guard. The **root** target does not
/// come through here at all; see [`RustProjectFrontend::compile`].
pub fn compile_manifest(
    manifest_path: &Path,
    filesystem_cache_dir: Option<&Path>,
    context: Rc<Context>,
) -> CompilerResult<CodegenOutput> {
    let session = context.session_rc();
    let wasm = build_manifest_to_wasm(manifest_path, filesystem_cache_dir, &session)?;
    lower_wasm_artifact(wasm, context)
}

/// Run `cargo` over the project the manifest at `manifest_path` declares, and hand back the
/// WebAssembly it produced.
///
/// The front half of [`compile_manifest`], stopping where the Rust toolchain does. It exists
/// because a caller can want the WebAssembly *itself* rather than anything lowered from it —
/// to feed it back into the compiler as an input, under a session of its own — and neither
/// [`compile_manifest`] nor [`crate::compile_to_memory`] can serve that: one returns a lowered
/// component and the other an assembled package.
///
/// # This is `CargoBuildStage`, and the path is the contract
///
/// `tests/support/src/compiler_test.rs` builds every Cargo-based fixture through here and then
/// installs the result as `session.input`, which is how a fixture's WebAssembly is compiled
/// with the very options the test declared. It reached that build as
/// `midenc_compile::stages::CargoBuildStage` until that module was deleted; this path
/// replaces it, and — like the type it replaces — moving or narrowing it breaks that crate.
///
/// The example below is the guard, and it is a **doctest** on purpose: doctests compile as an
/// external crate, so this fails for the same reason `tests/support` would. An in-crate
/// `#[test]` naming the same path cannot — it resolves through `crate::` and survives the item
/// being narrowed to `pub(crate)`, which is precisely the edit that breaks the real consumer.
///
/// ```
/// use midenc_compile::pipeline::frontends::rust::build_manifest_to_wasm;
///
/// // Named, not called: calling it runs a real `cargo build`. What `tests/support` depends on
/// // is that this path resolves and that the item is callable with a manifest path, no
/// // filesystem package cache, and a session.
/// let _build = build_manifest_to_wasm;
/// ```
///
/// # The last artifact wins
///
/// A single-package build produces exactly one WebAssembly artifact; a multi-package
/// `--package` build produces several and only the last is taken. That is the behaviour as it
/// stands, inherited from the stage this replaces, rather than a choice made here.
pub fn build_manifest_to_wasm(
    manifest_path: &Path,
    filesystem_cache_dir: Option<&Path>,
    session: &Session,
) -> CompilerResult<InputFile> {
    let options = session.options.clone();
    let mut outputs = manifest::cargo_build(manifest_path, filesystem_cache_dir, options)?;
    outputs.pop().ok_or_else(|| {
        Report::msg(format!(
            "`cargo build` of '{}' produced no WebAssembly artifact",
            manifest_path.display()
        ))
    })
}

/// Lower the WebAssembly a manifest build produced, through to Miden Assembly.
///
/// The work is [`wasm::lower_wasm_input`](super::wasm::lower_wasm_input)'s: parsing the module,
/// analyzing it, rewriting it and lowering it are exactly what a `.wasm` input does, and that is
/// where the WebAssembly route's copy of those phases lives.
///
/// # Why not [`WasmFrontend::compile`](super::wasm::WasmFrontend), which is the same route
///
/// Two reasons, either alone decisive:
///
/// - `compile_wasm` publishes through a [`TargetContext`], which is built from an *assembler's*
///   view of a target. A nested dependency build has no assembler to build one from.
/// - it does not return a [`CodegenOutput`] at all. It returns `Flow<ProjectSourceInputs>` and
///   stashes the lowered component in the frontend under [`TargetContext::target_key`], for
///   `post_process` to collect — which is exactly what this caller needs handed back.
///
/// Reaching *past* it into [`backend::hir_to_masm`](crate::pipeline::backend::hir_to_masm)
/// would not help either: it takes the same [`TargetContext`]. So the phases are shared one
/// level down, at the functions each of them is, rather than at the frontend callback.
///
/// Named rather than inlined so that the half of the entry point which does *not* require a
/// multi-minute `cargo build -Z build-std` against the SDK is assertable on its own.
fn lower_wasm_artifact(wasm: InputFile, context: Rc<Context>) -> CompilerResult<CodegenOutput> {
    super::wasm::lower_wasm_input(&wasm, context)
}

/// Compile the Rust source `input` to WebAssembly, and hand back the file it produced.
///
/// This is the `.rs → .wasm` half of the old `ParseRustStage`, and it is the *only* copy.
///
/// The result is a path rather than the bytes because that is the shape its caller wants it in:
/// [`RustStandaloneFrontend`] reads it and attributes the translated module to it — which is what
/// makes a standalone `.rs` build's HIR named after the artifact, exactly as the legacy
/// `ParseRustStage → ParseWasmStage` chain named it.
///
/// # Where the output goes
///
/// `<target_dir>/<artifact name>.wasm` on the `rustc` route, and wherever `cargo` put its
/// single `cdylib` artifact on the other. Neither is chosen here; both are what the legacy
/// stage did.
pub fn compile_rust_to_wasm(input: &InputFile, session: &Session) -> CompilerResult<PathBuf> {
    let file_type = input.file_type();
    if !matches!(file_type, FileType::Rust) {
        return Err(Report::msg(format!("invalid input file: expected '.rs', got {file_type}")));
    }
    let options = &session.options;
    let build = RustBuild::select(input, session)?;
    match (&input.file, build) {
        (InputType::Real(path), RustBuild::CargoProject { dependencies }) => {
            let filename = path
                .file_name()
                .ok_or_else(|| Report::msg("invalid input path: not a valid file name"))?;
            let project_dir = prepare_temporary_cargo_project(path, filename, session)?;
            cargo_build(&project_dir, filename, dependencies, session, options)
        }
        (InputType::Real(path), RustBuild::Rustc) => rustc(path, None, session, options),
        (InputType::Stdin { name, input }, build) => {
            // Piped-in source has to reach the disk before either builder can see it, so it is
            // written under the artifact name in the system temporary directory — the same
            // place, and the same name, the legacy stage used.
            let tmp = std::env::temp_dir();
            let name_of_artifact = session.name.as_str();
            match build {
                RustBuild::CargoProject { dependencies } => {
                    let project_dir = tmp.join(name_of_artifact);
                    let src_dir = project_dir.join("src");
                    std::fs::create_dir_all(&src_dir).map_err(|err| {
                        Report::msg(format!("failed to create temporary Cargo project: {err}"))
                    })?;
                    let filename = format!("{}.rs", name.file_stem().unwrap_or("lib"));
                    let tmp_rs = src_dir.join(&filename);
                    std::fs::write(&tmp_rs, input).map_err(|err| {
                        Report::msg(format!("failed to write Rust input to temporary file: {err}"))
                    })?;
                    cargo_build(
                        &project_dir,
                        Path::new(&filename).as_os_str(),
                        dependencies,
                        session,
                        options,
                    )
                }
                RustBuild::Rustc => {
                    let tmp_rs = tmp.join(name_of_artifact).with_extension("rs");
                    std::fs::write(&tmp_rs, input).map_err(|err| {
                        Report::msg(format!("failed to write Rust input to temporary file: {err}"))
                    })?;
                    rustc(&tmp_rs, Some(&tmp), session, options)
                }
            }
        }
    }
}

/// How one standalone Rust source is turned into WebAssembly.
///
/// The two routes differ in what they can resolve, not in what they produce: `rustc` compiles
/// a single file against `core` and nothing else, so a source that needs *any* crate
/// dependency has to be built as a Cargo package instead. Two things ask for one:
///
/// - **Cargo frontmatter** in the source itself, which names dependencies outright. Only
///   looked for when `--cargo-frontmatter` asked for it.
/// - **The Miden protocol**, which some target types and some `-l` libraries require, and
///   which is delivered as the `miden` SDK crate.
///
/// A separate type rather than a bare `bool` so that the choice is nameable — and therefore
/// assertable — without running either builder, which is what a test of the Cargo route
/// otherwise costs.
#[derive(Debug)]
enum RustBuild {
    /// `rustc` compiles the file directly.
    Rustc,
    /// The file is copied into a temporary Cargo package and built by `cargo`.
    CargoProject {
        /// The dependency table the source's cargo frontmatter declared, if it declared one.
        ///
        /// `None` when this route was taken for the *other* reason — a protocol target — in
        /// which case `cargo_build` supplies the SDK dependencies by itself.
        dependencies: Option<toml_edit::Table>,
    },
}

impl RustBuild {
    /// Decide how `input` is built within `session`.
    ///
    /// Reading the source is part of the decision, because the frontmatter is *in* it; that
    /// read is the whole cost of asking.
    fn select(input: &InputFile, session: &Session) -> CompilerResult<Self> {
        let options = &session.options;
        let dependencies = if options.cargo_frontmatter {
            match &input.file {
                InputType::Real(path) => {
                    let working_dir = if path.is_absolute() {
                        path.parent().unwrap().to_path_buf()
                    } else {
                        let path = path.canonicalize().map_err(|err| {
                            Report::msg(format!("unable to canonicalize input file path: {err}"))
                        })?;
                        path.parent().unwrap().to_path_buf()
                    };
                    let input = std::fs::read_to_string(path)
                        .map_err(|err| Report::msg(format!("unable to read input: {err}")))?;
                    crate::cargo::parse_cargo_frontmatter(&input, &working_dir)?
                }
                InputType::Stdin { input, .. } => {
                    let input = core::str::from_utf8(input)
                        .map_err(|err| Report::msg(format!("input is not valid utf-8: {err}")))?;
                    let working_dir = std::env::current_dir().map_err(|err| {
                        Report::msg(format!("unable to obtain current working directory: {err}"))
                    })?;
                    crate::cargo::parse_cargo_frontmatter(input, &working_dir)?
                }
            }
        } else {
            None
        };
        let use_cargo = dependencies.is_some()
            || options.target_requires_protocol()
            || options.link_libraries.iter().any(|lib| lib.is_protocol());
        Ok(if use_cargo {
            Self::CargoProject { dependencies }
        } else {
            Self::Rustc
        })
    }
}

/// Creates a temporary Cargo project structure for the Rust source file at `path`
///
/// NOTE: This does not write the `Cargo.toml` file - that is handled by the caller, depending on
/// how the Cargo metadata is derived.
fn prepare_temporary_cargo_project(
    path: &Path,
    filename: &OsStr,
    session: &Session,
) -> CompilerResult<PathBuf> {
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    let project_dir = tmp.join(session.name.as_str());
    let src_dir = project_dir.join("src");
    let tmp_rs = src_dir.join(filename);
    std::fs::create_dir_all(&src_dir)
        .map_err(|err| Report::msg(format!("failed to create temporary Cargo project: {err}")))?;
    std::fs::copy(path, &tmp_rs).map_err(|err| {
        Report::msg(format!("failed to copy input file to temporary Cargo project: {err}"))
    })?;
    Ok(project_dir)
}

fn cargo_build(
    project_dir: &Path,
    filename: &OsStr,
    frontmatter_dependencies: Option<toml_edit::Table>,
    session: &Session,
    options: &Options,
) -> CompilerResult<PathBuf> {
    let package_name = session.name.as_str();
    // A standalone source file belongs to no package and so declares no version. The session's
    // synthesized project reported `0.0.0` here — `miden_project::Package::new`'s default, which
    // nothing on this path ever overrode — and this generated manifest is thrown away with the
    // temporary project, so the value only has to be a version `cargo` accepts.
    let package_version = "0.0.0";

    let mut dependencies = frontmatter_dependencies.unwrap_or_default();
    let requires_protocol = options.target_requires_protocol();
    if let Ok(path) = std::env::var("MIDENC_SOURCE_TREE")
        && std::env::var("MIDENC_LINK_FROM_SOURCE_TREE").is_ok_and(|v| v == "1")
    {
        let path = Path::new(&path);
        if !dependencies.contains_key("miden-sdk-alloc") {
            let alloc_path = path.join("sdk/alloc");
            dependencies.insert("miden-sdk-alloc", dependency_path_to_toml_item(&alloc_path));
        }
        if requires_protocol && !dependencies.contains_key("miden") {
            let miden_path = path.join("sdk/sdk");
            dependencies.insert("miden", dependency_path_to_toml_item(&miden_path));
        } else if !requires_protocol && !dependencies.contains_key("miden-stdlib-sys") {
            let stdlib_sys_path = path.join("sdk/stdlib-sys");
            dependencies.insert("miden-stdlib-sys", dependency_path_to_toml_item(&stdlib_sys_path));
        }
    } else {
        if !dependencies.contains_key("miden-sdk-alloc") {
            dependencies.insert("miden-sdk-alloc", str_to_toml_item("*"));
        }
        let requires_protocol = options.target_requires_protocol();
        if requires_protocol && !dependencies.contains_key("miden") {
            dependencies.insert("miden", str_to_toml_item("*"));
        } else if !requires_protocol && !dependencies.contains_key("miden-stdlib-sys") {
            dependencies.insert("miden-stdlib-sys", str_to_toml_item("*"));
        }
    }

    let cargo_toml = format!(
        "\
cargo-features = [\"trim-paths\"]

[package]
name = \"{package_name}\"
version = \"{package_version}\"
edition = \"2024\"
authors = []

[dependencies]
{dependencies}

[lib]
crate-type = [\"cdylib\"]
path = \"src/{filename}\"

[profile.release]
panic = \"abort\"
# optimize for size
opt-level = \"s\"
debug = true
trim-paths = [\"diagnostics\", \"object\"]
",
        filename = filename.display(),
    );

    let manifest_path = project_dir.join("Cargo.toml");
    std::fs::write(&manifest_path, &cargo_toml)
        .map_err(|err| Report::msg(format!("failed to generate temporary Cargo.toml: {err}")))?;

    let cargo_build_args = build_cargo_args(&manifest_path, options);

    // Enable memcopy and 128-bit arithmetic ops
    let mut rustflags = String::from("-C target-feature=+bulk-memory,+wide-arithmetic");
    // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it for
    // cfg-based dependency selection.
    rustflags.push_str(" --cfg miden");
    // Enable errors on missing stub functions
    rustflags.push_str(" -C link-args=--fatal-warnings");
    // Remove the source file paths in the data segment for panics
    // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
    rustflags.push_str(" -Zlocation-detail=none");
    // Build with panic=immediate-abort
    rustflags.push_str(" -Zunstable-options");
    rustflags.push_str(" -Cpanic=immediate-abort");
    if let Ok(inherited) = std::env::var("RUSTFLAGS")
        && !inherited.is_empty()
    {
        rustflags.push(' ');
        rustflags.push_str(&inherited);
    }
    if let Some(explicit) = options.rustflags.as_deref() {
        rustflags.push(' ');
        rustflags.push_str(explicit);
    }

    let wasi = if options.target_requires_protocol() {
        "wasip2"
    } else {
        "wasip1"
    };

    let cargo_env = std::env::var("CARGO").map(PathBuf::from).ok();
    let cargo_path = cargo_env.as_deref().unwrap_or_else(|| Path::new("cargo"));

    // When a specific cargo wasn't provided, resolve the toolchain to build under via rustup. This
    // honors an inherited RUSTUP_TOOLCHAIN or the active `rust-toolchain.toml` pin rather than
    // forcing the generic `nightly` channel, which matters because the SDK crates require a
    // specific nightly and this build runs from a temporary directory where directory overrides do
    // not apply.
    let toolchain = if cargo_env.is_none() {
        crate::rust::rustup_toolchain()
    } else {
        None
    };

    let mut cargo = std::process::Command::new(cargo_path);
    if let Some(toolchain) = toolchain.as_deref() {
        cargo.arg(format!("+{toolchain}"));
    }
    cargo.env("RUSTFLAGS", rustflags);
    // This env var is used by crates (e.g. `miden-field`) to distinguish compiling to Wasm for a
    // "real" Wasm runtime vs compiling to Wasm as an intermediate artifact that will be compiled
    // to Miden VM code by `midenc`.
    cargo.env("MIDENC_TARGET_IS_MIDEN_VM", "1");
    cargo.args(&cargo_build_args);

    // Handle the target for buildable commands
    crate::rust::install_wasm32_target(wasi, toolchain.as_deref())?;

    cargo.arg("--target").arg(format!("wasm32-{wasi}"));

    // It will output the message as json so we can extract the wasm files that will be
    // componentized
    cargo.arg("--message-format").arg("json-render-diagnostics");
    cargo.stdout(std::process::Stdio::piped());
    cargo.stderr(std::process::Stdio::inherit());

    let artifacts = crate::rust::spawn_cargo(cargo, cargo_path)?;

    let mut outputs: Vec<PathBuf> = artifacts
        .into_iter()
        .flat_map(|a| a.filenames)
        .filter_map(|path| {
            if path.extension().is_some_and(|ext| ext == "wasm") {
                Some(path.into_std_path_buf())
            } else {
                None
            }
        })
        .collect();

    // We expect just a single artifact named `{package_name}.wasm`
    if outputs.len() != 1 {
        Err(Report::msg(format!(
            "expected `cargo build` to produce a single artifact, got: {outputs:#?}"
        )))
    } else {
        Ok(outputs.pop().unwrap())
    }
}

fn rustc(
    input: &Path,
    tmp_dir: Option<&Path>,
    session: &Session,
    options: &Options,
) -> CompilerResult<PathBuf> {
    let package_name = session.name.as_str();

    log::debug!(target: "rustc", "preparing to invoke rustc for {package_name}");
    log::debug!(target: "rustc", "  current_dir = {}", options.current_dir.display());
    log::debug!(target: "rustc", "  target_dir  = {}", options.target_dir.display());
    log::debug!(target: "rustc", "  tmp_dir     = {}", tmp_dir.unwrap_or(Path::new("unknown")).display());

    // Output is the same name as the input, just with a different extension
    let output_file = options.target_dir.join(format!("{package_name}.wasm"));

    // Set up the command used to compile the test inputs (typically Rust -> Wasm)
    let mut command = std::process::Command::new("rustc");
    command.current_dir(&options.current_dir);
    // Pipe output of command to terminal
    if options.diagnostics.is_verbose() {
        command.stdout(std::process::Stdio::inherit());
        command.stderr(std::process::Stdio::inherit());
    } else {
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
    }

    // If `RUSTFLAGS` is present, convert them to `rustc` flags
    let mut rustflags = std::env::var("RUSTFLAGS").ok().unwrap_or_default();
    if let Some(explicit) = options.rustflags.as_deref() {
        if !rustflags.is_empty() {
            rustflags.push(' ');
        }
        rustflags.push_str(explicit);
    }
    let rustflags = rustflags.split_ascii_whitespace().collect::<Vec<_>>();
    let mut rustc_flags = Vec::with_capacity(rustflags.len());
    let mut rustflags = rustflags.into_iter();
    let mut target = None;
    while let Some(flag) = rustflags.next() {
        if flag == "--target"
            && let Some(value) = rustflags.next()
        {
            target = Some(value.to_string());
            continue;
        } else if flag == "-C"
            && let Some(value) = rustflags.next()
        {
            if value == "panic=immediate-abort" {
                continue;
            }
            rustc_flags.extend([flag, value]);
        } else {
            rustc_flags.push(flag);
        }
    }

    let mut command = command;
    command
        .arg("--crate-name")
        .arg(package_name.replace("-", "_"))
        .args(["--crate-type", "cdylib"])
        .args(["--edition", "2024"])
        // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it for
        // cfg-based dependency selection.
        .args(["--cfg", "miden"])
        // Enable errors on missing stub functions
        .args(["-C", "link-args=--fatal-warnings"])
        .arg("--remap-path-scope=diagnostics,debuginfo,coverage,object")
        .arg("--remap-path-prefix")
        .arg(format!("{}=.", options.current_dir.display()));
    for remap_prefix in options.remap_path_prefixes.iter() {
        command.args([
            "--remap-path-prefix".into(),
            format!(
                "{}={}",
                remap_prefix.source_prefix().display(),
                remap_prefix.target_prefix().display()
            ),
        ]);
    }
    if let Some(tmp_dir) = tmp_dir {
        command.arg("--remap-path-prefix").arg(format!("{}=.", tmp_dir.display()));
    }
    command
        .args(["-Z", "unstable-options"])
        // Remove the source file paths in the data segment for panics
        // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
        .args(["-Z", "location-detail=none"])
        .arg("-g") // generate debug info
        .args(["-C", "opt-level=s"]) // optimize for size
        .args(["-C", "target-feature=+wide-arithmetic"])
        .args(rustc_flags)
        .arg("--target")
        .arg(target.as_deref().unwrap_or("wasm32-wasip1"))
        .arg("-o")
        .arg(&output_file)
        .arg(input);
    log::debug!(target: "rustc", "executing `{} {}`", command.get_program().display(),
        DisplayMany::new(command.get_args().map(|arg| arg.display()), " ")
    );
    let output = command
        .output()
        .map_err(|err| Report::msg(format!("failed to execute `rustc`: {err}")))?;
    if !output.status.success() {
        return Err(Report::msg(
            "`rustc` returned an error when compiling the input, see stderr output for more \
             details",
        ));
    }

    Ok(output_file)
}

fn build_cargo_args(manifest_path: &Path, options: &Options) -> Vec<String> {
    let mut args = vec!["build".to_string()];

    // Add build-std flags required for Miden compilation
    args.extend(
        [
            "-Z",
            "build-std=core,alloc,panic_abort",
            "-Z",
            "build-std-features=optimize_for_size",
        ]
        .into_iter()
        .map(|s| s.to_string()),
    );

    // Configure profile settings
    let cfg_pairs: Vec<(&str, &str)> = vec![
        ("profile.dev.panic", "\"abort\""),
        ("profile.dev.opt-level", "1"),
        ("profile.dev.overflow-checks", "false"),
        ("profile.dev.debug", "true"),
        ("profile.dev.debug-assertions", "false"),
        ("profile.release.opt-level", "\"s\""),
        ("profile.release.lto", "true"),
        ("profile.release.codegen-units", "1"),
        ("profile.release.panic", "\"abort\""),
    ];

    for (key, value) in cfg_pairs {
        args.push("--config".to_string());
        args.push(format!("{key}={value}"));
    }

    // Forward cargo-specific options
    if options.profile == "release" {
        args.push("--release".to_string());
    }

    args.push("--manifest-path".to_string());
    args.push(manifest_path.to_string_lossy().to_string());

    if options.workspace {
        args.push("--workspace".to_string());
    }

    for package in &options.packages {
        args.push("--package".to_string());
        args.push(package.to_string());
    }

    args
}

fn dependency_path_to_toml_item(path: &Path) -> toml_edit::Item {
    use toml_edit::*;

    let mut table = Table::new();
    table.insert("path", Item::Value(Value::String(Formatted::new(path.display().to_string()))));
    Item::Table(table)
}

fn str_to_toml_item(s: &str) -> toml_edit::Item {
    use toml_edit::*;

    Item::Value(Value::String(Formatted::new(s.to_string())))
}

/// Compiles a target whose sources are Rust, by shelling out to cargo.
///
/// # Two arms, divided by role, and the division is what this type is about
///
/// The **root** target — the one the request selected — is built by running `cargo` to
/// WebAssembly and then handing that WebAssembly to the shared WebAssembly tail, in this
/// process, against the `Context` the driver's own `Session` backs. It therefore publishes
/// every checkpoint on [`RUST_FRONTEND`]'s route, and can stop at any of them.
///
/// A **dependency** is built by `crate::cargo::cargo_build`, which constructs a fresh
/// `Options`, `Session` and `Context` and compiles against them. It publishes nothing, and must
/// not: every `--stop-after`, `--emit`, `-Zlint` and `-Canalyze-only` names the top-level target
/// alone, and a dependency stopped at its own analysis would never produce the package the root
/// is waiting for. The arm is *structurally* unable to publish — it never receives a
/// [`TargetContext`] beyond the role check, and the function it calls,
/// [`compile_manifest`], has no way to publish at all.
///
/// # Three caches, one per thing that is expensive to recompute
///
/// [`Frontend::compile`], [`Frontend::provenance`] and [`Frontend::post_process`] are each
/// called for the same target, the last of them against a *fresh* [`TargetContext`], and
/// `provenance` repeatedly and possibly before `compile`. Everything memoized here is keyed by
/// [`TargetContext::target_key`] rather than by package identity: a project with both a `[lib]`
/// and a `[[bin]]` has one package id for two targets, so a package-keyed cache serves the
/// library's codegen output to the executable.
pub struct RustProjectFrontend {
    session: Rc<Session>,
    /// A **dependency** target's cargo build output, and any seed a caller supplied.
    ///
    /// Not the root's: the root's lowering is done by `wasm`, which keeps its own.
    compiled: RefCell<BTreeMap<TargetKey, CodegenOutput>>,
    /// The WebAssembly `cargo` produced for a **root** target.
    ///
    /// Memoized because either of two callbacks may reach it first — `provenance` describes the
    /// WebAssembly and `compile` lowers it — and a `cargo` build is the most expensive thing
    /// this frontend does.
    built: RefCell<BTreeMap<TargetKey, WasmSource>>,
    /// The WebAssembly half of the **root** arm, which is everything past the cargo build.
    ///
    /// Held rather than reimplemented, exactly as [`RustStandaloneFrontend`] holds one: the two
    /// routes then publish the same checkpoints, write the same inline `--emit` documents, and
    /// stash the same [`CodegenOutput`] for post-processing, none of which a copy could be
    /// relied on to keep true.
    wasm: WasmFrontend,
}

impl RustProjectFrontend {
    /// Construct a frontend that builds within `session`, with an empty cache.
    pub fn new(session: Rc<Session>) -> Self {
        Self {
            session,
            compiled: RefCell::new(BTreeMap::new()),
            built: RefCell::new(BTreeMap::new()),
            wasm: WasmFrontend::default(),
        }
    }

    /// Construct a frontend whose cache already holds `output` for the target `key` names.
    ///
    /// For the caller that has *already* produced this target's [`CodegenOutput`] in-process:
    /// it hands the output over so that [`Frontend::compile`],
    /// [`Frontend::provenance`] and [`Frontend::post_process`] all serve this target from
    /// the seed, and no nested cargo build is spawned to reproduce work the running process
    /// just did.
    ///
    /// # Building the key
    ///
    /// `key` must be built from the *selector-resolved target*, not from the package. This
    /// is the hazard the switch to target keying introduces: the seed key this replaces was
    /// `(package name, version)`, which is a property of the package, but `compile`,
    /// `provenance` and `post_process` all look up [`TargetContext::target_key`], which
    /// carries the target's name and [`TargetType`](midenc_session::miden_project::TargetType)
    /// too. A package-derived key still type-checks and still inserts, it simply never
    /// matches: the seed is silently ignored, and the target is either rebuilt by cargo or,
    /// in `post_process`, reported as the `internal error` a cache miss raises. Resolve the
    /// target from the selector first and build the key from it.
    pub fn seeded(session: Rc<Session>, key: TargetKey, output: CodegenOutput) -> Self {
        Self {
            session,
            compiled: RefCell::new(BTreeMap::from([(key, output)])),
            built: RefCell::new(BTreeMap::new()),
            wasm: WasmFrontend::default(),
        }
    }

    /// Where the nested builds publish and look for compiled dependency packages.
    ///
    /// Derived from `self.session` — the session the project `cargo` is invoked *in* — rather
    /// than from `cx.session()`: the cache lives under the root project's `target/` directory
    /// and must be the same for every target of the build.
    fn filesystem_cache_dir(&self) -> Option<PathBuf> {
        self.session.filesystem_package_cache_dir()
    }

    /// Run `cargo` over this **root** target's manifest, and hand back the WebAssembly it
    /// produced.
    ///
    /// Memoized, because two callbacks want it and either may come first: `provenance` describes
    /// this WebAssembly and `compile` lowers it. Without the memo the assembler's provenance
    /// hashing would spawn a `cargo` build per call.
    ///
    /// # The `Session` this uses is the driver's, and that is the rule
    ///
    /// The root project build gets the `Session` and `Context` the driver constructed;
    /// everything else gets a fresh `Session` carrying only those options that are inherently
    /// global. Almost every compiler option names the *root* target — `--entrypoint` says which
    /// function of the root is the program's entry, `--test-harness` asks for extra code in the
    /// root's generated main, `--emit` and the `-C` stop flags describe what the caller wants to
    /// see of the root — and inheriting any of them into a dependency would be wrong: a
    /// dependency stopped at its own analysis never produces the package the root is waiting for.
    ///
    /// Both arms used to run through [`crate::cargo::cargo_build`], which builds `nested_options`
    /// from a fixed subset. That is right for a dependency and wrong for the root: the root's
    /// `--entrypoint` was dropped, so codegen produced a component with no entrypoint, and
    /// `MasmComponent::source_inputs` then emitted a *library* root module for an executable
    /// target — which `load_target_sources` rejects with "requested target type is executable,
    /// but root module provided to assembler ... is library".
    fn built_for_root(&self, cx: &TargetContext<'_>) -> CompilerResult<WasmSource> {
        let key = cx.target_key();
        if let Some(found) = self.built.borrow().get(&key) {
            return Ok(found.clone());
        }

        let cargo_manifest_path = cx.assembly().manifest_path.with_file_name("Cargo.toml");
        let wasm = build_manifest_to_wasm(
            &cargo_manifest_path,
            self.filesystem_cache_dir().as_deref(),
            &cx.session(),
        )?;
        let source = WasmFrontend::read_input(&wasm)?;
        self.built.borrow_mut().insert(key, source.clone());
        Ok(source)
    }

    /// Build this **dependency** target with cargo, uncached.
    ///
    /// # Why the role is a branch rather than a set of copied options
    ///
    /// Expressing the owner's root-versus-dependency rule as a branch on
    /// [`TargetRole`](crate::pipeline::TargetRole) is what makes the dependency case
    /// *structurally* unable to inherit root options, or to publish a checkpoint: this arm never
    /// receives the root `Context`, and [`compile_manifest`] — which is what
    /// [`crate::cargo::cargo_build`] ends in — takes no [`TargetContext`] and so has nothing to
    /// publish through. Neither is a list of things someone must remember not to do. The role is
    /// the assembler's own answer, derived per callback by
    /// [`FrontendProvider::role_of`](crate::pipeline::FrontendProvider) —
    /// [`MasmProjectFrontend`](super::masm::MasmProjectFrontend) makes root-only decisions the
    /// same way.
    ///
    /// **No option was reclassified.** A dependency's `nested_options` are exactly what they were;
    /// the change is that the root no longer goes through them.
    fn build_dependency(&self, cx: &TargetContext<'_>) -> CompilerResult<CodegenOutput> {
        debug_assert!(!cx.role().is_root(), "the root target is built by `built_for_root`");
        let assembly = cx.assembly();
        let cargo_opts = crate::cargo::CargoOptions::from_compiler(&self.session.options)?;
        let source_manager = self.session.source_manager.clone();
        crate::cargo::cargo_build(
            assembly.package.name().to_string(),
            assembly.target,
            assembly.manifest_path.with_file_name("Cargo.toml"),
            self.filesystem_cache_dir().as_deref(),
            &self.session.options,
            &cargo_opts,
            source_manager,
        )
    }

    /// What was lowered for this target, whichever arm produced it.
    ///
    /// A dependency's build and a caller-supplied seed live in `compiled`; the root's lowering
    /// is kept by the embedded [`WasmFrontend`], because that is what performed it.
    fn lowered(&self, key: &TargetKey) -> Option<CodegenOutput> {
        self.compiled.borrow().get(key).cloned().or_else(|| self.wasm.lowered_for(key))
    }
}

impl Frontend for RustProjectFrontend {
    /// Build this target with cargo, returning the Miden Assembly it produced.
    ///
    /// # The root target publishes; a dependency does not
    ///
    /// For the **root** target, `cargo` produces WebAssembly and everything past that is what a
    /// `.wasm` target does — so the WebAssembly is handed to the embedded [`WasmFrontend`],
    /// which publishes `wasm.parsed`, `hir.initial` and, through
    /// [`backend::hir_to_masm`](crate::pipeline::backend::hir_to_masm), the three that follow.
    /// Any of them may be the request's goal, and the [`Flow::Break`] that follows is returned
    /// from here to `provide_sources_interruptible`, the one callback that can express a stop.
    ///
    /// For a **dependency**, `crate::cargo::cargo_build` builds a fresh `Options`, `Session` and
    /// `Context` and compiles against them, and hands back only a finished [`CodegenOutput`].
    /// Nothing of it is published, and nothing may be: every `--stop-after`, `--emit`, `-Zlint`
    /// and `-Canalyze-only` refers to the top-level target alone, dependencies must be fully
    /// assembled before the top-level target can begin, and the fresh `Options` those builds get
    /// deliberately carry none of those flags. A dependency that stopped at its own analysis
    /// would never produce the package the root is waiting for.
    ///
    /// # Anything already lowered is served from the cache, ahead of either arm
    ///
    /// Two things arrive here, and the lookup has to see both. A caller-supplied *seed* is a
    /// [`CodegenOutput`] this process already produced, and serving it spares the cargo build
    /// entirely, whatever the target's role. And a *second callback for one target* must be
    /// served rather than rebuilt — which for the root means consulting what the embedded
    /// [`WasmFrontend`] stashed, because that is where the root's lowering lives.
    ///
    /// The root half of that is not a saving but a correctness guard. Re-entering `compile_wasm`
    /// would re-translate, re-publish every checkpoint on the route — so `--emit=wat`, both
    /// `--emit=hir` documents and `--emit=masm` would all be written a second time — and then
    /// fail on the capture slot's "already captured" invariant. Nothing in tree calls
    /// `provide_sources_interruptible` twice for one target, so this is a guard rather than a
    /// path; it is here because the arrangement it replaced had it for free, from a cache that
    /// held every arm's output.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        let key = cx.target_key();
        let session = cx.session();
        if let Some(found) = self.lowered(&key) {
            return Ok(Flow::Continue(
                found.component.source_inputs(cx.assembly().target, &session)?,
            ));
        }

        if cx.role().is_root() {
            return self.wasm.compile_wasm(cx, self.built_for_root(cx)?);
        }

        let compiled = self.build_dependency(cx)?;
        let source_inputs = compiled.component.source_inputs(cx.assembly().target, &session)?;
        self.compiled.borrow_mut().insert(key, compiled);
        Ok(Flow::Continue(source_inputs))
    }

    /// The build provenance of this target's sources.
    ///
    /// Called repeatedly while the assembler hashes the dependency closure, and possibly before
    /// `compile` has run at all, so every arm memoizes.
    ///
    /// # The root's provenance is the WebAssembly, and is answered without lowering anything
    ///
    /// This is what keeps publishing out of a callback that cannot express a stop:
    /// [`Frontend::provenance`] must never publish and never stop, so the root arm goes only as
    /// far as the `cargo` build and describes what came out of it. The lowering — and with it
    /// every checkpoint — happens in `compile`. The answer is the same either way: the
    /// provenance a lowered root carries is the one the shared tail recorded for exactly this
    /// WebAssembly, and both arms fill the same memo inside the embedded [`WasmFrontend`].
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        let key = cx.target_key();
        {
            let compiled = self.compiled.borrow();
            if let Some(found) = compiled.get(&key) {
                return Ok(found.source_provenance());
            }
        }

        if cx.role().is_root() {
            let source = self.built_for_root(cx)?;
            return self.wasm.provenance_of(cx, &source);
        }

        let compiled = self.build_dependency(cx)?;
        let provenance = compiled.source_provenance();
        self.compiled.borrow_mut().insert(key, compiled);
        Ok(provenance)
    }

    /// Attach the account-component metadata and rodata this target's build produced.
    ///
    /// The assembler builds a fresh context for this call, so the build is recovered by
    /// [`TargetContext::target_key`] — from `compiled` for a dependency or a seed, and from the
    /// embedded [`WasmFrontend`] for the root, whose lowering it performed. A miss in both means
    /// the assembler asked to post-process a target this frontend never compiled, which is a
    /// compiler bug: report it rather than indexing into a map and panicking.
    fn post_process(
        &self,
        package: &mut MastPackage,
        cx: &TargetContext<'_>,
    ) -> CompilerResult<()> {
        let key = cx.target_key();
        let found = self.lowered(&key).ok_or_else(|| {
            Report::msg(format!(
                "internal error: cannot post-process target '{}' of package '{}': no cargo build \
                 was cached for it, so `compile` never ran for this target",
                key.name(),
                key.package()
            ))
        })?;
        crate::pipeline::assembly::post_process_package(
            package,
            &found.component,
            &found.sections,
            cx.assembly().target,
            cx.assembly().package_registry,
        )
    }
}

/// Compiles a target whose root is a single Rust source file.
///
/// The build is two halves, and only the first is Rust-specific: [`compile_rust_to_wasm`]
/// produces a WebAssembly module in this process, and everything past that is what a `.wasm`
/// target does. The second half is *shared*, not reimplemented — this frontend holds a
/// [`WasmFrontend`] and hands the module to its `compile_wasm` — so the two routes
/// publish the same checkpoints, write the same inline `--emit` documents, and stash the same
/// [`CodegenOutput`] for post-processing, none of which a copy could be relied on to keep true.
///
/// The embedded frontend also owns the per-target state, which is why [`Frontend::post_process`]
/// simply delegates: what codegen produced was stashed there, under
/// [`TargetContext::target_key`].
#[derive(Default)]
pub struct RustStandaloneFrontend {
    /// The WebAssembly half of this route, which is everything past `wasm.parsed`.
    wasm: WasmFrontend,
}

impl RustStandaloneFrontend {
    /// Locate this target's Rust source.
    ///
    /// # Which of the two sources wins
    ///
    /// A stdin-backed input is the one case a path cannot serve: it exists only in memory, and
    /// the target root synthesized for it names a file that was never written. So it is taken
    /// from [`TargetContext::input`], and everything else from
    /// `assembly().resolved_target_root` — which is also the safer choice for a file-backed
    /// input, because a provider serves every callback for its extension and a same-extension
    /// dependency would otherwise be compiled from the *root's* source. See
    /// [`WasmFrontend`](super::wasm::WasmFrontend), which is built the same way and explains
    /// why that combination is expected to be unreachable rather than relied upon.
    fn read(cx: &TargetContext<'_>) -> CompilerResult<InputFile> {
        match cx.input() {
            Some(input) if matches!(input.file, InputType::Stdin { .. }) => Ok(input.clone()),
            _ => {
                let path = cx.assembly().resolved_target_root.clone();
                let file_type = FileType::try_from(path.as_ref()).map_err(|err| {
                    Report::msg(format!("invalid target root '{}': {err}", path.display()))
                })?;
                Ok(InputFile::new(file_type, InputType::Real(path.into_path_buf())))
            }
        }
    }
}

impl Frontend for RustStandaloneFrontend {
    /// Compile this target's Rust to WebAssembly, then finish as a `.wasm` target would.
    ///
    /// Every checkpoint on this route is published by the shared tail, `wasm.parsed` included:
    /// the module this half produces is exactly what that checkpoint names, so publishing it
    /// here as well would publish it twice.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        let source = Self::read(cx)?;
        let path = compile_rust_to_wasm(&source, &cx.session())?;
        let wasm = std::fs::read(&path).map_err(|err| {
            Report::msg(format!(
                "failed to read the WebAssembly compiled from '{}' at '{}': {err}",
                source.file_name(),
                path.display()
            ))
        })?;
        self.wasm.compile_wasm(cx, WasmSource::new(path.into_boxed_path(), wasm))
    }

    /// Attach the account-component metadata and the rodata advice map this target produced.
    ///
    /// Delegated, because the shared tail is what stashed them.
    fn post_process(
        &self,
        package: &mut MastPackage,
        cx: &TargetContext<'_>,
    ) -> CompilerResult<()> {
        self.wasm.post_process(package, cx)
    }

    /// This target's build provenance: its Rust source, as written.
    ///
    /// **Not** the WebAssembly it compiles to, which is what
    /// [`WasmFrontend::provenance`](super::wasm::WasmFrontend) answers with and what the
    /// embedded frontend memoizes for its own use. Two reasons, and the first is decisive:
    /// this callback is reached while the assembler hashes the dependency closure, repeatedly
    /// and possibly *before* `compile` has run at all, and answering with the WebAssembly would
    /// mean invoking `rustc` — or `cargo` — to answer a question about provenance.
    /// [`Frontend::provenance`] rules that out.
    ///
    /// The second is that it is the better answer regardless: provenance is what a human reads
    /// out of a package to see what it was built from, and for `midenc foo.rs` that is
    /// `foo.rs`. Reading one text file is the cheap case [`Frontend::provenance`] exempts from
    /// memoization, so nothing is cached here.
    ///
    /// This does leave the `source_provenance` the shared tail records *inside* the compiled
    /// component holding the disassembled WebAssembly instead, which is what the legacy
    /// `.rs → .wasm → HIR` chain recorded. That field is write-only on this route — the
    /// provenance the assembler hashes is what comes back from here — so the two are not in
    /// competition; unifying them belongs with the field's only reader,
    /// [`RustProjectFrontend::provenance`].
    ///
    /// # Known limitation: `support` is empty, and the Cargo route has sources it does not name
    ///
    /// A standalone target is one file, so on the `rustc` route the root *is* the whole of the
    /// build's sources. On the Cargo route it is not: cargo frontmatter may declare a path
    /// dependency — `example = { path = "vendor/example" }` — which is compiled into the target
    /// and appears nowhere in what this returns. The assembler hashes this answer to decide
    /// whether a cached build is still current, so an edit under `vendor/example/` leaves a
    /// stale build looking current.
    ///
    /// It is latent rather than live: nothing routes a standalone input through the assembler
    /// yet, so this callback is unreached until that flip. Naming the frontmatter's path
    /// dependencies here means walking them, which is `RustBuild::select`'s table — reachable,
    /// but a change of what this method costs, and worth deciding once the caller exists.
    /// [`RustProjectFrontend::provenance`] does not have the problem: it derives from the
    /// completed cargo build, which knows every file it read.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        let source = Self::read(cx)?;
        let root = match &source.file {
            InputType::Real(path) => SourceFileProvenance::from_path(path.clone())?,
            InputType::Stdin { name, input } => SourceFileProvenance {
                path: name.as_path().to_path_buf().into_boxed_path(),
                content: core::str::from_utf8(input)
                    .map_err(|err| Report::msg(format!("failed to read {name}: {err}")))?
                    .to_string()
                    .into_boxed_str(),
            },
        };
        Ok(ProjectSourceProvenanceInputs {
            root,
            support: Vec::new(),
        })
    }
}

pub(crate) mod manifest {
    //! The manifest-driven `cargo` build.
    //!
    //! Everything here is a move: it ran as `midenc_compile::stages::cargo::support` and is
    //! reached now through [`compile_manifest`](super::compile_manifest) and
    //! [`build_manifest_to_wasm`](super::build_manifest_to_wasm), which are the two depths a
    //! caller can want it to. The one change of substance is that the environment the nested
    //! `cargo` is spawned with is now named — see [`cargo_env`] — so the filesystem package
    //! cache can be asserted without running a build.
    //!
    //! It is a module rather than a set of free functions in the parent because the names
    //! collide: `cargo_build` and `build_cargo_args` here are *not* the ones the standalone
    //! entry point uses, and both pairs have to keep their own names to stay recognisable
    //! against the code they were moved from. Disambiguate by path.
    //!
    //! No `#[cfg(feature = "std")]`: [`crate::pipeline`] is std-only, so the gate the original
    //! carried is implied by where this now lives.

    use std::{
        boxed::Box,
        path::{Path, PathBuf},
        string::{String, ToString},
        sync::Arc,
        vec::Vec,
    };

    use miden_assembly::{DefaultSourceManager, SourceManager};
    use miden_mast_package::TargetType;
    use midenc_hir::{
        Report,
        diagnostics::{IntoDiagnostic, SourceManagerExt},
    };
    use midenc_session::{InputFile, OptLevel, ProjectManifest, miden_project};

    use crate::{CompilerResult, cargo::CargoOptions};

    /// Executes a Cargo-based build with the provided compiler options and package registry
    pub(crate) fn cargo_build(
        manifest_path: &Path,
        filesystem_cache_dir: Option<&Path>,
        mut compiler_opts: Box<midenc_session::Options>,
    ) -> CompilerResult<Vec<InputFile>> {
        // Extract cargo-specific options from parsed Compiler struct
        compiler_opts.manifest_path = Some(manifest_path.to_path_buf());
        let cargo_opts = CargoOptions::from_compiler(&compiler_opts)?;

        let cwd = compiler_opts.current_dir.clone();
        let (project_dir, project_manifest_path) = match compiler_opts.manifest_path.as_mut() {
            Some(manifest_path)
                if manifest_path
                    .file_stem()
                    .is_some_and(|stem| stem.eq_ignore_ascii_case("miden-project")) =>
            {
                let manifest_path = manifest_path.clone();
                let cwd = manifest_path.parent().map(|dir| dir.to_path_buf()).unwrap_or(cwd);
                (cwd, manifest_path)
            }
            Some(cargo_manifest_path) => {
                let Some(project_dir) = cargo_manifest_path.parent() else {
                    return Err(Report::msg(
                        "unable to locate project manifest: --manifest-path specifies a path with \
                         no parent",
                    ));
                };
                let manifest_path = project_dir.join("miden-project.toml");
                let project_dir = project_dir.to_path_buf();
                *cargo_manifest_path = manifest_path.clone();
                (project_dir, manifest_path)
            }
            None => {
                let Ok(cwd) = std::env::current_dir() else {
                    return Err(Report::msg(
                        "unable to locate project manifest: current working directory is \
                         unavailable",
                    ));
                };
                let manifest_path = cwd.join("miden-project.toml");
                compiler_opts.manifest_path = Some(manifest_path.clone());
                (cwd, manifest_path)
            }
        };

        let source_manager =
            Arc::new(DefaultSourceManager::default()) as Arc<dyn SourceManager + Send + Sync>;
        let outputs = if compiler_opts.workspace {
            let source = source_manager.load_file(&project_manifest_path).into_diagnostic()?;
            let workspace = miden_project::Workspace::load(source, &source_manager)?;
            build_workspace(
                &workspace,
                project_dir,
                compiler_opts,
                &cargo_opts,
                filesystem_cache_dir,
                source_manager,
            )?
        } else {
            // Check if the project manifest is a workspace manifest - this requires us to build
            // the entire workspace, rather than a single project. However, we only support this
            // if `--package` was given, as otherwise there is no way for us to select a package
            // to build
            let source = source_manager.load_file(&project_manifest_path).into_diagnostic()?;
            if let miden_project::ast::MidenProject::Workspace(_) =
                miden_project::ast::MidenProject::parse(source.clone())?
            {
                if compiler_opts.packages.is_empty() {
                    return Err(Report::msg(
                        "a workspace manifest was provided, but --workspace was not specified",
                    ));
                }
                let workspace = miden_project::Workspace::load(source, &source_manager)
                    .map(Arc::<miden_project::Workspace>::from)?;
                let mut outputs = Vec::new();
                for requested in compiler_opts.packages.iter() {
                    let Some(package) = workspace.get_member_by_name(requested) else {
                        return Err(Report::msg(format!(
                            "requested pacakge '{requested}' is not a valid workspace member"
                        )));
                    };
                    let compiler_opts = compiler_opts.clone();
                    let project = miden_project::Project::WorkspacePackage {
                        package,
                        workspace: workspace.clone(),
                    };
                    // The member came off the workspace already loaded, so its facts are taken
                    // from it rather than read from a file it never had of its own.
                    reject_unbuildable_target(
                        &ProjectManifest::from_package(&project.package()),
                        &compiler_opts,
                    )?;
                    let output = build_project(
                        project,
                        &compiler_opts,
                        &cargo_opts,
                        filesystem_cache_dir,
                        Arc::clone(&source_manager),
                    )?;
                    outputs.push(output);
                }
                outputs
            } else {
                // Loaded rather than AST-parsed: this is also where a malformed manifest of a
                // *dependency* is first reported, since a dependency build reaches `cargo` through
                // here without going through `prepare_project`. The facts below are then taken off
                // the package it produced, so nothing is read twice.
                let project =
                    miden_project::Project::load(&project_manifest_path, &source_manager)?;
                let package = project.package();
                let package_name = package.name().into_inner();
                if compiler_opts.packages.len() > 1 {
                    return Err(Report::msg(format!(
                        "multiple packages were requested via --package, but the project manifest \
                         only defines a single package ({package_name})"
                    )));
                } else if !compiler_opts.packages.is_empty()
                    && !compiler_opts.packages.iter().any(|p| &*package_name == p)
                {
                    return Err(Report::msg(format!(
                        "the provided project manifest defines a package ({}) that differs from \
                         the one requested via --package ({package_name})",
                        &compiler_opts.packages[0]
                    )));
                }
                reject_unbuildable_target(
                    &ProjectManifest::from_package(&package),
                    &compiler_opts,
                )?;
                let output = build_project(
                    project,
                    &compiler_opts,
                    &cargo_opts,
                    filesystem_cache_dir,
                    source_manager,
                )?;
                vec![output]
            }
        };

        Ok(outputs)
    }

    fn build_workspace(
        workspace: &miden_project::Workspace,
        _cwd: PathBuf,
        _compiler_opts: Box<midenc_session::Options>,
        _cargo_opts: &CargoOptions,
        _filesystem_cache_dir: Option<&std::path::Path>,
        _source_manager: Arc<dyn SourceManager>,
    ) -> CompilerResult<Vec<InputFile>> {
        //let metadata = load_metadata(cargo_opts.manifest_path.as_deref())?;

        //let mut packages =
        //   load_component_metadata(&metadata, cargo_opts.packages.iter(), cargo_opts.workspace)?;

        if workspace.members().is_empty() {
            return Err(Report::msg(format!(
                "workspace ({}) contains no members",
                workspace.manifest_path().unwrap_or(Path::new("virtual")).display()
            )));
        }

        todo!("build a dependency graph of the workspace members and build each package")
    }

    fn build_project(
        _project: miden_project::Project,
        compiler_opts: &midenc_session::Options,
        cargo_opts: &CargoOptions,
        filesystem_cache_dir: Option<&std::path::Path>,
        _source_manager: Arc<dyn SourceManager + Send + Sync>,
    ) -> CompilerResult<InputFile> {
        /*
        let tmp = tempfile::TempDir::new()
            .map_err(|err| Report::msg(format!("could not create temporary directory: {err}")))?;
        let mut default_registry =
            midenc_session::registry::HybridPackageRegistry::new(compiler_opts)?;
        let registry = registry.unwrap_or(&mut default_registry);
        let package = project.package();
        let dependency_graph = miden_project::ProjectDependencyGraphBuilder::new(&*registry)
            .with_source_manager(source_manager.clone())
            .with_git_cache_root(
                compiler_opts
                    .midenup_home
                    .as_deref()
                    .unwrap_or(tmp.path())
                    .join("git")
                    .join("checkouts"),
            );

        let dependency_graph = dependency_graph.build(package.clone())?;
        crate::cargo::load_cargo_based_source_dependencies(
            &package,
            &dependency_graph,
            registry,
            compiler_opts,
            cargo_opts,
            source_manager,
        )?;
         */

        let rustup_toolchain = crate::rust::rustup_toolchain();
        let cargo_build_args = build_cargo_args(cargo_opts, compiler_opts.optimize);

        let mandatory_rust_flags = [
            // Enable memcopy and 128-bit arithmetic ops
            "-C",
            "target-feature=+bulk-memory,+wide-arithmetic",
            // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it
            // for cfg-based dependency selection.
            "--cfg",
            "miden",
            // Enable errors on missing stub functions
            "-C",
            "link-args=--fatal-warnings",
            // Remove the source file paths in the data segment for panics
            // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
            "-Zlocation-detail=none",
            // Build with panic=immediate-abort
            "-Zunstable-options",
            "-Cpanic=immediate-abort",
        ];
        let inherited_encoded = std::env::var_os("CARGO_ENCODED_RUSTFLAGS");
        let inherited_plain = std::env::var_os("RUSTFLAGS");
        let extra_rust_flags = merge_rust_flags(
            &mandatory_rust_flags,
            inherited_encoded.as_deref(),
            inherited_plain.as_deref(),
            compiler_opts.rustflags.as_deref(),
        );

        let wasi = if compiler_opts.target_requires_protocol() {
            "wasip2"
        } else {
            "wasip1"
        };

        let env = cargo_env(filesystem_cache_dir, &extra_rust_flags);
        let mut wasm_outputs =
            run_cargo(wasi, rustup_toolchain.as_deref(), &cargo_build_args, env)?;

        assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
        let wasm_output = wasm_outputs.pop().expect("expected at least one Wasm artifact");

        Ok(InputFile::from_path(wasm_output).unwrap())
    }

    /// The environment the nested `cargo` is spawned with.
    ///
    /// `MIDENC_PACKAGE_CACHE` is the whole reason `filesystem_cache_dir` is threaded down from
    /// the entry point: it tells the nested `midenc` invocations where to publish the packages
    /// they compile and where to look for the ones their own dependencies already produced. The
    /// directory is fingerprinted by the root build's compiler, options, and manifest closure so
    /// every participant in that build uses the same isolated cache. Absent a cache directory the
    /// variable is *unset* rather than set to an empty path, which is what makes the nested build
    /// fall back to its own default.
    ///
    /// The composed rust flags are emitted twice: as `RUSTFLAGS` (space-joined, lossy for
    /// arguments that contain spaces) and, authoritatively, as `CARGO_ENCODED_RUSTFLAGS`
    /// (0x1f-joined). Cargo prefers the encoded variable, so emitting it explicitly is what keeps
    /// the mandatory Miden flags — `--cfg miden`, the target features, the panic strategy — from
    /// being replaced by an inherited value; the caller's own flags survive because
    /// [`merge_rust_flags`] folds them into the composed list first.
    ///
    /// Named — rather than left inline where it was — so that this can be asserted without
    /// spawning `cargo -Z build-std` against the SDK.
    pub(super) fn cargo_env(
        filesystem_cache_dir: Option<&Path>,
        extra_rust_flags: &[String],
    ) -> Vec<(&'static str, String)> {
        let mut env = vec![
            ("RUSTFLAGS", extra_rust_flags.join(" ")),
            ("CARGO_ENCODED_RUSTFLAGS", extra_rust_flags.join("\x1f")),
        ];
        if let Some(filesystem_cache_dir) = filesystem_cache_dir {
            env.push(("MIDENC_PACKAGE_CACHE", filesystem_cache_dir.to_string_lossy().into_owned()));
        }
        env
    }

    /// Composes the rustc flag list for the nested cargo build.
    ///
    /// Inherited flags follow cargo's own precedence: a non-empty `CARGO_ENCODED_RUSTFLAGS`
    /// (0x1f-separated; arguments may contain spaces) replaces plain `RUSTFLAGS`
    /// (whitespace-split). Inherited flags are folded in after the mandatory set and before the
    /// explicit `--rustflags`, preserving the pre-existing override order — nothing a caller
    /// passes is dropped.
    pub(super) fn merge_rust_flags(
        mandatory: &[&str],
        inherited_encoded: Option<&std::ffi::OsStr>,
        inherited_plain: Option<&std::ffi::OsStr>,
        explicit: Option<&str>,
    ) -> Vec<String> {
        let mut args: Vec<String> = mandatory.iter().map(|flag| flag.to_string()).collect();
        let encoded = inherited_encoded
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty());
        let plain = inherited_plain
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty());
        if let Some(encoded) = encoded {
            args.extend(encoded.split('\x1f').filter(|arg| !arg.is_empty()).map(str::to_string));
        } else if let Some(plain) = plain {
            args.extend(plain.split_whitespace().map(str::to_string));
        }
        if let Some(explicit) = explicit {
            args.extend(explicit.split_whitespace().map(str::to_string));
        }
        args
    }

    /// Returns the Cargo profile value for a compiler optimization level.
    fn cargo_profile_opt_level(opt_level: OptLevel) -> &'static str {
        match opt_level {
            OptLevel::None | OptLevel::Balanced => "2",
            OptLevel::Basic => "1",
            OptLevel::Max => "3",
            OptLevel::Size => "\"s\"",
            OptLevel::SizeMin => "\"z\"",
        }
    }

    /// Builds the argument vector for the underlying `cargo build` invocation.
    fn build_cargo_args(cargo_opts: &CargoOptions, opt_level: OptLevel) -> Vec<String> {
        let mut args = vec!["build".to_string()];

        // Add build-std flags required for Miden compilation
        args.extend(
            [
                "-Z",
                "build-std=core,alloc,panic_abort",
                "-Z",
                "build-std-features=optimize_for_size",
            ]
            .into_iter()
            .map(|s| s.to_string()),
        );

        // Configure profile settings
        let cfg_pairs: Vec<(&str, &str)> = vec![
            ("profile.dev.panic", "\"abort\""),
            ("profile.dev.opt-level", "1"),
            ("profile.dev.overflow-checks", "false"),
            ("profile.dev.debug", "true"),
            ("profile.dev.debug-assertions", "false"),
            ("profile.release.opt-level", cargo_profile_opt_level(opt_level)),
            ("profile.release.lto", "true"),
            ("profile.release.codegen-units", "1"),
            ("profile.release.panic", "\"abort\""),
        ];

        for (key, value) in cfg_pairs {
            args.push("--config".to_string());
            args.push(format!("{key}={value}"));
        }

        // Forward cargo-specific options
        if cargo_opts.release {
            args.push("--release".to_string());
        }

        if let Some(ref manifest_path) = cargo_opts.manifest_path {
            args.push("--manifest-path".to_string());
            args.push(manifest_path.to_string_lossy().to_string());
        }

        if cargo_opts.workspace {
            args.push("--workspace".to_string());
        }

        for package in &cargo_opts.packages {
            args.push("--package".to_string());
            args.push(package.to_string());
        }

        args
    }

    fn run_cargo<E>(
        wasi: &str,
        toolchain: Option<&str>,
        spawn_args: &[String],
        env: E,
    ) -> CompilerResult<Vec<PathBuf>>
    where
        E: IntoIterator<Item = (&'static str, String)>,
    {
        let cargo_env = std::env::var("CARGO").map(PathBuf::from).ok();
        let cargo_path = cargo_env.as_deref().unwrap_or_else(|| Path::new("cargo"));

        let mut cargo = std::process::Command::new(cargo_path);
        cargo.envs(env);
        if cargo_env.is_none() {
            cargo.arg(format!("+{}", toolchain.unwrap_or("nightly")));
        }

        // This env var is used by crates (e.g. `miden-field`) to distinguish compiling to Wasm
        // for a "real" Wasm runtime vs compiling to Wasm as an intermediate artifact that
        // will be compiled to Miden VM code by `midenc`.
        cargo.env("MIDENC_TARGET_IS_MIDEN_VM", "1");
        cargo.args(spawn_args);

        // Handle the target for buildable commands
        crate::rust::install_wasm32_target(wasi, None)?;

        cargo.arg("--target").arg(format!("wasm32-{wasi}"));

        // It will output the message as json so we can extract the wasm files
        // that will be componentized
        cargo.arg("--message-format").arg("json-render-diagnostics");
        cargo.stdout(std::process::Stdio::piped());
        cargo.stderr(std::process::Stdio::inherit());

        let artifacts = crate::rust::spawn_cargo(cargo, cargo_path)?;

        let outputs: Vec<PathBuf> = artifacts
            .into_iter()
            .filter_map(|a| {
                let path: PathBuf = a.filenames.first().unwrap().clone().into();
                if path.to_str().unwrap().contains("wasm32-wasip") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        Ok(outputs)
    }

    /// Reject a project whose target this build cannot produce, before `cargo` is spawned for it.
    ///
    /// Two rejections, and both are this build's own: a **kernel**, which the compiler does not
    /// support through this route, and an **executable that cannot be identified** — a package
    /// declaring several `[[bin]]`s with no `--target` to choose between them, or a `--target`
    /// naming one it does not declare. The second is [`ProjectManifest::selected_executable`],
    /// which is also what derives the entrypoint in `Session::new`, so a project rejected here is
    /// exactly a project no session could have named an entrypoint for.
    ///
    /// # This used to modify the options, and the modifications were dead
    ///
    /// It was `modify_midenc_options_for_target`, and it took `&mut Options`: it set
    /// `entrypoint` and pushed a `RemapPathPrefix` for the package's source directory. Neither
    /// write could be observed. The `Options` it received are a **clone** of the session's, made
    /// in `build_manifest_to_wasm` and owned by [`cargo_build`] — while `entrypoint` is read off
    /// `session.options` by codegen and `remap_path_prefixes` off `session.options` by the
    /// WebAssembly frontend, neither of which this clone reaches. Of the clone, [`build_project`]
    /// reads exactly three things: `optimize`, `rustflags`, and `target_requires_protocol()` for
    /// the WASI target. So the writes went into a value that was dropped.
    ///
    /// It also derived a target type into a local and never assigned it back, which is why
    /// removing the derivation changes nothing: the WASI choice reads `options.target_type` as it
    /// arrived. `Session::new` is what sets that, from the same manifest facts, and is now the
    /// only place either rule is written down.
    ///
    /// Taking `&Options` rather than `&mut Options` is what keeps it that way: a write that
    /// cannot be observed can no longer be added back without the type saying so.
    fn reject_unbuildable_target(
        manifest: &ProjectManifest,
        options: &midenc_session::Options,
    ) -> CompilerResult<()> {
        // `--target-type` if the caller gave one, and otherwise what the manifest declares — the
        // same order `Session::new` resolves it in.
        let target_type = options.target_type.unwrap_or_else(|| manifest.library_target_type());
        match target_type {
            TargetType::Executable => {
                // Selected only to be checked: which executable is *built* is decided by the
                // `cargo` invocation and by the target the assembler asks for, not here.
                manifest.selected_executable(options.target.as_deref())?;
                Ok(())
            }
            TargetType::Kernel => {
                Err(Report::msg("kernels are not currently supported via midenc"))
            }
            TargetType::Library
            | TargetType::AccountComponent
            | TargetType::Note
            | TargetType::TransactionScript => Ok(()),
            // `TargetType` is `#[non_exhaustive]`, so this arm is reachable by upgrading
            // `miden-project` alone. A target type this build has never been taught to produce is
            // refused rather than built as whatever the arms above would have made of it.
            unsupported => Err(Report::msg(format!("unsupported --target-type: {unsupported}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        string::{String, ToString},
        sync::Arc,
        vec,
        vec::Vec,
    };
    use core::cell::RefCell;

    use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};
    use midenc_codegen_masm::MasmComponent;
    use midenc_hir::Context;
    use midenc_session::{
        InputFile, Options, OutputFile, OutputType, OutputTypeSpec, OutputTypes,
        miden_project::TargetType,
    };

    use super::*;
    use crate::pipeline::{
        CheckpointId, FrontendId, FrontendRegistry, Goal, Observer, OutputRequest,
        RecordingObserver, RequestState, TargetRole,
        frontends::WASM_FRONTEND,
        resolve_goal,
        testing::{self, VirtualProject, wat_fixture},
    };

    /// The project route *is* the WebAssembly route, and this is what holds it to that.
    ///
    /// A route is what `--stop-after`, the `-C` stop flags and `--emit` are validated against,
    /// so it has to describe what the frontend actually publishes — and past the `cargo` build,
    /// a manifest-backed Rust root target is run by
    /// [`WasmFrontend::compile_wasm`](super::super::wasm::WasmFrontend) itself. So the
    /// declaration here is a hand copy of a table that lives in `wasm.rs`, and nothing ties the
    /// two together at compile time; this is what does, in both directions:
    ///
    /// - a checkpoint **added** to `WASM_FRONTEND` and to the shared tail would leave this route
    ///   under-declaring it, and `EmitObserver` would silently skip `--emit` for it.
    /// - a checkpoint **removed** from the tail would leave this route offering a stop point
    ///   nothing reaches — the driver's empty-capture error.
    ///
    /// Compared against `WASM_FRONTEND` itself rather than against literals, because a literal
    /// is just a third copy of the same table. The renderers are deliberately not compared; see
    /// [`the_standalone_route_is_the_wasm_route`].
    #[test]
    fn the_project_route_is_the_wasm_route() {
        assert_eq!(RUST_FRONTEND.id(), FrontendId::new("rust"));
        assert_eq!(
            RUST_FRONTEND.extensions(),
            &["rs"],
            "dispatch is by target-root extension, and a Rust target is rooted at a `.rs` file"
        );
        assert_eq!(
            RUST_FRONTEND.route(),
            WASM_FRONTEND.route(),
            "past the cargo build this route is run by the wasm frontend's own code, so it must \
             declare exactly what that code publishes"
        );
        assert_eq!(
            RUST_FRONTEND.aliases(),
            WASM_FRONTEND.aliases(),
            "and `--stop-after` must name the same points on both, since the same phases produce \
             them"
        );
        for checkpoint in WASM_FRONTEND.route() {
            assert_eq!(
                RUST_FRONTEND.artifact_at(*checkpoint),
                WASM_FRONTEND.artifact_at(*checkpoint),
                "the artifact published at {checkpoint} is produced by one piece of code, so both \
                 routes must declare it as the same thing"
            );
        }
        assert_eq!(RUST_FRONTEND.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
        assert_ne!(RUST_FRONTEND.id(), WASM_FRONTEND.id(), "they are still two registrations");
        FrontendRegistry::new()
            .register(RUST_FRONTEND)
            .expect("the route, its aliases and its artifacts must be mutually consistent");
    }

    /// Every stop point on the route resolves; a name that is on no route is still rejected.
    ///
    /// The first half is the whole of what this task bought a manifest-backed build:
    /// `--stop-after=lower` used to be rejected here, because a cargo build published nothing
    /// short of the package. The second half is the guard that the route did not simply become
    /// permissive — a name nothing declares must still fail, naming what the route does offer.
    #[test]
    fn every_stop_point_on_the_project_route_resolves_and_an_unknown_one_does_not() {
        let goal = resolve_goal(&OutputRequest::default(), &RUST_FRONTEND)
            .expect("an uncapped run resolves to the terminal checkpoint");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);

        for (alias, checkpoint) in RUST_FRONTEND.aliases() {
            let capped = OutputRequest::new(Vec::new()).with_stop_after(Some(alias.to_string()));
            let goal = resolve_goal(&capped, &RUST_FRONTEND)
                .unwrap_or_else(|err| panic!("`--stop-after={alias}` must resolve: {err}"));
            assert_eq!(goal.checkpoint(), *checkpoint);
        }

        let capped = OutputRequest::new(Vec::new()).with_stop_after(Some("nonesuch".to_string()));
        let err = resolve_goal(&capped, &RUST_FRONTEND)
            .expect_err("no route declares a `nonesuch` checkpoint");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("lower") && rendered.contains("masm.lowered"),
            "the rejection must name the stop points this route does offer: {rendered}"
        );
    }

    /// The registration builds *this* frontend, not merely some frontend.
    ///
    /// `Rc<dyn Frontend>` is opaque, so the instance is identified by behaviour: only
    /// [`RustProjectFrontend::post_process`] reports a cache miss as a missing cargo build.
    #[test]
    fn instantiate_builds_a_rust_project_frontend() {
        let project = library("rust_frontend_instantiate");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = target_context(&assembly, context.clone(), &state);

        let frontend = RUST_FRONTEND.instantiate(context.session_rc());
        let err = frontend
            .post_process(&mut any_package(), &cx)
            .expect_err("nothing was built for this target");
        assert!(
            format!("{err}").contains("no cargo build was cached for it"),
            "only the Rust frontend reports a miss this way, got: {err}"
        );
    }

    fn library(name: &str) -> VirtualProject {
        let root = wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// A project named `name` with a single target of type `ty`.
    fn project(name: &str, fixture: &str, ty: TargetType) -> VirtualProject {
        let root = wat_fixture(
            fixture,
            if ty.is_executable() {
                "main.wat"
            } else {
                "lib.wat"
            },
        );
        VirtualProject::new(name, &root, ty).expect("should build")
    }

    /// A default HIR context, which is also the source of a target's session.
    fn context() -> Rc<Context> {
        Rc::new(Context::default())
    }

    /// The state of a full build: no observers, and a capture slot nothing reaches. These
    /// tests exercise `post_process`, which publishes no checkpoints.
    fn request() -> RequestState {
        RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new())
    }

    /// Build a target context for `assembly` within `state`.
    fn target_context<'a>(
        assembly: &'a miden_assembly::TargetAssemblyContext<'a>,
        context: Rc<Context>,
        state: &'a RequestState,
    ) -> TargetContext<'a> {
        TargetContext::for_testing(assembly, context, TargetRole::Root, state)
    }

    /// The `[lib]`/`[[bin]]` collision the old `(PackageId, Version)` key could not express.
    ///
    /// One package, two targets: the old provider keyed its cache on the package name and
    /// version, which are identical for both, so the executable read the library's codegen
    /// output. Asserted through the frontend rather than against a hand-built map: a
    /// frontend seeded with the *library* target's build must not serve that build to the
    /// *executable* target, and must say so by naming the target it could not find.
    #[test]
    fn a_library_build_is_not_served_to_the_executable_target() {
        let lib = project("shared", "rust_frontend_key_lib", TargetType::Library);
        let exe = project("shared", "rust_frontend_key_exe", TargetType::Executable);
        let lib_assembly = lib.assembly_context().expect("lib assembly context");
        let exe_assembly = exe.assembly_context().expect("exe assembly context");

        let context = context();
        let state = request();
        let lib_cx = target_context(&lib_assembly, context.clone(), &state);
        let exe_cx = target_context(&exe_assembly, context.clone(), &state);
        assert_eq!(
            lib_cx.target_key().package(),
            exe_cx.target_key().package(),
            "the two targets must belong to one package, or this proves nothing"
        );

        // Exactly the state of an in-process run that has built the library and is now
        // assembling the executable of the same package.
        let frontend = RustProjectFrontend::seeded(
            context.session_rc(),
            lib_cx.target_key(),
            codegen_output(),
        );

        frontend
            .post_process(&mut any_package(), &lib_cx)
            .expect("the seeded library target must be served from the cache");

        let err = frontend
            .post_process(&mut any_package(), &exe_cx)
            .expect_err("the executable must not read the library's codegen output");
        let msg = format!("{err}");
        assert!(msg.contains("internal error"), "a cache miss here is a compiler bug: {msg}");
        assert!(
            msg.contains(&**exe_cx.target_key().name()),
            "the miss must name the executable target it looked for, got: {msg}"
        );
    }

    /// The frontend distinguishes targets in what it reports, not just in what it stores.
    ///
    /// Two contexts for one package over an empty cache: both miss, and each miss must name
    /// its own target. A package-keyed frontend would produce one message twice.
    #[test]
    fn a_cache_miss_names_the_target_it_looked_for() {
        let lib = project("shared", "rust_frontend_miss_lib", TargetType::Library);
        let exe = project("shared", "rust_frontend_miss_exe", TargetType::Executable);
        let lib_assembly = lib.assembly_context().expect("lib assembly context");
        let exe_assembly = exe.assembly_context().expect("exe assembly context");

        let context = context();
        let state = request();
        let lib_cx = target_context(&lib_assembly, context.clone(), &state);
        let exe_cx = target_context(&exe_assembly, context.clone(), &state);

        let frontend = RustProjectFrontend::new(context.session_rc());
        let lib_err = format!(
            "{}",
            frontend
                .post_process(&mut any_package(), &lib_cx)
                .expect_err("nothing was compiled for the library target")
        );
        let exe_err = format!(
            "{}",
            frontend
                .post_process(&mut any_package(), &exe_cx)
                .expect_err("nothing was compiled for the executable target")
        );

        assert_ne!(
            lib_err, exe_err,
            "two targets of one package must not be reported as the same target"
        );
        assert!(
            lib_err.contains(&**lib_cx.target_key().name()),
            "the library's miss must name the library target: {lib_err}"
        );
        assert!(
            exe_err.contains(&**exe_cx.target_key().name()),
            "the executable's miss must name the executable target: {exe_err}"
        );
    }

    /// A seeded frontend serves `post_process` from the seed rather than reporting a miss.
    ///
    /// This is the virtual-project path: the running process already produced this target's
    /// [`CodegenOutput`], so seeding it must spare the nested cargo build entirely.
    #[test]
    fn a_seeded_build_is_served_to_post_process() {
        let project = library("rust_frontend_seeded");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = target_context(&assembly, context.clone(), &state);

        let frontend =
            RustProjectFrontend::seeded(context.session_rc(), cx.target_key(), codegen_output());

        frontend
            .post_process(&mut any_package(), &cx)
            .expect("a seeded target must be post-processed from the seed, not reported missing");
    }

    /// A `post_process` with nothing cached is an internal invariant violation, and must be
    /// reported as one. The provider this frontend replaces indexed with `compiled[&key]`,
    /// which panics on a miss.
    #[test]
    fn post_process_without_a_cached_build_is_an_invariant_error() {
        let project = library("rust_frontend_post_process");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = target_context(&assembly, context.clone(), &state);

        let frontend = RustProjectFrontend::new(context.session_rc());
        let mut package = any_package();

        let err = frontend
            .post_process(&mut package, &cx)
            .expect_err("post-processing a target that was never compiled must fail, not panic");
        let msg = format!("{err}");
        assert!(msg.contains("internal error"), "a cache miss here is a compiler bug: {msg}");
    }

    /// A well-formed package to hand to `post_process`.
    ///
    /// A package must export at least one procedure to be constructible, so rather than
    /// hand-building a MAST forest this borrows the compiler's own intrinsics package.
    /// `post_process` must reject the cache miss before it looks at the package at all, so
    /// which package it is does not matter.
    fn any_package() -> MastPackage {
        (*midenc_codegen_masm::intrinsics::load()).clone()
    }

    /// A minimal build result to seed a frontend's cache with.
    ///
    /// `post_process` reads only the component's rodata and the account-component metadata,
    /// both empty here, so this stands in for a real cargo build without running one.
    fn codegen_output() -> CodegenOutput {
        let root = miden_assembly_syntax::Path::new("seeded")
            .to_absolute()
            .map(|path| Arc::from(path.into_owned()))
            .expect("should absolutize the root module path");
        CodegenOutput {
            component: Arc::new(MasmComponent {
                id: None,
                synthetic_wrapper: false,
                root,
                init: None,
                entrypoint: None,
                rodata: Vec::new(),
                heap_base: 0,
                stack_pointer: None,
                modules: Vec::new(),
            }),
            sections: Default::default(),
            source_provenance: ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: std::path::PathBuf::from("seeded.wat").into_boxed_path(),
                    content: "(module)".into(),
                },
                support: Vec::new(),
            },
        }
    }

    // -------------------------------------------------------------------------------------
    // The standalone entry point: one `.rs` file to WebAssembly.
    // -------------------------------------------------------------------------------------

    /// A Rust source `rustc` can compile straight to a WebAssembly `cdylib`.
    ///
    /// `#![no_std]` and a panic handler of its own, because the entry point builds with no
    /// sysroot crates beyond `core`: this is the shape every standalone `.rs` fixture in
    /// `tests/lit/debug` is written in. The exported `add` is what makes a compiled module
    /// distinguishable from an empty one.
    const BARE_SOURCE: &str = r#"
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn add(a: u32, b: u32) -> u32 {
    a + b
}
"#;

    /// The same source, preceded by `cargo -Zscript`-style frontmatter.
    ///
    /// The dependency is declared by a *relative* path, which is what makes the frontmatter
    /// observably parsed: `parse_cargo_frontmatter` absolutizes such a path against the
    /// directory the source was read from, so the table that comes back names a directory no
    /// literal in this file spells.
    const FRONTMATTER_SOURCE: &str = r#"//! ```cargo
//! [dependencies]
//! example = { path = "vendor/example" }
//! ```
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn add(a: u32, b: u32) -> u32 {
    a + b
}
"#;

    /// A context whose session is named `name` and whose build artifacts land in `target_dir`.
    ///
    /// Both are set explicitly because both are read by the entry point: the artifact name
    /// decides the `--crate-name` and the output file's stem, and `target_dir` is where that
    /// output file is written. `Session`'s own derivation for the empty input this builds on
    /// would give the *current directory's* base name and the workspace's `target/`, neither
    /// of which a test can assert against or is entitled to write into.
    fn rust_context(
        name: &str,
        target_dir: &std::path::Path,
        configure: impl FnOnce(&mut Options),
    ) -> Rc<Context> {
        testing::context_emitting(Default::default(), |options| {
            options.name = Some(name.to_string());
            options.target_dir = target_dir.to_path_buf();
            configure(options);
        })
    }

    /// The compiler input naming the `.rs` file at `path`.
    fn rust_input(path: &std::path::Path) -> InputFile {
        InputFile::from_path(path).expect("a `.rs` file is a valid compiler input")
    }

    /// A `.rs` fixture holding `source`, and the directory it was written to.
    fn rust_fixture(dir: &str, source: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let path = testing::fixture_source(dir, "lib.rs", source);
        let parent = path.parent().expect("a fixture is written into a directory").to_path_buf();
        (path, parent)
    }

    /// A bare `.rs` file is compiled to WebAssembly by the public entry point.
    ///
    /// The assertion goes as far as the *content* of the module, not merely its magic number:
    /// `rustc` writing an empty module would still produce four valid bytes, so the fixture's
    /// own export is what says the source was compiled rather than a placeholder emitted.
    #[test]
    fn a_bare_rust_file_compiles_to_wasm_through_the_entry_point() {
        let (source, _) = rust_fixture("rust_entry_point_bare", BARE_SOURCE);
        let target_dir = testing::fixture_dir("rust_entry_point_bare_out");
        let context = rust_context("rust_entry_point_bare", &target_dir, |_| {});

        let wasm = compile_rust_to_wasm(&rust_input(&source), context.session())
            .expect("rustc should compile the fixture to WebAssembly");

        assert_eq!(
            wasm,
            target_dir.join("rust_entry_point_bare.wasm"),
            "the module is written into the session's target directory, under the artifact name"
        );
        let bytes = std::fs::read(&wasm).expect("the produced module should be readable");
        assert_eq!(&bytes[..4], b"\0asm", "the entry point must produce a WebAssembly binary");
        let wat = midenc_frontend_wasm::wasm_to_wat(&bytes).expect("the module should disassemble");
        assert!(
            wat.contains(r#"(export "add""#),
            "and it must be the fixture's own module, not an empty one: {wat}"
        );
    }

    /// Cargo frontmatter sends a `.rs` source through a temporary Cargo project instead.
    ///
    /// Asserted on the selection rather than on a completed build: the Cargo route runs
    /// `cargo build -Z build-std` against the Miden SDK, which is minutes of work and a network
    /// fetch, and is exercised end to end by the lit suite. What is decidable here is *which*
    /// route was chosen and what it was given, which is the whole of what this task changed.
    #[test]
    fn cargo_frontmatter_takes_the_temporary_cargo_project_route() {
        let (source, dir) = rust_fixture("rust_entry_point_frontmatter", FRONTMATTER_SOURCE);
        let target_dir = testing::fixture_dir("rust_entry_point_frontmatter_out");
        let context = rust_context("rust_entry_point_frontmatter", &target_dir, |options| {
            options.cargo_frontmatter = true;
        });

        let build = RustBuild::select(&rust_input(&source), context.session())
            .expect("the frontmatter should parse");

        let RustBuild::CargoProject { dependencies } = build else {
            panic!("a source declaring dependencies cannot be built by `rustc` alone: {build:?}");
        };
        let dependencies = dependencies.expect("the frontmatter's own dependency table");
        assert!(
            dependencies.contains_key("example"),
            "the dependency the frontmatter declares must reach the Cargo project: {dependencies}"
        );
        assert!(
            dependencies
                .to_string()
                .contains(&dir.join("vendor/example").display().to_string()),
            "and its relative path must be resolved against the source's own directory: \
             {dependencies}"
        );
    }

    /// The two halves that keep the test above from passing for the wrong reason.
    ///
    /// The same source with frontmatter parsing switched off, and a source with no frontmatter
    /// at all, must both go to `rustc` — otherwise "takes the Cargo route" would be what every
    /// `.rs` input does.
    #[test]
    fn a_source_declaring_no_dependencies_is_compiled_by_rustc() {
        let (frontmatter, _) = rust_fixture("rust_entry_point_route_off", FRONTMATTER_SOURCE);
        let (bare, _) = rust_fixture("rust_entry_point_route_bare", BARE_SOURCE);
        let target_dir = testing::fixture_dir("rust_entry_point_route_out");

        let ignored = rust_context("rust_entry_point_route_off", &target_dir, |_| {});
        assert!(
            matches!(
                RustBuild::select(&rust_input(&frontmatter), ignored.session()),
                Ok(RustBuild::Rustc)
            ),
            "frontmatter is only looked for when `--cargo-frontmatter` asked for it"
        );

        let looking = rust_context("rust_entry_point_route_bare", &target_dir, |options| {
            options.cargo_frontmatter = true;
        });
        assert!(
            matches!(
                RustBuild::select(&rust_input(&bare), looking.session()),
                Ok(RustBuild::Rustc)
            ),
            "a source that declares no dependencies has nothing a Cargo project would add"
        );
    }

    /// A target type that requires the Miden protocol takes the Cargo route without frontmatter.
    ///
    /// The second of the two reasons a standalone build needs Cargo: the protocol SDK is a
    /// crate dependency, and `rustc` alone cannot resolve one.
    #[test]
    fn a_protocol_target_takes_the_cargo_route_without_frontmatter() {
        let (source, _) = rust_fixture("rust_entry_point_protocol", BARE_SOURCE);
        let target_dir = testing::fixture_dir("rust_entry_point_protocol_out");
        let context = rust_context("rust_entry_point_protocol", &target_dir, |options| {
            options.target_type = Some(TargetType::AccountComponent);
        });

        assert!(
            matches!(
                RustBuild::select(&rust_input(&source), context.session()),
                Ok(RustBuild::CargoProject { dependencies: None })
            ),
            "an account component links the protocol SDK, which only Cargo can supply"
        );
    }

    // -------------------------------------------------------------------------------------
    // The two routes.
    // -------------------------------------------------------------------------------------

    /// The checkpoints a `.rs` build publishes for itself, in order, on either route.
    ///
    /// `package.assembled` — the sixth on the route — is absent because the orchestrator
    /// publishes it, as it does for every frontend.
    fn rust_trace() -> Vec<CheckpointId> {
        vec![
            CheckpointId::WASM_PARSED,
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::HIR_TRANSFORMED,
            CheckpointId::MASM_LOWERED,
        ]
    }

    /// A standalone `.rs` target reaches every checkpoint its route declares.
    #[test]
    fn a_standalone_rust_target_publishes_its_whole_route() {
        let (source, _) = rust_fixture("rust_standalone_route", BARE_SOURCE);
        let target_dir = testing::fixture_dir("rust_standalone_route_out");
        let context = rust_context("rust_standalone_route", &target_dir, |_| {});

        let project = VirtualProject::new("rust_standalone_route", &source, TargetType::Library)
            .expect("should build project");
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let flow = RUST_STANDALONE_FRONTEND
            .instantiate(cx.session())
            .compile(&cx)
            .expect("the standalone frontend should compile a bare `.rs` file");

        assert!(!flow.is_break(), "package.assembled is the orchestrator's to publish, not ours");
        assert_eq!(
            observer.borrow().records().iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            rust_trace(),
            "the frontend publishes the WebAssembly it built and the HIR it translated, and the \
             shared backend the three that follow"
        );
        assert_eq!(
            RUST_STANDALONE_FRONTEND.route().len(),
            rust_trace().len() + 1,
            "and the route declares exactly those, plus the terminal checkpoint the driver \
             publishes"
        );
    }

    // -------------------------------------------------------------------------------------
    // The manifest-backed route's two arms.
    //
    // These drive `RustProjectFrontend` with the `cargo` build already done — its result
    // planted in the frontend's own memo — because what is under test is everything *past*
    // the build, and a real `cargo build -Z build-std` against the SDK is minutes of work and
    // a network fetch. The two integration gates
    // (`end_to_end::examples::is_prime` and `sdk::rust_sdk_account_package_build_is_deterministic`)
    // exercise the build itself.
    // -------------------------------------------------------------------------------------

    /// The WebAssembly a `cargo` build of this fixture would have produced.
    fn built_wasm() -> WasmSource {
        let wasm = wat::parse_str(MANIFEST_WAT).expect("the fixture should assemble to wasm");
        WasmSource::new(std::path::PathBuf::from("rust_project_root.wasm").into_boxed_path(), wasm)
    }

    /// A frontend for which `cargo` has already run for the target `key` names.
    ///
    /// Planted into `built` — the memo `built_for_root` fills — rather than into `compiled`:
    /// seeding a [`CodegenOutput`] would short-circuit `compile` before the role branch, which
    /// is precisely the branch these tests are about.
    fn already_built(session: Rc<Session>, key: TargetKey) -> RustProjectFrontend {
        let frontend = RustProjectFrontend::new(session);
        frontend.built.borrow_mut().insert(key, built_wasm());
        frontend
    }

    /// A manifest-backed Rust **root** target publishes the whole of its route.
    ///
    /// This is the defect this task closes. The route was `[package.assembled]` alone, because
    /// the root's build ran through `compile_manifest` — which has no [`TargetContext`] and
    /// hands back a finished [`CodegenOutput`] — so the intermediate artifacts existed and were
    /// never published. The root arm now runs the same shared WebAssembly tail a standalone
    /// `.rs` build runs, and publishes exactly what that tail publishes.
    #[test]
    fn a_manifest_backed_root_target_publishes_the_whole_route() {
        let project = library("rust_project_root_publishes");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let frontend = already_built(context.session_rc(), cx.target_key());
        let flow = frontend.compile(&cx).expect("the root arm should lower the built WebAssembly");

        assert!(!flow.is_break(), "package.assembled is the orchestrator's to publish, not ours");
        assert_eq!(
            observer.borrow().records().iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            rust_trace(),
            "a manifest-backed root target publishes what the shared tail publishes"
        );
        assert_eq!(
            RUST_FRONTEND.route().len(),
            rust_trace().len() + 1,
            "and the route declares exactly those, plus the terminal checkpoint the driver \
             publishes"
        );
    }

    /// A root target stops at the goal, at every checkpoint its route offers short of the
    /// package.
    ///
    /// Publishing is not enough on its own: the gate a `--stop-after` or `-C` flag needs is that
    /// the stop reaches `provide_sources_interruptible`, which is what [`Frontend::compile`]
    /// returning [`Flow::Break`] does. Each checkpoint is driven on its own, because a route that
    /// stopped only at its first would satisfy a single-goal test.
    #[test]
    fn a_manifest_backed_root_target_stops_at_each_goal_on_its_route() {
        for goal in rust_trace() {
            let project = library("rust_project_root_stops");
            let assembly = project.assembly_context().expect("assembly context");
            let context = context();
            let observer = Rc::new(RefCell::new(RecordingObserver::default()));
            let state = RequestState::new(
                Goal::at(goal),
                vec![observer.clone() as Rc<RefCell<dyn Observer>>],
            );
            let cx =
                TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

            let frontend = already_built(context.session_rc(), cx.target_key());
            let flow = frontend.compile(&cx).expect("the root arm should reach the goal");

            assert!(flow.is_break(), "the run must stop at its goal '{goal}'");
            let published = observer.borrow().records().iter().map(|(c, _)| *c).collect::<Vec<_>>();
            assert_eq!(
                published.last(),
                Some(&goal),
                "no work may happen past the stop point, so '{goal}' must be the last \
                 publication: {published:?}"
            );
            let captured = state.take_outcome().expect("the goal must have been captured");
            assert_eq!(captured.checkpoint(), goal);
        }
    }

    /// A **dependency** target publishes nothing, and does not take the root's built
    /// WebAssembly.
    ///
    /// The discriminating half of the two tests above, and the one that keeps the root arm from
    /// quietly becoming every arm. The same frontend, with the same `cargo` result already in
    /// hand, driven with the dependency role: it must ignore that result — a dependency is built
    /// by `crate::cargo::cargo_build` against a `Session` of its own — and publish nothing on
    /// the way. The build then fails, because this fixture's virtual project has no manifest for
    /// a nested build to read, and that failure *is* the evidence the other arm was taken.
    #[test]
    fn a_dependency_target_publishes_nothing_and_ignores_the_roots_build() {
        let project = library("rust_project_dependency");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx =
            TargetContext::for_testing(&assembly, context.clone(), TargetRole::Dependency, &state);

        let frontend = already_built(context.session_rc(), cx.target_key());
        // `ProjectSourceInputs` is not `Debug`, so `expect_err` is unavailable here.
        let Err(err) = frontend.compile(&cx) else {
            panic!(
                "a dependency must not be served the root's WebAssembly: it is built by a nested \
                 cargo invocation, which this fixture has no manifest for"
            );
        };

        assert!(
            observer.borrow().records().is_empty(),
            "a dependency's checkpoints must never reach this request's observers: {:?}",
            observer.borrow().records()
        );
        assert!(
            state.take_outcome().is_none(),
            "and a dependency must never capture the request's artifact, whatever its goal"
        );
        let rendered = format!("{err}");
        assert!(
            !rendered.contains("wasm.parsed") && !rendered.contains("hir."),
            "the failure must be the nested build's, not a stop this arm cannot express: \
             {rendered}"
        );
    }

    /// A second `compile` for one root target is served from what the first lowered.
    ///
    /// The arrangement this replaced cached every arm's [`CodegenOutput`] in one map, so a second
    /// callback for a target was served for free. The root's lowering now lives in the embedded
    /// [`WasmFrontend`] instead, and a lookup that missed it would re-enter `compile_wasm`: that
    /// re-publishes every checkpoint on the route — writing `--emit=wat`, both `--emit=hir`
    /// documents and `--emit=masm` a second time — before failing on the capture slot's
    /// "already captured" invariant.
    ///
    /// Asserted through the observer rather than through the return value, because a re-lowering
    /// that happened to succeed would still return usable sources; what must not happen is the
    /// second round of publications.
    #[test]
    fn a_second_compile_of_one_root_target_is_served_rather_than_relowered() {
        let project = library("rust_project_root_second_compile");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let frontend = already_built(context.session_rc(), cx.target_key());
        let _ = frontend.compile(&cx).expect("the first callback lowers the built WebAssembly");
        let after_first = observer.borrow().records().len();
        assert_eq!(after_first, rust_trace().len(), "the first callback publishes the route");

        let _ = frontend.compile(&cx).expect("the second callback must be served, not rebuilt");
        assert_eq!(
            observer.borrow().records().len(),
            after_first,
            "a second callback for one target must publish nothing further: {:?}",
            observer.borrow().records()
        );
    }

    /// The root target is post-processed from what the shared tail lowered.
    ///
    /// The root's [`CodegenOutput`] is not in this frontend's own `compiled` map — the embedded
    /// [`WasmFrontend`] produced it and keeps it — so a `post_process` that consulted only
    /// `compiled` would report every root target as never compiled. The paired assertion is the
    /// discriminating one: the *same* frontend must still report a miss for a target nothing was
    /// built for, so this cannot be satisfied by dropping the check.
    #[test]
    fn a_root_target_is_post_processed_from_what_the_shared_tail_lowered() {
        let project = library("rust_project_root_post_process");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let frontend = already_built(context.session_rc(), cx.target_key());
        let err = frontend
            .post_process(&mut any_package(), &cx)
            .expect_err("nothing has been compiled for this target yet");
        assert!(
            format!("{err}").contains("no cargo build was cached for it"),
            "a miss must still be reported as one: {err}"
        );

        let _ = frontend.compile(&cx).expect("the root arm should lower the built WebAssembly");
        frontend
            .post_process(&mut any_package(), &cx)
            .expect("the root's lowering is the embedded wasm frontend's, and must be found there");
    }

    /// The root arm answers `provenance` without lowering anything, and without publishing.
    ///
    /// [`Frontend::provenance`] must not publish and must never stop, and the assembler reaches
    /// it while hashing the dependency closure — possibly *before* `compile*, and repeatedly. So
    /// the root arm goes only as far as the `cargo` build and describes what came out of it; a
    /// root arm that answered by lowering would publish `hir.transformed` from a callback whose
    /// caller cannot express a stop.
    #[test]
    fn the_root_arms_provenance_publishes_nothing() {
        let project = library("rust_project_root_provenance");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        // A goal short of the package, so that a publication from here would also *stop* — the
        // failure mode this exists to prevent, rather than merely a noisy observer.
        let state = RequestState::new(
            Goal::at(CheckpointId::HIR_TRANSFORMED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let frontend = already_built(context.session_rc(), cx.target_key());
        let provenance = frontend.provenance(&cx).expect("the root's provenance is its wasm");

        assert!(
            observer.borrow().records().is_empty(),
            "provenance must publish nothing: {:?}",
            observer.borrow().records()
        );
        assert!(state.take_outcome().is_none(), "and must never capture");
        assert_eq!(
            provenance.root.path.as_ref(),
            std::path::Path::new("rust_project_root.wasm"),
            "the root's provenance is the WebAssembly cargo produced, not the `.rs` target root"
        );
        assert!(
            provenance.root.content.contains("(module"),
            "and its content is that WebAssembly as text: {}",
            provenance.root.content
        );
    }

    /// `compile` and `provenance` share one `cargo` build, whichever the assembler reaches
    /// first.
    ///
    /// The memo is not an optimization here: the assembler asks for provenance repeatedly while
    /// hashing the dependency closure, and a `cargo` build per call would be minutes each. The
    /// planted entry is removed after the first read, so a second build attempt would fail
    /// rather than silently succeed.
    #[test]
    fn the_roots_cargo_build_is_shared_by_provenance_and_compile() {
        let project = library("rust_project_root_memo");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let frontend = already_built(context.session_rc(), cx.target_key());
        frontend.provenance(&cx).expect("the first callback pays for the build");
        assert!(
            frontend.built.borrow().contains_key(&cx.target_key()),
            "the build must be memoized for the callbacks still to come"
        );
        let _ = frontend
            .compile(&cx)
            .expect("the second callback must be served the same build, not run another");
    }

    /// `--stop-after=parse` resolves the same way on both Rust routes.
    ///
    /// It used to be a stop point on the standalone route and a usage error on the project one,
    /// because a cargo-driven build published nothing at `parse`. Both now run the same shared
    /// tail, so both stop at the WebAssembly they produced.
    #[test]
    fn stop_after_parse_resolves_the_same_way_on_both_rust_routes() {
        let request = OutputRequest::new(Vec::new()).with_stop_after(Some("parse".to_string()));

        for frontend in [RUST_STANDALONE_FRONTEND, RUST_FRONTEND] {
            let goal = resolve_goal(&request, &frontend).unwrap_or_else(|err| {
                panic!("a `.rs` build stops at the WebAssembly it produced: {err}")
            });
            assert_eq!(
                goal.checkpoint(),
                CheckpointId::WASM_PARSED,
                "on the '{}' route",
                frontend.id()
            );
        }
    }

    // -------------------------------------------------------------------------------------
    // The standalone registration.
    // -------------------------------------------------------------------------------------

    /// The standalone route *is* the WebAssembly route, and this is what holds it to that.
    ///
    /// The tables in [`RUST_STANDALONE_FRONTEND`] are a hand copy of
    /// [`WASM_FRONTEND`](super::super::wasm::WASM_FRONTEND)'s, but what this route *publishes*
    /// is not: `compile` delegates to `WasmFrontend::compile_wasm`, so the publications come
    /// from `wasm.rs` while the declarations are a separate literal over here. Nothing else
    /// ties the two together — [`TargetContext::checkpoint`] does not check a published
    /// checkpoint against the route that declared it — so divergence is silent, and silent in
    /// both directions:
    ///
    /// - a checkpoint **added** to `WASM_FRONTEND` and to the shared tail would leave this
    ///   route under-declaring it. `EmitObserver` finds no `decl_at` and skips it, so that
    ///   `--emit` simply never happens on a standalone `.rs` build, with no diagnostic.
    /// - a checkpoint **removed** from the tail would leave this route offering a stop point
    ///   nothing reaches — the empty-capture error this registration exists to avoid.
    ///
    /// Asserted against `WASM_FRONTEND` itself rather than against literals, because a literal
    /// is just a third copy of the same table.
    ///
    /// The artifact *ids* are compared per checkpoint; the renderers are not, and must not be.
    /// Each route declares its own `unrendered`, and comparing `fn` pointers is meaningless
    /// anyway — the compiler may merge two functions with identical bodies. What the tests
    /// above pin is that neither route's renderers write.
    #[test]
    fn the_standalone_route_is_the_wasm_route() {
        assert_eq!(
            RUST_STANDALONE_FRONTEND.route(),
            WASM_FRONTEND.route(),
            "past `wasm.parsed` this route is run by the wasm frontend's own code, so it must \
             declare exactly what that code publishes"
        );
        assert_eq!(
            RUST_STANDALONE_FRONTEND.aliases(),
            WASM_FRONTEND.aliases(),
            "and `--stop-after` must name the same points on both, since the same phases produce \
             them"
        );
        for checkpoint in WASM_FRONTEND.route() {
            assert_eq!(
                RUST_STANDALONE_FRONTEND.artifact_at(*checkpoint),
                WASM_FRONTEND.artifact_at(*checkpoint),
                "the artifact published at {checkpoint} is produced by one piece of code, so both \
                 routes must declare it as the same thing"
            );
        }

        // And the two are still distinct registrations, or the equalities above would be a
        // tautology about one value compared with itself.
        assert_ne!(RUST_STANDALONE_FRONTEND.id(), WASM_FRONTEND.id());
        assert_eq!(RUST_STANDALONE_FRONTEND.extensions(), &["rs"]);
        assert_eq!(WASM_FRONTEND.extensions(), &["wasm", "wat"]);
    }

    /// The standalone registration is well-formed, and is a *different* frontend.
    ///
    /// The aliases are checked here against literals, which
    /// [`the_standalone_route_is_the_wasm_route`] deliberately does not: that test pins the two
    /// routes *to each other*, and this one pins what a user types. Renaming an alias on both
    /// routes at once would satisfy that test and fail this one, which is the point.
    #[test]
    fn the_standalone_registration_is_accepted_by_the_registry() {
        let mut registry = FrontendRegistry::new();
        registry
            .register(RUST_STANDALONE_FRONTEND)
            .expect("the standalone registration must be well-formed");

        assert_ne!(
            RUST_STANDALONE_FRONTEND.id(),
            RUST_FRONTEND.id(),
            "two registrations for one extension must be distinguishable, since which of them a \
             request runs is decided per request"
        );
        let found = registry.for_extension("rs").expect("dispatch is by target-root extension");
        assert_eq!(found.id(), RUST_STANDALONE_FRONTEND.id());
        assert_eq!(found.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
        assert_eq!(found.resolve_alias("parse"), Some(CheckpointId::WASM_PARSED));
        assert_eq!(found.resolve_alias("analyze"), Some(CheckpointId::HIR_ANALYZED));
        assert_eq!(found.resolve_alias("transform"), Some(CheckpointId::HIR_TRANSFORMED));
        assert_eq!(found.resolve_alias("lower"), Some(CheckpointId::MASM_LOWERED));
        assert_eq!(found.resolve_alias("assemble"), Some(CheckpointId::PACKAGE_ASSEMBLED));
        assert_eq!(found.artifact_at(CheckpointId::WASM_PARSED), Some(ArtifactId::WASM));
        assert_eq!(found.artifact_at(CheckpointId::HIR_INITIAL), Some(ArtifactId::HIR));
        assert_eq!(found.artifact_at(CheckpointId::MASM_LOWERED), Some(ArtifactId::MASM));
    }

    /// One registry cannot hold both, which is why the standalone one is request-scoped.
    ///
    /// The registry's `rs` entry stays the *project* frontend, because that is what a Rust
    /// dependency of any project is; a standalone request installs its own provider for the
    /// extension instead. This pins the constraint that forces that arrangement rather than
    /// leaving it as a claim in a doc comment.
    #[test]
    fn one_registry_cannot_hold_both_rust_registrations() {
        let mut registry = FrontendRegistry::new();
        registry
            .register(RUST_FRONTEND)
            .expect("the project registration should register");

        let err = registry
            .register(RUST_STANDALONE_FRONTEND)
            .expect_err("`rs` is already claimed");
        let rendered = format!("{err}");
        assert!(rendered.contains("'rs'"), "the rejection must name the extension: {rendered}");
    }

    // -------------------------------------------------------------------------------------
    // Emission.
    //
    // Every `--emit` this route can satisfy is written inline by the code that owns it — the
    // `--emit=wat` document by the wasm frontend's own emitter, the two `--emit=hir` documents
    // by translation and by the shared backend's rewrites, `--emit=masm` by the shared
    // backend's codegen, and the package by `crate::compile`. Declaring a renderer here as
    // well would make two writers for one output type, and `Session::emit` resolves an output
    // type's destination once: the second writer either races for the same file or prints the
    // document twice when that destination is stdout.
    // -------------------------------------------------------------------------------------

    /// Every document with the `extension` extension in `dir`.
    fn documents(dir: &std::path::Path, extension: &str) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .expect("the output directory should exist")
            .map(|entry| entry.expect("should read a directory entry").path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
            .collect()
    }

    /// The declared output type and file extension of each checkpoint on the Rust routes.
    const RENDERED_AS: [(CheckpointId, OutputType, &str); 6] = [
        (CheckpointId::WASM_PARSED, OutputType::Wat, "wat"),
        (CheckpointId::HIR_INITIAL, OutputType::Hir, "hir"),
        (CheckpointId::HIR_ANALYZED, OutputType::Hir, "hir"),
        (CheckpointId::HIR_TRANSFORMED, OutputType::Hir, "hir"),
        (CheckpointId::MASM_LOWERED, OutputType::Masm, "masm"),
        (CheckpointId::PACKAGE_ASSEMBLED, OutputType::Masp, "masp"),
    ];

    /// Run every declared renderer of `frontend` into `dir`, and assert that none of them wrote.
    fn assert_route_is_unrendered(frontend: FrontendRegistration, dir: &str) {
        let out_dir = testing::fixture_dir(dir);
        for (checkpoint, output_type, extension) in RENDERED_AS {
            let output_types = OutputTypes::new([OutputTypeSpec::Typed {
                output_type,
                path: Some(OutputFile::Directory(out_dir.clone())),
            }])
            .expect("the output type is valid");
            let context = testing::context_emitting(output_types, |_| {});
            let decl = frontend
                .decl_at(checkpoint)
                .expect("every route checkpoint declares an artifact");
            let artifact = Artifact::new(decl.id, String::from("unused"));

            (decl.render)(&artifact, context.session())
                .unwrap_or_else(|err| panic!("rendering {checkpoint} must succeed: {err}"));

            assert!(
                documents(&out_dir, extension).is_empty(),
                "{checkpoint} must write nothing: its artifact is written inline"
            );
        }
    }

    /// None of the standalone route's declared renderers writes anything of its own.
    #[test]
    fn every_declaration_on_the_standalone_route_is_unrendered() {
        assert_route_is_unrendered(RUST_STANDALONE_FRONTEND, "rust_standalone_unrendered_out");
    }

    /// Nor does any of the project route's, which now declares the same six checkpoints.
    ///
    /// It reaches them through the same shared tail, so each document is already written by the
    /// phase that produced it — `--emit=wat` by `WasmFrontend::emit_wat`, the two `--emit=hir`
    /// documents by translation and by `backend::apply_rewrites`, `--emit=masm` by
    /// `backend::codegen`, and the package by `crate::compile`. A renderer here would be a
    /// second writer for an output type whose destination `Session::emit` resolves once.
    #[test]
    fn every_declaration_on_the_project_route_is_unrendered() {
        assert_route_is_unrendered(RUST_FRONTEND, "rust_project_unrendered_out");
    }

    // -------------------------------------------------------------------------------------
    // The manifest entry point: a project manifest to Miden Assembly.
    // -------------------------------------------------------------------------------------

    /// A workspace-level `miden-project.toml`.
    ///
    /// Deliberately memberless: every assertion below is about a decision the build makes
    /// *before* it would descend into a member, and a member would have to exist on disk.
    const WORKSPACE_MANIFEST: &str = "[workspace]\nmembers = []\n";

    /// A package-level `miden-project.toml`, naming one package.
    const PACKAGE_MANIFEST: &str = "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n";

    /// A package declaring two executables, neither of which a build can pick unaided.
    const TWO_EXECUTABLES_MANIFEST: &str = r#"
[package]
name = "fixture"
version = "0.1.0"

[[bin]]
name = "one"
path = "one.rs"

[[bin]]
name = "two"
path = "two.rs"
"#;

    /// A package whose library target is a kernel.
    const KERNEL_MANIFEST: &str = r#"
[package]
name = "fixture"
version = "0.1.0"

[lib]
kind = "kernel"
path = "lib.rs"
"#;

    /// A context for a manifest build, configured by `configure`.
    fn manifest_context(configure: impl FnOnce(&mut Options)) -> Rc<Context> {
        testing::context_emitting(Default::default(), configure)
    }

    /// The message [`compile_manifest`] failed with, or a panic carrying `unless`.
    ///
    /// [`CodegenOutput`] is not [`Debug`](core::fmt::Debug) — it holds a whole lowered
    /// component — so `expect_err` cannot be used on what this entry point returns.
    fn manifest_error(result: CompilerResult<CodegenOutput>, unless: &str) -> String {
        match result {
            Ok(_) => panic!("{unless}"),
            Err(err) => format!("{err}"),
        }
    }

    /// Write `manifest` as `<temp>/…/<dir>/miden-project.toml` and hand back its directory.
    ///
    /// [`testing::fixture_dir`] rather than [`testing::fixture_source`], because these tests
    /// assert on what is *absent* from the directory as much as what is present — the
    /// `Cargo.toml` derivation below turns on there being no `Cargo.toml` to read — and only
    /// `fixture_dir` empties the directory first.
    fn manifest_fixture(dir: &str, manifest: &str) -> std::path::PathBuf {
        let dir = testing::fixture_dir(dir);
        std::fs::write(dir.join("miden-project.toml"), manifest)
            .expect("should write the project manifest");
        dir
    }

    /// A kernel project is refused, and refused before `cargo` is spawned for it.
    ///
    /// The compiler does not support kernels through this route. Reaching `cargo` first would
    /// spend a multi-minute build on a project that cannot be finished.
    #[test]
    fn a_kernel_project_is_refused_before_cargo_runs() {
        let dir = manifest_fixture("rust_manifest_kernel", KERNEL_MANIFEST);

        let msg = manifest_error(
            compile_manifest(&dir.join("miden-project.toml"), None, manifest_context(|_| {})),
            "a kernel cannot be built through this route",
        );
        assert!(
            msg.contains("kernels are not currently supported"),
            "a kernel target must be refused on its own terms: {msg}"
        );
    }

    /// `--target` naming an executable the project does not declare is refused.
    #[test]
    fn an_unknown_executable_selection_is_refused() {
        let dir = manifest_fixture("rust_manifest_unknown_target", TWO_EXECUTABLES_MANIFEST);

        let msg = manifest_error(
            compile_manifest(
                &dir.join("miden-project.toml"),
                None,
                manifest_context(|options| options.target = Some("three".to_string())),
            ),
            "the project declares no executable named 'three'",
        );
        assert!(
            msg.contains("no executable target name 'three'"),
            "the diagnostic must name the target it could not find: {msg}"
        );
    }

    /// The session and the manifest build agree on which executable a project builds.
    ///
    /// They answer the same question for different halves of one build: the session derives
    /// `--entrypoint` from the selected executable, and the manifest build refuses to spawn
    /// `cargo` for a project whose executable cannot be identified. Both now go through
    /// [`ProjectManifest::selected_executable`], and this is what says so — an ambiguity has to
    /// be an ambiguity on both sides, or a project would be refused by one and built by the
    /// other. They carried separate copies of the rule until they were converged.
    #[test]
    fn an_ambiguous_executable_is_refused_by_both_the_session_and_the_manifest_build() {
        let dir = manifest_fixture("rust_manifest_ambiguous_target", TWO_EXECUTABLES_MANIFEST);
        // The session's copy is reached only for a `Cargo.toml` input, which resolves to the
        // manifest beside it — so both sides are asked about the very same project.
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"fixture\"\n")
            .expect("should write the Cargo manifest");

        let from_manifest_build = manifest_error(
            compile_manifest(&dir.join("miden-project.toml"), None, manifest_context(|_| {})),
            "a project declaring two executables cannot be built without a selection",
        );
        let from_session = Session::new(
            InputFile::from_path(dir.join("Cargo.toml")).expect("a Cargo manifest is an input"),
            alloc::boxed::Box::default(),
            None,
            Arc::new(midenc_session::diagnostics::DefaultSourceManager::default()),
        )
        .err()
        .map(|err| format!("{err}"))
        .expect("a session cannot name an entrypoint for a project declaring two executables");

        let expected = "ambiguous executable target selection";
        assert!(
            from_manifest_build.contains(expected),
            "the manifest build must refuse an ambiguous executable: {from_manifest_build}"
        );
        assert!(
            from_session.contains(expected),
            "and the session must refuse it identically: {from_session}"
        );
    }

    /// A Cargo manifest path is resolved to the project manifest beside it.
    ///
    /// This is the shape every caller passes — `cargo miden` hands over a `Cargo.toml` — and
    /// the derivation is invisible unless something downstream of it can only have come from
    /// the sibling. So the fixture directory holds a *workspace* `miden-project.toml` and no
    /// `Cargo.toml` at all: the workspace rejection can only be raised by a build that read
    /// the sibling, and an implementation that loaded the given path verbatim would fail to
    /// find a file instead.
    #[test]
    fn a_cargo_manifest_is_resolved_to_the_project_manifest_beside_it() {
        let dir = manifest_fixture("rust_manifest_from_cargo_toml", WORKSPACE_MANIFEST);
        assert!(!dir.join("Cargo.toml").exists(), "the Cargo manifest must not exist");

        let msg = manifest_error(
            compile_manifest(&dir.join("Cargo.toml"), None, manifest_context(|_| {})),
            "a workspace manifest cannot be built without a selection",
        );
        assert!(
            msg.contains("--workspace was not specified"),
            "the build must have read the `miden-project.toml` beside the Cargo manifest: {msg}"
        );
    }

    /// A path already naming a project manifest is used as given.
    ///
    /// The discriminating half of the test above: the same rejection, reached without any
    /// derivation, which is what says the derivation applies to Cargo manifests only.
    #[test]
    fn a_project_manifest_is_used_as_given() {
        let dir = manifest_fixture("rust_manifest_direct", WORKSPACE_MANIFEST);

        let msg = manifest_error(
            compile_manifest(&dir.join("miden-project.toml"), None, manifest_context(|_| {})),
            "a workspace manifest cannot be built without a selection",
        );
        assert!(
            msg.contains("--workspace was not specified"),
            "a `miden-project.toml` path must be loaded as given: {msg}"
        );
    }

    /// `--workspace` and `--package` each take the workspace route, and reach different code.
    ///
    /// Together with the two tests above this pins the whole of the workspace fork: without a
    /// selection the build refuses, with `--workspace` it enters the workspace builder, and
    /// with `--package` it resolves the selection against the workspace's members. All three
    /// are decided before `cargo` is spawned, which is what makes them assertable at all.
    #[test]
    fn a_workspace_manifest_is_built_only_when_a_selection_was_made() {
        let dir = manifest_fixture("rust_manifest_workspace", WORKSPACE_MANIFEST);
        let manifest = dir.join("miden-project.toml");

        let msg = manifest_error(
            compile_manifest(&manifest, None, manifest_context(|options| options.workspace = true)),
            "this workspace has no members to build",
        );
        assert!(
            msg.contains("contains no members"),
            "`--workspace` must reach the workspace builder: {msg}"
        );

        let msg = manifest_error(
            compile_manifest(
                &manifest,
                None,
                manifest_context(|options| options.packages = vec!["absent".to_string()]),
            ),
            "the selected package is not a member of this workspace",
        );
        assert!(
            msg.contains("is not a valid workspace member"),
            "`--package` must be resolved against the workspace's own members: {msg}"
        );
    }

    /// A `--package` selection must name the package the manifest defines.
    ///
    /// The single-package half of the fork above, and the one a mistyped `--package` hits:
    /// selecting a package that is not this one must be reported rather than silently
    /// building whatever the manifest happens to define.
    ///
    /// # The message this asserts on is inverted, and that is a defect, not an intent
    ///
    /// The diagnostic interpolates the two package names into **opposite** slots, so it reads
    /// *"the provided project manifest defines a package (other) that differs from the one
    /// requested via --package (fixture)"* — the requested name where the defined one belongs
    /// and vice versa. The assertion below is therefore about *which names appear*, not about
    /// which role each is given: `msg.contains("fixture")` passes on the inverted text, and
    /// would keep passing if the slots were corrected.
    ///
    /// The bug is **pre-existing**: the message came over verbatim with the rest of
    /// `stages::cargo::support`, and correcting it in a move task would have made the move
    /// unverifiable. It is reported separately. Anyone tightening this test to assert on the
    /// roles must fix the message first, or the tightened assertion will fail.
    #[test]
    fn a_package_selection_must_name_the_manifests_own_package() {
        let dir = manifest_fixture("rust_manifest_package_selection", PACKAGE_MANIFEST);

        let msg = manifest_error(
            compile_manifest(
                &dir.join("miden-project.toml"),
                None,
                manifest_context(|options| options.packages = vec!["other".to_string()]),
            ),
            "`other` is not the package this manifest defines",
        );
        assert!(
            msg.contains("--package") && msg.contains("fixture") && msg.contains("other"),
            "the rejection must name both the requested package and the one the manifest defines, \
             whichever way round it prints them: {msg}"
        );
    }

    /// The filesystem cache directory reaches the `cargo` invocation, and only when given.
    ///
    /// This is the second of the entry point's two parameters, and the one with no other
    /// observable effect: it is delivered to the nested build as `MIDENC_PACKAGE_CACHE`, which
    /// is how a dependency compiled here is found again by the crates that import it. Asserted
    /// against the environment the spawn is configured with rather than by running `cargo`,
    /// which is minutes of work and a network fetch.
    #[test]
    fn the_filesystem_cache_directory_is_handed_to_the_nested_cargo_build() {
        let rustflags = vec!["-C".to_string(), "target-feature=+bulk-memory".to_string()];
        let cache = std::path::Path::new("/tmp/midenc-package-cache");

        let with = manifest::cargo_env(Some(cache), &rustflags);
        assert!(
            with.iter()
                .any(|(key, value)| *key == "RUSTFLAGS" && *value == rustflags.join(" ")),
            "the rust flags must survive the cache directory being added: {with:?}"
        );
        let (_, found) = with
            .iter()
            .find(|(key, _)| *key == "MIDENC_PACKAGE_CACHE")
            .expect("a cache directory must be delivered to the nested build");
        assert_eq!(found, &cache.display().to_string());

        let without = manifest::cargo_env(None, &rustflags);
        assert!(
            !without.iter().any(|(key, _)| *key == "MIDENC_PACKAGE_CACHE"),
            "no cache directory means the variable is unset, not set to nothing: {without:?}"
        );
        assert!(
            without
                .iter()
                .any(|(key, value)| *key == "RUSTFLAGS" && *value == rustflags.join(" ")),
            "and the rust flags are handed over either way: {without:?}"
        );
    }

    #[test]
    fn the_encoded_rustflags_override_inherited_values_with_the_same_flags() {
        let rustflags = vec![
            "-C".to_string(),
            "target-feature=+bulk-memory".to_string(),
            "--cfg".to_string(),
            "miden".to_string(),
        ];

        let env = manifest::cargo_env(None, &rustflags);
        let (_, encoded) = env
            .iter()
            .find(|(key, _)| *key == "CARGO_ENCODED_RUSTFLAGS")
            .expect("the encoded variable must be set so an inherited value cannot override it");
        // 0x1f-separated units, one per argument.
        assert_eq!(*encoded, "-C\x1ftarget-feature=+bulk-memory\x1f--cfg\x1fmiden");
    }

    #[test]
    fn merged_rust_flags_preserve_inherited_encoded_arguments() {
        use std::ffi::OsStr;

        let mandatory = ["--cfg", "miden"];

        // A non-empty encoded value wins over the plain spelling, mirroring cargo's precedence,
        // and its 0x1f-separated arguments survive verbatim — including ones with spaces.
        let merged = manifest::merge_rust_flags(
            &mandatory,
            Some(OsStr::new("-C\x1flink-args=--flag one --flag two")),
            Some(OsStr::new("--cfg plain_ignored")),
            Some("--cfg explicit"),
        );
        assert_eq!(
            merged,
            vec![
                "--cfg".to_string(),
                "miden".to_string(),
                "-C".to_string(),
                "link-args=--flag one --flag two".to_string(),
                "--cfg".to_string(),
                "explicit".to_string(),
            ]
        );

        // Without an encoded value the plain spelling is whitespace-split, as cargo does.
        let merged =
            manifest::merge_rust_flags(&mandatory, None, Some(OsStr::new("-C opt-level=3")), None);
        assert_eq!(
            merged,
            vec![
                "--cfg".to_string(),
                "miden".to_string(),
                "-C".to_string(),
                "opt-level=3".to_string(),
            ]
        );

        // Empty inherited values are treated as unset.
        let merged = manifest::merge_rust_flags(
            &mandatory,
            Some(OsStr::new("")),
            Some(OsStr::new("")),
            None,
        );
        assert_eq!(merged, vec!["--cfg".to_string(), "miden".to_string()]);
    }

    /// A WebAssembly module with a body, for the lowering half of the entry point.
    const MANIFEST_WAT: &str = r#"
(module
  (func $add (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add)
)
"#;

    /// The entry point's second half turns the build's WebAssembly into a [`CodegenOutput`].
    ///
    /// The first half is a real `cargo build -Z build-std` against the Miden SDK — minutes of
    /// work and a network fetch — so what is decidable here is that the module such a build
    /// hands back is lowered all the way to Miden Assembly, which is the whole of what this
    /// entry point contributes past the build itself. The lit suite and the `cargo miden`
    /// integration tests cover the two halves joined.
    #[test]
    fn the_web_assembly_a_manifest_build_produces_is_lowered_to_miden_assembly() {
        let wasm = testing::fixture_source("rust_manifest_lowering", "lib.wat", MANIFEST_WAT);
        let input = InputFile::from_path(wasm).expect("a `.wat` file is a valid compiler input");

        let output = lower_wasm_artifact(input, manifest_context(|_| {}))
            .expect("the module a manifest build produces must lower to Miden Assembly");

        // Two weaker assertions this deliberately avoids. A module *count* is satisfied by a
        // build that lowered nothing: codegen emits a root module for every component, an
        // empty one included. And a bare `masm.contains("add")` is satisfied by a body that
        // dropped the export entirely, because `add` is also a Miden Assembly mnemonic — this
        // fixture's `i32.add` lowers to `u32wrapping_add`, whose text contains it.
        //
        // So the assertion is the procedure *declaration*, carrying the signature the fixture
        // declared: nothing but lowering this module's own exported function produces it.
        let masm = output
            .component
            .modules
            .iter()
            .map(|module| format!("{module}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            masm.contains("pub proc add(i32, i32) -> i32"),
            "the lowered Miden Assembly must export the fixture's own procedure: {masm}"
        );
    }
}
