use core::{fmt::Write as _, str::FromStr};
use std::{
    boxed::Box,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::{SourceManager, serde::Serializable};
use miden_mast_package::Package as MastPackage;
use miden_note_codec_wit::NOTE_CODEC_WIT;
use midenc_hir::Report;
use midenc_session::{InputFile, LinkLibrary, Session, miden_project};
use sha2::{Digest, Sha256};
use wit_component::DecodedWasm;
use wit_parser::{Function, FunctionKind, Resolve, Type, TypeDefKind, TypeId, WorldId, WorldItem};

use crate::{CodegenOutput, CompilerResult};

/// Metadata table that points to an author-side note codec crate.
const NOTE_CODEC_CRATE_METADATA: &str = "note-codec-crate";

/// Metadata field that contains the codec crate directory.
const NOTE_CODEC_CRATE_PATH: &str = "path";

/// Rust target used to build note codec components; rustc links the cdylib as a
/// Wasm component, so no separate encoding step is necessary.
const NOTE_CODEC_TARGET: &str = "wasm32-wasip2";

/// Directory used to exchange Miden packages with nested Cargo builds.
const PACKAGE_CACHE_ENV: &str = "MIDENC_PACKAGE_CACHE";

/// One immutable staged package and its build-isolation key.
struct StagedNotePackage {
    cache_dir: PathBuf,
    build_key: String,
}

/// Cargo-specific options extracted from the `Compiler` struct.
///
/// These options are recognized by `cargo miden build` and forwarded to the underlying
/// `cargo build` invocation. They are not used by the `midenc` compiler itself.
#[derive(Debug, Default)]
pub struct CargoOptions {
    /// Build in release mode
    pub release: bool,
    /// Path to Cargo.toml
    pub manifest_path: Option<PathBuf>,
    /// Build all packages in the workspace
    pub workspace: bool,
    /// Packages to build
    pub packages: Vec<CargoPackageSpec>,
    /// Require Cargo.lock to remain unchanged.
    pub locked: bool,
    /// Prevent Cargo from accessing the network.
    pub offline: bool,
}

/// Represents a cargo package specifier.
///
/// See `cargo help pkgid` for more information.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CargoPackageSpec {
    /// The name of the package, e.g. `foo`.
    pub name: String,
    /// The version of the package, if specified.
    pub version: Option<miden_mast_package::Version>,
}

impl CargoPackageSpec {
    /// Creates a new package specifier from a string.
    pub fn new(spec: impl Into<String>) -> CompilerResult<Self> {
        let spec = spec.into();

        // Bail out if the package specifier contains a URL.
        if spec.contains("://") {
            return Err(Report::msg("URL package specifier `{spec}` is not supported"));
        }

        Ok(match spec.split_once('@') {
            Some((name, version)) => Self {
                name: name.to_string(),
                version: Some(version.parse().map_err(|err| {
                    Report::msg(format!("invalid package version '{spec}': `{err}`"))
                })?),
            },
            None => Self {
                name: spec,
                version: None,
            },
        })
    }
}

impl FromStr for CargoPackageSpec {
    type Err = Report;

    fn from_str(s: &str) -> CompilerResult<Self> {
        Self::new(s)
    }
}

impl core::fmt::Display for CargoPackageSpec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{name}", name = self.name)?;
        if let Some(version) = &self.version {
            write!(f, "@{version}")?;
        }

        Ok(())
    }
}

impl CargoOptions {
    /// Extract cargo-specific options from a Compiler struct.
    pub fn from_compiler(options: &midenc_session::Options) -> CompilerResult<Self> {
        let packages = options
            .packages
            .iter()
            .map(|s| CargoPackageSpec::new(s.clone()))
            .collect::<CompilerResult<Vec<_>>>()?;

        Ok(Self {
            release: options.profile == "release",
            manifest_path: options.manifest_path.clone(),
            workspace: options.workspace,
            packages,
            locked: options.cargo_locked,
            offline: options.cargo_offline,
        })
    }
}

