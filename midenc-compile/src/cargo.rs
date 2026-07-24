use core::str::FromStr;
use std::{
    boxed::Box,
    path::{Path, PathBuf},
    rc::Rc,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::SourceManager;
use midenc_hir::Report;
use midenc_session::{InputFile, LinkLibrary, Session, miden_project};

use crate::{CodegenOutput, CompilerResult};

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

pub(crate) fn cargo_build(
    package: Arc<miden_project::Package>,
    target: &miden_project::Target,
    manifest_path: std::path::PathBuf,
    filesystem_cache_dir: Option<&std::path::Path>,
    options: &midenc_session::Options,
    cargo_opts: &CargoOptions,
    source_manager: Arc<dyn SourceManager>,
) -> CompilerResult<CodegenOutput> {
    let package_name = package.name().to_string();
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

    let package = if target.ty.is_executable() {
        midenc_session::fixup_cargo_target(package)
    } else {
        package
    };

    let input = InputFile::from_path(manifest_path).unwrap();
    let session = Rc::new(Session::new_project(
        package_name.clone(),
        Some(input.clone()),
        miden_project::Project::Package(package),
        nested_options,
        None,
        source_manager,
    ));
    let context = Rc::new(midenc_hir::Context::new(session));

    crate::cargo_project_codegen_pipeline(input, filesystem_cache_dir, context)
    // We expect dependencies to *always* produce packages (.masp)
    /*
    let CodegenOutput {
        component,
        account_component_metadata_bytes,
    } = crate::cargo_project_codegen_pipeline(input, context.clone())?
    else {
        panic!(
            "expected cargo build of {package_name} to produce component, but got HIR output \
             instead",
        );
    };

    Ok(CodegenOutput {
        component,
        account_component_metadata_bytes,
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
    package.write_masp_file(&masp_out_dir).map_err(|err| {
        Report::msg(format!(
            "failed to materialize dependency package '{package_name}' to '{}': {err}",
            masp_out_dir.display()
        ))
    })?;

    Ok(package)
     */
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
