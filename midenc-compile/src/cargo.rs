use core::str::FromStr;
use std::{
    boxed::Box,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::SourceManager;
use miden_mast_package::Package as MastPackage;
use midenc_hir::Report;
use midenc_session::{InputFile, LinkLibrary, Session, miden_project};
use wit_component::{ComponentEncoder, DecodedWasm};
use wit_parser::WorldItem;

use crate::{CodegenOutput, CompilerResult};

/// Metadata table that points to an author-side note codec crate.
const NOTE_CODEC_CRATE_METADATA: &str = "note-codec-crate";

/// Metadata field that contains the codec crate directory.
const NOTE_CODEC_CRATE_PATH: &str = "path";

/// Rust target used for zero-import note codec components.
const NOTE_CODEC_TARGET: &str = "wasm32-unknown-unknown";

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

/// Builds the optional note codec component declared by a project package.
pub(crate) fn build_project_note_codec(
    project_package: &miden_project::Package,
    project_manifest_path: &Path,
    note_project_dir: &Path,
    note_package: &MastPackage,
) -> CompilerResult<Option<Vec<u8>>> {
    let Some(codec_crate_dir) =
        note_codec_crate_dir(project_package.metadata(), project_manifest_path, note_project_dir)?
    else {
        return Ok(None);
    };

    build_note_codec_component(&codec_crate_dir, note_project_dir, note_package).map(Some)
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
    note_project_dir: &Path,
    note_package: &MastPackage,
) -> CompilerResult<Vec<u8>> {
    let _staged_package = stage_note_package(note_project_dir, note_package)?;
    let manifest_path = codec_crate_dir.join("Cargo.toml");
    let artifact_name = codec_artifact_name(&manifest_path)?;
    let target_dir = codec_crate_dir.join("target");
    let wasm_path = target_dir
        .join(NOTE_CODEC_TARGET)
        .join("release")
        .join(artifact_name)
        .with_extension("wasm");
    // Cargo does not track a package file that a procedural macro reads. Remove the final output
    // so the codec crate expands against the package staged above.
    match fs::remove_file(&wasm_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Report::msg(format!(
                "failed to prepare note codec output '{}': {error}",
                wasm_path.display()
            )));
        }
    }
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(codec_crate_dir)
        .args([
            "build",
            "--manifest-path",
            manifest_path.to_str().ok_or_else(|| {
                Report::msg(format!(
                    "codec manifest path '{}' is not UTF-8",
                    manifest_path.display()
                ))
            })?,
            "--lib",
            "--release",
            "--target",
            NOTE_CODEC_TARGET,
            "--target-dir",
            target_dir.to_str().ok_or_else(|| {
                Report::msg(format!("codec target path '{}' is not UTF-8", target_dir.display()))
            })?,
        ])
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .map_err(|error| {
            Report::msg(format!(
                "failed to start `cargo build` for note codec crate '{}': {error}",
                codec_crate_dir.display()
            ))
        })?;
    if !output.status.success() {
        return Err(Report::msg(format!(
            "failed to build note codec crate '{}' for {NOTE_CODEC_TARGET} in release mode \
             (install the target with `rustup target add {NOTE_CODEC_TARGET}` if \
             needed)\nstdout:\n{}\nstderr:\n{}",
            codec_crate_dir.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )));
    }

    let module = fs::read(&wasm_path).map_err(|error| {
        Report::msg(format!(
            "note codec build succeeded but did not produce the expected cdylib '{}': {error}",
            wasm_path.display()
        ))
    })?;
    let component = ComponentEncoder::default()
        .module(&module)
        .map_err(|error| {
            Report::msg(format!(
                "note codec module '{}' is not component-ready: {error}",
                wasm_path.display()
            ))
        })?
        .validate(true)
        .encode()
        .map_err(|error| {
            Report::msg(format!(
                "failed to encode note codec module '{}' as a component: {error}",
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

/// Stages the current in-memory note package for `from_project!` during the codec build.
fn stage_note_package(
    note_project_dir: &Path,
    note_package: &MastPackage,
) -> CompilerResult<tempfile::TempDir> {
    let profiles_dir = note_project_dir.join("target/miden");
    fs::create_dir_all(&profiles_dir).map_err(|error| {
        Report::msg(format!(
            "failed to create note package staging root '{}': {error}",
            profiles_dir.display()
        ))
    })?;
    let staging_dir = tempfile::Builder::new()
        .prefix("zz-note-codec-input-")
        .tempdir_in(&profiles_dir)
        .map_err(|error| {
            Report::msg(format!(
                "failed to create note package staging directory in '{}': {error}",
                profiles_dir.display()
            ))
        })?;
    note_package.write_masp_file(staging_dir.path()).map_err(|error| {
        Report::msg(format!(
            "failed to stage note package {}@{} for codec generation: {error}",
            note_package.name, note_package.version
        ))
    })?;
    Ok(staging_dir)
}

/// Returns the expected Wasm artifact name and checks the cdylib configuration.
fn codec_artifact_name(manifest_path: &Path) -> CompilerResult<String> {
    let source = fs::read_to_string(manifest_path).map_err(|error| {
        Report::msg(format!(
            "note codec crate has no readable manifest at '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let manifest = source.parse::<toml_edit::DocumentMut>().map_err(|error| {
        Report::msg(format!(
            "failed to parse note codec manifest '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let package =
        manifest
            .get("package")
            .and_then(toml_edit::Item::as_table_like)
            .ok_or_else(|| {
                Report::msg(format!(
                    "codec manifest '{}' has no `[package]` table",
                    manifest_path.display()
                ))
            })?;
    let package_name = package.get("name").and_then(toml_edit::Item::as_str).ok_or_else(|| {
        Report::msg(format!("codec manifest '{}' has no package name", manifest_path.display()))
    })?;
    let lib = manifest.get("lib").and_then(toml_edit::Item::as_table_like).ok_or_else(|| {
        Report::msg(format!("codec manifest '{}' has no `[lib]` table", manifest_path.display()))
    })?;
    let is_cdylib =
        lib.get("crate-type")
            .and_then(toml_edit::Item::as_array)
            .is_some_and(|crate_types| {
                crate_types.iter().any(|crate_type| crate_type.as_str() == Some("cdylib"))
            });
    if !is_cdylib {
        return Err(Report::msg(format!(
            "note codec manifest '{}' must set `[lib] crate-type = [\"cdylib\"]`",
            manifest_path.display()
        )));
    }
    let lib_name = lib.get("name").and_then(toml_edit::Item::as_str).unwrap_or(package_name);
    Ok(lib_name.replace('-', "_"))
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
    if !world.imports.is_empty() {
        return Err(Report::msg(format!(
            "note codec component must have zero imports, found: {:#?}",
            world.imports
        )));
    }
    if world.exports.len() != 1 {
        return Err(Report::msg(format!(
            "note codec component must export only `miden:note-codec/codec@1.0.0`, found: {:#?}",
            world.exports
        )));
    }

    let interface = world.exports.values().find_map(|item| {
        let WorldItem::Interface { id, .. } = item else {
            return None;
        };
        let interface = &resolve.interfaces[*id];
        let package_id = interface.package?;
        let package = &resolve.packages[package_id].name;
        (interface.name.as_deref() == Some("codec")
            && package.namespace == "miden"
            && package.name == "note-codec"
            && package.version.as_ref().is_some_and(|version| version.to_string() == "1.0.0"))
        .then_some(interface)
    });
    let interface = interface
        .ok_or_else(|| Report::msg("component does not export `miden:note-codec/codec@1.0.0`"))?;
    for function in ["supported-types", "parse", "display", "validate"] {
        if !interface.functions.contains_key(function) {
            return Err(Report::msg(format!(
                "`miden:note-codec/codec@1.0.0` is missing `{function}`"
            )));
        }
    }
    Ok(())
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