#[cfg(false)]
pub fn load_cargo_based_source_dependencies(
    package: &miden_project::Package,
    dependency_graph: &miden_project::ProjectDependencyGraph,
    registry: &mut midenc_session::registry::HybridPackageRegistry,
    options: &midenc_session::Options,
    cargo_opts: &CargoOptions,
    source_manager: Arc<dyn SourceManager + Send + Sync>,
) -> CompilerResult<()> {
    log::debug!(target: "cargo-miden", "processing Cargo-based source dependencies for {}@{}", package.name(), package.version());

    let package_id = package.name().into_inner();
    let Some(node) = dependency_graph.get(&package_id) else {
        log::debug!(target: "cargo-miden", "{package_id} has no dependencies");
        return Ok(());
    };

    let mut visited = BTreeSet::new();
    for dep in node.dependencies.iter() {
        if !visited.insert(dep.dependency.clone()) {
            continue;
        }
        let Some(dependency) = dependency_graph.get(&dep.dependency) else {
            return Err(Report::msg(format!(
                "unable to resolve dependency on '{}'",
                &dep.dependency
            )));
        };
        match &dependency.provenance {
            miden_project::ProjectDependencyNodeProvenance::Preassembled {
                path, selected, ..
            } => {
                log::debug!(target: "cargo-miden", "resolved dependency {}@{selected} to preassembled package at {}", &dependency.name, path.display());
            }
            miden_project::ProjectDependencyNodeProvenance::Registry {
                requirement,
                selected: _,
            } => {
                log::debug!(target: "cargo-miden", "expecting dependency {} {requirement} to be in registry", &dependency.name);
            }
            miden_project::ProjectDependencyNodeProvenance::Source(proj) => match proj {
                miden_project::ProjectSource::Real { manifest_path, .. } => {
                    let project = miden_project::Project::load(manifest_path, &source_manager)?;
                    let project_package = project.package();
                    let target = project_package.library_target().ok_or_else(|| {
                        Report::msg(format!(
                            "invalid dependency '{}': no a library target",
                            project_package.name()
                        ))
                    })?;
                    if target.path.is_none() {
                        let package = cargo_build(
                            project_package.clone(),
                            target.inner(),
                            manifest_path.with_file_name("Cargo.toml"),
                            options,
                            cargo_opts,
                            source_manager.clone(),
                        )?;
                        registry.publish_package(package)?;
                    }
                }
                miden_project::ProjectSource::Virtual { .. } => {
                    unreachable!("virtual manifests are only possible at the top-level")
                }
            },
        }
    }

    Ok(())
}

