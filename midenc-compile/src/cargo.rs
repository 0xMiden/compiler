use core::str::FromStr;
use std::{
    boxed::Box,
    collections::BTreeSet,
    path::PathBuf,
    rc::Rc,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::SourceManager;
use miden_package_registry::PackageStore;
use midenc_hir::Report;
use midenc_session::{InputFile, LinkLibrary, Session, miden_project};

use crate::{CompilerResult, stages::Artifact};

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
            miden_project::ProjectDependencyNodeProvenance::Preassembled { path, selected } => {
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
                        cargo_build(
                            project_package.clone(),
                            target.inner(),
                            manifest_path.with_file_name("Cargo.toml"),
                            registry,
                            options,
                            cargo_opts,
                            source_manager.clone(),
                        )?;
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

fn cargo_build(
    package: Arc<miden_project::Package>,
    target: &miden_project::Target,
    manifest_path: std::path::PathBuf,
    registry: &mut midenc_session::registry::HybridPackageRegistry,
    options: &midenc_session::Options,
    cargo_opts: &CargoOptions,
    source_manager: Arc<dyn SourceManager + Send + Sync>,
) -> CompilerResult<Arc<miden_mast_package::Package>> {
    let package_name = package.name().to_string();
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
        trim_path_prefixes: options.trim_path_prefixes.clone(),
        rustflags: options.rustflags.clone(),
        link_libraries: vec![LinkLibrary::std()],
        ..midenc_session::Options::new(
            Some(package_name.clone()),
            Some(target.ty),
            options.current_dir.clone(),
            options.target_dir.clone(),
            options.output_dir.clone(),
            options.sysroot.clone(),
        )
    });
    if nested_options.target_requires_protocol() {
        nested_options.link_libraries.push(LinkLibrary::base());
    }
    // Inherit release/debug profile from parent build
    if cargo_opts.release {
        nested_options.profile = "release".to_string();
    }

    let package = midenc_session::fixup_targets(package, true);

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

    // We expect dependencies to *always* produce packages (.masp)
    let Artifact::Assembled(package) = crate::cargo_project_pipeline(input, context)? else {
        panic!(
            "expected cargo build of {package_name} to produce assembled artifact, but got HIR \
             output instead",
        );
    };

    registry.publish_package(package.clone())?;

    Ok(package)
}