/// Build the dependency `target` of the project at `manifest_path` with `cargo`.
///
/// `package_name` names the package that declares `target`, and is the name the nested session is
/// opened under. The package itself is not needed: the nested build loads the manifest it is
/// pointed at, and the project *that* yields is the one it compiles.
pub(crate) fn cargo_build(
    package_name: String,
    target: &miden_project::Target,
    manifest_path: std::path::PathBuf,
    filesystem_cache_dir: Option<&std::path::Path>,
    options: &midenc_session::Options,
    cargo_opts: &CargoOptions,
    source_manager: Arc<dyn SourceManager>,
) -> CompilerResult<CodegenOutput> {
    // The directory of the dependency being compiled, captured before `manifest_path` is consumed
    // below. The compiled package is materialized under this directory's `target` (see the end of
    // this function).
    /*
    let dependency_dir = manifest_path
        .parent()
        .ok_or_else(|| {
            Report::msg(format!(
                "dependency manifest path '{}' has no parent directory",
                manifest_path.display()
            ))
        })?
        .to_path_buf();
         */
    let mut nested_options = Box::new(midenc_session::Options {
        manifest_path: Some(manifest_path.clone()),
        target: Some(target.name.to_string()),
        optimize: options.optimize,
        debug: options.debug,
        search_paths: options.search_paths.clone(),
        midenup_home: options.midenup_home.clone(),
        toolchain: options.toolchain.clone(),
        color: options.color,
        diagnostics: options.diagnostics,
        remap_path_prefixes: options.remap_path_prefixes.clone(),
        rustflags: options.rustflags.clone(),
        cargo_locked: options.cargo_locked,
        cargo_offline: options.cargo_offline,
        link_libraries: vec![LinkLibrary::core()],
        ..midenc_session::Options::new(
            Some(package_name.clone()),
            Some(target.ty),
            options.current_dir.clone(),
            options.target_dir.clone(),
            options.output_dir.clone(),
            options.sysroot.clone(),
        )
    })
    .with_output_types(Default::default(), None);
    if nested_options.target_requires_protocol() {
        nested_options.link_libraries.push(LinkLibrary::protocol());
    }
    // Inherit release/debug profile from parent build
    if cargo_opts.release {
        nested_options.profile = "release".to_string();
    }

    let input = InputFile::from_path(manifest_path.clone()).unwrap();
    let session = Rc::new(Session::new_project(
        package_name.clone(),
        Some(input.clone()),
        nested_options,
        None,
        source_manager,
    ));
    let context = Rc::new(midenc_hir::Context::new(session));

    crate::pipeline::frontends::rust::compile_manifest(
        &manifest_path,
        filesystem_cache_dir,
        context,
    )
    // We expect dependencies to *always* produce packages (.masp)
    /*
    let CodegenOutput {
        component,
        sections,
    } = crate::pipeline::frontends::rust::compile_manifest(&manifest_path, None, context.clone())?
    else {
        panic!(
            "expected cargo build of {package_name} to produce component, but got HIR output \
             instead",
        );
    };

    Ok(CodegenOutput {
        component,
        sections,
    })
     */

    //component.source_inputs(target, context.session())

    /*
    // Materialize the compiled dependency package on disk, in addition to publishing it to the
    // in-memory registry. A dependent crate that imports this dependency (e.g. via
    // `#[account(..)]`) resolves the dependency's `.masp` from disk while expanding its own Rust
    // macros. The profile sub-directory mirrors the one searched by that macro: `release` for
    // release builds and `debug` otherwise.
    let profile = if cargo_opts.release {
        "release"
    } else {
        "debug"
    };
    let masp_out_dir = dependency_dir.join("target").join("miden").join(profile);
    write_package_atomic(&package, &masp_out_dir).map_err(|err| {
        Report::msg(format!(
            "failed to materialize dependency package '{package_name}' to '{}': {err}",
            masp_out_dir.display()
        ))
    })?;

    Ok(package)
     */
}

/// Write `package` to `<out_dir>/<package name>.masp`, atomically.
///
/// Delegates to [`midenc_session::registry::write_package_atomically`], which carries the
/// concurrent-reader rationale for the temp-file-and-rename publication.
pub fn write_package_atomic(
    package: &miden_mast_package::Package,
    out_dir: &Path,
) -> Result<PathBuf, Report> {
    midenc_session::registry::write_package_atomically(package, out_dir).map_err(|err| {
        Report::msg(format!(
            "failed to publish package artifact '{}.{}' into '{}': {err}",
            &*package.name,
            miden_mast_package::Package::EXTENSION,
            out_dir.display()
        ))
    })
}

/// Returns true when project metadata declares an author-side note codec crate.
pub(crate) fn has_project_note_codec(metadata: &miden_project::MetadataSet) -> bool {
    metadata.get(NOTE_CODEC_CRATE_METADATA).is_some()
}

/// Builds the optional note codec component declared by a project package.
pub(crate) fn build_project_note_codec(
    project_package: &miden_project::Package,
    project_manifest_path: &Path,
    note_project_dir: &Path,
    note_package: &MastPackage,
    session: &Session,
) -> CompilerResult<Option<Vec<u8>>> {
    let Some(codec_crate_dir) =
        note_codec_crate_dir(project_package.metadata(), project_manifest_path, note_project_dir)?
    else {
        return Ok(None);
    };

    build_note_codec_component(&codec_crate_dir, note_package, session).map(Some)
}

/// Reads the optional codec crate path from Miden project metadata.
fn note_codec_crate_dir(
    metadata: &miden_project::MetadataSet,
    project_manifest_path: &Path,
    project_dir: &Path,
) -> CompilerResult<Option<PathBuf>> {
    let Some(codec_metadata) = metadata.get(NOTE_CODEC_CRATE_METADATA) else {
        return Ok(None);
    };
    let path = codec_metadata
        .get(NOTE_CODEC_CRATE_PATH)
        .ok_or_else(|| {
            Report::msg(format!(
                "`[package.metadata.{NOTE_CODEC_CRATE_METADATA}]` in '{}' must define a string \
                 `{NOTE_CODEC_CRATE_PATH}`",
                project_manifest_path.display()
            ))
        })?
        .inner()
        .as_str()
        .ok_or_else(|| {
            Report::msg(format!(
                "`[package.metadata.{NOTE_CODEC_CRATE_METADATA}].{NOTE_CODEC_CRATE_PATH}` in '{}' \
                 must be a string",
                project_manifest_path.display()
            ))
        })?;
    if path.is_empty() {
        return Err(Report::msg(format!(
            "`[package.metadata.{NOTE_CODEC_CRATE_METADATA}].{NOTE_CODEC_CRATE_PATH}` in '{}' \
             must not be empty",
            project_manifest_path.display()
        )));
    }

    let codec_crate_dir = project_dir.join(path);
    let codec_crate_dir = codec_crate_dir.canonicalize().map_err(|error| {
        Report::msg(format!(
            "note codec crate '{}' does not exist; update \
             `[package.metadata.{NOTE_CODEC_CRATE_METADATA}].{NOTE_CODEC_CRATE_PATH}` in '{}': \
             {error}",
            codec_crate_dir.display(),
            project_manifest_path.display()
        ))
    })?;
    if !codec_crate_dir.is_dir() {
        return Err(Report::msg(format!(
            "note codec crate path '{}' is not a directory",
            codec_crate_dir.display()
        )));
    }
    Ok(Some(codec_crate_dir))
}

/// Builds and componentizes one author-side note codec crate.
fn build_note_codec_component(
    codec_crate_dir: &Path,
    note_package: &MastPackage,
    session: &Session,
) -> CompilerResult<Vec<u8>> {
    let session_target_dir = if session.options.target_dir.is_absolute() {
        session.options.target_dir.clone()
    } else {
        session.options.current_dir.join(&session.options.target_dir)
    };
    let work_dir = session_target_dir.join(&session.options.profile).join("note-codec");
    let staged_package = stage_note_package(&work_dir, codec_crate_dir, note_package)?;
    let manifest_path = codec_crate_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(Report::msg(format!(
            "note codec crate has no manifest at '{}'",
            manifest_path.display()
        )));
    }

    let cargo_env = env::var_os("CARGO").map(PathBuf::from);
    let cargo_path = cargo_env.as_deref().unwrap_or_else(|| Path::new("cargo"));
    let toolchain = if cargo_env.is_none() {
        crate::rust::rustup_toolchain()
    } else {
        None
    };
    crate::rust::install_wasm32_target("wasip2", toolchain.as_deref())?;

    let cargo_target_dir = work_dir.join("cargo-target").join(&staged_package.build_key);
    let mut cargo = Command::new(cargo_path);
    if let Some(toolchain) = toolchain.as_deref() {
        cargo.arg(format!("+{toolchain}"));
    }
    cargo
        .current_dir(codec_crate_dir)
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--lib")
        .arg("--profile")
        .arg(&session.options.profile)
        .arg("--target")
        .arg(NOTE_CODEC_TARGET)
        .arg("--target-dir")
        .arg(&cargo_target_dir)
        .arg("--message-format")
        .arg("json-render-diagnostics")
        .env(PACKAGE_CACHE_ENV, &staged_package.cache_dir)
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    apply_cargo_policy(&mut cargo, session.options.cargo_locked, session.options.cargo_offline);

    let manifest_path = manifest_path.canonicalize().map_err(|error| {
        Report::msg(format!(
            "failed to resolve note codec manifest '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let artifacts = crate::rust::spawn_cargo(cargo, cargo_path).map_err(|error| {
        note_codec_cargo_error(
            error,
            &manifest_path,
            session.options.cargo_locked,
            session.options.cargo_offline,
        )
    })?;
    let mut wasm_paths = artifacts
        .into_iter()
        .filter(|artifact| {
            artifact.manifest_path.as_std_path() == manifest_path
                && artifact.target.crate_types.contains(&cargo_metadata::CrateType::CDyLib)
        })
        .flat_map(|artifact| artifact.filenames)
        .filter(|path| path.extension() == Some("wasm"))
        .map(|path| path.into_std_path_buf())
        .collect::<Vec<_>>();
    wasm_paths.sort();
    wasm_paths.dedup();
    if wasm_paths.len() != 1 {
        return Err(Report::msg(format!(
            "note codec build for '{}' must produce exactly one `{NOTE_CODEC_TARGET}` cdylib; set \
             `[lib] crate-type = [\"cdylib\"]` in '{}', found: {wasm_paths:#?}",
            codec_crate_dir.display(),
            manifest_path.display()
        )));
    }
    let wasm_path = wasm_paths.pop().expect("one codec artifact was checked above");

    // The wasm32-wasip2 target links through wasm-component-ld, so the produced
    // cdylib artifact is already a Wasm component.
    let component = fs::read(&wasm_path).map_err(|error| {
        Report::msg(format!(
            "note codec build produced an unreadable component '{}': {error}",
            wasm_path.display()
        ))
    })?;
    validate_note_codec_component(&component).map_err(|error| {
        Report::msg(format!(
            "note codec crate '{}' produced an invalid component: {error}",
            codec_crate_dir.display()
        ))
    })?;
    Ok(component)
}

/// Stages the current package in a content-addressed package-cache directory.
fn stage_note_package(
    work_dir: &Path,
    codec_crate_dir: &Path,
    note_package: &MastPackage,
) -> CompilerResult<StagedNotePackage> {
    let package_bytes = note_package.to_bytes();
    let mut hasher = Sha256::new();
    hasher.update(&package_bytes);
    hasher.update([0]);
    hasher.update(codec_crate_dir.as_os_str().to_string_lossy().as_bytes());
    let mut build_key = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut build_key, "{byte:02x}").expect("writing to a string cannot fail");
    }
    let cache_dir = work_dir.join("package-cache").join(&build_key);
    let package_path = cache_dir.join(&*note_package.name).with_extension(MastPackage::EXTENSION);

    if package_path.is_file() {
        let staged_bytes = fs::read(&package_path).map_err(|error| {
            Report::msg(format!(
                "failed to read staged note package '{}': {error}",
                package_path.display()
            ))
        })?;
        if staged_bytes != package_bytes {
            return Err(Report::msg(format!(
                "content-addressed note package path '{}' contains different bytes",
                package_path.display()
            )));
        }
    } else {
        write_package_atomic(note_package, &cache_dir).map_err(|error| {
            Report::msg(format!(
                "failed to stage note package {}@{} for codec generation: {error}",
                note_package.name, note_package.version
            ))
        })?;
    }

    Ok(StagedNotePackage {
        cache_dir,
        build_key,
    })
}

/// Applies the outer Cargo resolution policy to a nested command.
fn apply_cargo_policy(cargo: &mut Command, locked: bool, offline: bool) {
    if locked {
        cargo.arg("--locked");
    }
    if offline {
        cargo.arg("--offline");
    }
}

/// Adds lockfile and network recovery guidance to a nested Cargo failure.
fn note_codec_cargo_error(
    error: Report,
    manifest_path: &Path,
    locked: bool,
    offline: bool,
) -> Report {
    if !locked && !offline {
        return error;
    }

    let mut guidance = format!(
        "note codec build failed under the outer Cargo policy for '{}': {error}",
        manifest_path.display()
    );
    if locked {
        guidance.push_str(
            "; update and commit the codec workspace Cargo.lock before retrying with --locked",
        );
    }
    if offline {
        guidance
            .push_str("; fetch the codec dependencies while online before retrying with --offline");
    }
    Report::msg(guidance)
}

/// Verifies the component sandbox and the versioned codec interface export.
fn validate_note_codec_component(component: &[u8]) -> CompilerResult<()> {
    let DecodedWasm::Component(resolve, world_id) =
        wit_component::decode(component).map_err(|error| {
            Report::msg(format!("failed to decode the encoded note codec component: {error}"))
        })?
    else {
        return Err(Report::msg("note codec output is not a component"));
    };
    let world = &resolve.worlds[world_id];
    // The wasm32-wasip2 standard library wires WASI interfaces into every component.
    // Consumers stub them as trapping imports, so only `wasi:*` imports are permitted.
    for (key, _) in world.imports.iter() {
        let name = resolve.name_world_key(key);
        if !name.starts_with("wasi:") {
            return Err(Report::msg(format!(
                "note codec component may import only `wasi:*` interfaces, found `{name}`"
            )));
        }
    }
    if world.exports.len() != 1 {
        return Err(Report::msg(format!(
            "note codec component must export only `miden:note-codec/codec@1.0.0`, found: {:#?}",
            world.exports
        )));
    }

    let (actual_key, actual_interface_id) = codec_world_export(&resolve, world_id)?;

    let mut expected = Resolve::default();
    let package_id = expected.push_str("note-codec.wit", NOTE_CODEC_WIT).map_err(|error| {
        Report::msg(format!("failed to resolve the pinned note codec WIT: {error:#}"))
    })?;
    let expected_world_id = expected.packages[package_id]
        .worlds
        .get("note-codec")
        .copied()
        .ok_or_else(|| Report::msg("pinned note codec WIT has no `note-codec` world"))?;
    let (expected_key, expected_interface_id) = codec_world_export(&expected, expected_world_id)?;

    if actual_key != expected_key {
        return Err(Report::msg(format!(
            "note codec world exports `{actual_key}`, expected `{expected_key}`"
        )));
    }
    compare_codec_interface_signatures(
        &resolve,
        actual_interface_id,
        &expected,
        expected_interface_id,
    )
}

/// Returns the canonical key and interface ID for the sole codec world export.
fn codec_world_export(
    resolve: &Resolve,
    world_id: WorldId,
) -> CompilerResult<(String, wit_parser::InterfaceId)> {
    let world = &resolve.worlds[world_id];
    let (key, item) = world.exports.iter().next().ok_or_else(|| {
        Report::msg("note codec component does not export `miden:note-codec/codec@1.0.0`")
    })?;
    let WorldItem::Interface { id, .. } = item else {
        return Err(Report::msg(format!(
            "note codec world export `{}` is not an interface",
            resolve.name_world_key(key)
        )));
    };
    let interface = &resolve.interfaces[*id];
    let package_id = interface.package.ok_or_else(|| {
        Report::msg("note codec world exports an interface without a package identity")
    })?;
    let package = &resolve.packages[package_id].name;
    if interface.name.as_deref() != Some("codec")
        || package.namespace != "miden"
        || package.name != "note-codec"
        || package.version.as_ref().is_none_or(|version| version.to_string() != "1.0.0")
    {
        return Err(Report::msg("component does not export `miden:note-codec/codec@1.0.0`"));
    }
    Ok((resolve.name_canonicalized_world_key(key), *id))
}

/// Compares all exported codec function names and structural signatures.
fn compare_codec_interface_signatures(
    actual_resolve: &Resolve,
    actual_id: wit_parser::InterfaceId,
    expected_resolve: &Resolve,
    expected_id: wit_parser::InterfaceId,
) -> CompilerResult<()> {
    let actual = &actual_resolve.interfaces[actual_id];
    let expected = &expected_resolve.interfaces[expected_id];
    if actual.functions.len() != expected.functions.len() {
        return Err(Report::msg(format!(
            "`miden:note-codec/codec@1.0.0` exports {} functions, expected {}",
            actual.functions.len(),
            expected.functions.len()
        )));
    }
    for (name, expected_function) in &expected.functions {
        let actual_function = actual.functions.get(name).ok_or_else(|| {
            Report::msg(format!("`miden:note-codec/codec@1.0.0` is missing `{name}`"))
        })?;
        let actual_signature = function_signature(actual_resolve, actual_function)?;
        let expected_signature = function_signature(expected_resolve, expected_function)?;
        if actual_signature != expected_signature {
            return Err(Report::msg(format!(
                "`miden:note-codec/codec@1.0.0.{name}` has signature `{actual_signature}`, \
                 expected `{expected_signature}`"
            )));
        }
    }
    Ok(())
}

/// Returns a structural function signature with aliases resolved.
fn function_signature(resolve: &Resolve, function: &Function) -> CompilerResult<String> {
    if function.kind != FunctionKind::Freestanding {
        return Err(Report::msg(format!(
            "note codec function `{}` must be freestanding",
            function.name
        )));
    }
    let params = function
        .params
        .iter()
        .map(|param| {
            canonical_wit_type(resolve, param.ty, &mut Vec::new())
                .map(|ty| format!("{}: {ty}", param.name))
        })
        .collect::<CompilerResult<Vec<_>>>()?
        .join(", ");
    let result = function
        .result
        .map(|ty| canonical_wit_type(resolve, ty, &mut Vec::new()))
        .transpose()?
        .unwrap_or_else(|| "_".to_string());
    Ok(format!("func({params}) -> {result}"))
}

/// Returns a structural WIT type signature with aliases resolved.
fn canonical_wit_type(
    resolve: &Resolve,
    ty: Type,
    active: &mut Vec<TypeId>,
) -> CompilerResult<String> {
    let primitive = match ty {
        Type::Bool => Some("bool"),
        Type::U8 => Some("u8"),
        Type::U16 => Some("u16"),
        Type::U32 => Some("u32"),
        Type::U64 => Some("u64"),
        Type::S8 => Some("s8"),
        Type::S16 => Some("s16"),
        Type::S32 => Some("s32"),
        Type::S64 => Some("s64"),
        Type::F32 => Some("f32"),
        Type::F64 => Some("f64"),
        Type::Char => Some("char"),
        Type::String => Some("string"),
        Type::ErrorContext => Some("error-context"),
        Type::Id(_) => None,
    };
    if let Some(primitive) = primitive {
        return Ok(primitive.to_string());
    }
    let Type::Id(id) = ty else {
        unreachable!("all primitive WIT types returned above")
    };
    if active.contains(&id) {
        return Err(Report::msg("recursive types are not valid in the note codec interface"));
    }
    active.push(id);
    let result = match &resolve.types[id].kind {
        TypeDefKind::Type(ty) => canonical_wit_type(resolve, *ty, active),
        TypeDefKind::List(ty) => {
            canonical_wit_type(resolve, *ty, active).map(|ty| format!("list<{ty}>"))
        }
        TypeDefKind::Result(result) => {
            let ok = result
                .ok
                .map(|ty| canonical_wit_type(resolve, ty, active))
                .transpose()?
                .unwrap_or_else(|| "_".to_string());
            let err = result
                .err
                .map(|ty| canonical_wit_type(resolve, ty, active))
                .transpose()?
                .unwrap_or_else(|| "_".to_string());
            Ok(format!("result<{ok}, {err}>"))
        }
        kind => Err(Report::msg(format!(
            "unsupported `{}` type in the note codec interface signature",
            kind.as_str()
        ))),
    };
    active.pop();
    result
}

/// Parse `cargo -Zscript`-style frontmatter from a given input string, if present.
///
/// Returns `Ok(None)` if the input does not define Cargo frontmatter.
///
/// The following are expected of the input:
///
/// * The frontmatter is defined in a Rust module doc, i.e. `//!`
/// * The module doc begins on the first line of the input
/// * The frontmatter block is opened with "```cargo" and closed with "```"
/// * The contents of the frontmatter block must be valid TOML, and should define only
///   the `[dependencies] table
pub fn parse_cargo_frontmatter(
    input: &str,
    working_dir: &Path,
) -> CompilerResult<Option<toml_edit::Table>> {
    let mut cargo_frontmatter = String::new();
    let mut opened = false;
    for line in input.lines() {
        let line = line.trim_start();
        if let Some(line) = line.strip_prefix("//!") {
            let line = line.trim();
            if !opened && line.starts_with("```cargo") {
                opened = true;
            } else if opened && line.starts_with("```") {
                // The end of the frontmatter section has been reached
                break;
            } else if opened {
                cargo_frontmatter.push_str(line);
                cargo_frontmatter.push('\n');
            }
        } else if opened {
            return Err(Report::msg(
                "unclosed Cargo frontmatter block: reached end of module doc before closing ```",
            ));
        }
    }

    if cargo_frontmatter.is_empty() {
        return Ok(None);
    }

    let toml = toml_edit::Document::parse(&cargo_frontmatter).map_err(|err| {
        Report::msg(format!("unable to parse Cargo frontmatter as valid TOML table: {err}"))
    })?;

    let Some(toml) = toml.get("dependencies") else {
        return Err(Report::msg(
            "invalid Cargo frontmatter: expected `[dependencies]` table to be present",
        ));
    };

    let Some(mut dependencies) = toml.as_table().cloned() else {
        return Err(Report::msg(
            "invalid Cargo frontmatter: expected `dependencies` key to be a table",
        ));
    };

    for (k, v) in dependencies.iter_mut() {
        let Some(dep) = v.as_inline_table_mut() else {
            continue;
        };
        let Some(path) = dep.get_mut("path") else {
            continue;
        };
        let toml_edit::Value::String(path) = path else {
            return Err(Report::msg(format!(
                "invalid dependency spec for '{k}': expected 'path' to be a string"
            )));
        };
        let dependency_path = Path::new(path.value());
        if dependency_path.is_absolute() {
            continue;
        }
        let new_path = working_dir.join(dependency_path);
        *path = toml_edit::Formatted::new(new_path.display().to_string());
    }

    Ok(Some(dependencies))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_interface_validation_compares_function_signatures() {
        let (expected_resolve, expected_id) = resolve_codec_interface(NOTE_CODEC_WIT);
        compare_codec_interface_signatures(
            &expected_resolve,
            expected_id,
            &expected_resolve,
            expected_id,
        )
        .unwrap();

        let changed = NOTE_CODEC_WIT.replace(
            "parse: func(type-fqn: string, value: string)",
            "parse: func(type-fqn: string, value: u64)",
        );
        let (actual_resolve, actual_id) = resolve_codec_interface(&changed);
        let error = compare_codec_interface_signatures(
            &actual_resolve,
            actual_id,
            &expected_resolve,
            expected_id,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(".parse` has signature"));
        assert!(error.contains("value: u64"));
        assert!(error.contains("value: string"));
    }

    #[test]
    fn note_codec_staging_is_content_addressed() {
        let root = tempfile::TempDir::new().unwrap();
        let package = midenc_codegen_masm::intrinsics::load();
        let codec_crate = root.path().join("codec");
        fs::create_dir(&codec_crate).unwrap();

        let first = stage_note_package(root.path(), &codec_crate, &package).unwrap();
        let second = stage_note_package(root.path(), &codec_crate, &package).unwrap();
        assert_eq!(first.cache_dir, second.cache_dir);
        assert_eq!(first.build_key, second.build_key);
        assert_eq!(first.build_key.len(), 64);
        let package_path =
            first.cache_dir.join(&*package.name).with_extension(MastPackage::EXTENSION);
        assert_eq!(fs::read(package_path).unwrap(), package.to_bytes());
    }

    #[test]
    fn note_codec_cargo_policy_is_forwarded() {
        let mut cargo = Command::new("cargo");
        cargo.arg("build");
        apply_cargo_policy(&mut cargo, true, true);
        let args = cargo
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(args.iter().any(|arg| arg == "--locked"));
        assert!(args.iter().any(|arg| arg == "--offline"));
    }

    /// Resolves the codec interface from one complete WIT document.
    fn resolve_codec_interface(wit: &str) -> (Resolve, wit_parser::InterfaceId) {
        let mut resolve = Resolve::default();
        let package_id = resolve.push_str("note-codec-test.wit", wit).unwrap();
        let world_id = resolve.packages[package_id].worlds["note-codec"];
        let (_, interface_id) = codec_world_export(&resolve, world_id).unwrap();
        (resolve, interface_id)
    }
}
