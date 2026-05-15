use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use miden_mast_package::TargetType;
use midenc_compile::Compiler;
use midenc_session::{
    SourceManager,
    diagnostics::{DefaultSourceManager, PrintDiagnostic, SourceManagerExt},
    miden_project,
    registry::HybridPackageRegistry,
};
use tempfile::TempDir;

use crate::{BuildOutput, CommandOutput, OutputType, compile_masm, config::CargoPackageSpec};

/// Command-line arguments accepted by `cargo miden build`.
///
/// All arguments following `build` are parsed by the `midenc` compiler's argument parser.
/// Cargo-specific options (`--release`, `--manifest-path`, `--workspace`, `--package`)
/// are recognized and forwarded to the underlying `cargo build` invocation.
/// All other options are passed to `midenc` for compilation.
#[derive(Clone, Debug, Args)]
#[command(disable_version_flag = true, trailing_var_arg = true)]
pub struct BuildCommand {
    /// Arguments parsed by midenc (includes cargo-compatible options).
    #[arg(value_name = "ARG", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl BuildCommand {
    /// Executes `cargo miden build`, returning the resulting command output.
    pub fn exec(self, build_output_type: OutputType) -> Result<Option<CommandOutput>> {
        // Parse all arguments using midenc's Compiler parser.
        // This gives us a structured representation of all options.
        let cwd = std::env::current_dir()?;
        let compiler_opts = Compiler::try_parse_from(cwd, &self.args).map_err(|e| {
            // Render the clap error with full formatting (colors, suggestions, etc.)
            anyhow::anyhow!("failed to parse 'cargo miden build' arguments: {}", e.render())
        })?;

        Self::exec_from_options(compiler_opts, None, build_output_type)
    }

    /// Executes `cargo miden build` with the provided compiler options and package registry
    pub fn exec_from_options(
        mut compiler_opts: Box<midenc_session::Options>,
        registry: Option<&mut HybridPackageRegistry>,
        build_output_type: OutputType,
    ) -> Result<Option<CommandOutput>> {
        // Extract cargo-specific options from parsed Compiler struct
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
                    bail!(
                        "unable to locate project manifest: --manifest-path specifies a path with \
                         no parent"
                    )
                };
                let manifest_path = project_dir.join("miden-project.toml");
                let project_dir = project_dir.to_path_buf();
                *cargo_manifest_path = manifest_path.clone();
                (project_dir, manifest_path)
            }
            None => {
                let Ok(cwd) = std::env::current_dir() else {
                    bail!(
                        "unable to locate project manifest: current working directory is \
                         unavailable"
                    )
                };
                let manifest_path = cwd.join("miden-project.toml");
                compiler_opts.manifest_path = Some(manifest_path.clone());
                (cwd, manifest_path)
            }
        };

        let source_manager = Arc::new(DefaultSourceManager::default()) as Arc<dyn SourceManager>;
        let outputs = if compiler_opts.workspace {
            let source = source_manager.load_file(&project_manifest_path)?;
            let workspace = miden_project::Workspace::load(source, &source_manager)
                .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;
            Self::build_workspace(
                &workspace,
                build_output_type,
                project_dir,
                compiler_opts,
                &cargo_opts,
                None,
                source_manager,
            )?
        } else if !compiler_opts.packages.is_empty() {
            let source = source_manager.load_file(&project_manifest_path)?;
            // Check if the project manifest is a workspace manifest - this requires us to build
            // the entire workspace, rather than a single project
            if let miden_project::ast::MidenProject::Workspace(_) =
                miden_project::ast::MidenProject::parse(source.clone())
                    .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?
            {
                let workspace = miden_project::Workspace::load(source, &source_manager)
                    .map(Arc::<miden_project::Workspace>::from)
                    .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;
                let mut outputs = Vec::new();
                for requested in compiler_opts.packages.iter() {
                    let Some(package) = workspace.get_member_by_name(requested) else {
                        bail!("requested pacakge '{requested}' is not a valid workspace member");
                    };
                    let output = Self::build_project(
                        miden_project::Project::WorkspacePackage {
                            package,
                            workspace: workspace.clone(),
                        },
                        compiler_opts.target_type,
                        build_output_type,
                        compiler_opts.clone(),
                        &cargo_opts,
                        None,
                        Arc::clone(&source_manager),
                    )?;
                    outputs.push(output);
                }
                outputs
            } else {
                let project = miden_project::Project::load(&project_manifest_path, &source_manager)
                    .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;
                let output = Self::build_project(
                    project,
                    compiler_opts.target_type,
                    build_output_type,
                    compiler_opts,
                    &cargo_opts,
                    registry,
                    source_manager,
                )?;
                vec![output]
            }
        } else {
            todo!()
        };

        Ok(Some(CommandOutput::BuildCommandOutput { output: outputs }))
    }

    fn build_workspace(
        workspace: &miden_project::Workspace,
        _build_output_type: OutputType,
        _cwd: PathBuf,
        _compiler_opts: Box<midenc_session::Options>,
        _cargo_opts: &CargoOptions,
        _registry: Option<&mut HybridPackageRegistry>,
        _source_manager: Arc<dyn SourceManager>,
    ) -> Result<Vec<BuildOutput>> {
        //let metadata = load_metadata(cargo_opts.manifest_path.as_deref())?;

        //let mut packages =
        //   load_component_metadata(&metadata, cargo_opts.packages.iter(), cargo_opts.workspace)?;

        if workspace.members().is_empty() {
            bail!(
                "workspace ({}) contains no members",
                workspace.manifest_path().unwrap_or(Path::new("virtual")).display()
            );
        }

        todo!("build a dependency graph of the workspace members and build each package")
    }

    fn build_project(
        project: miden_project::Project,
        target_type: Option<TargetType>,
        build_output_type: OutputType,
        mut compiler_opts: Box<midenc_session::Options>,
        cargo_opts: &CargoOptions,
        registry: Option<&mut HybridPackageRegistry>,
        source_manager: Arc<dyn SourceManager>,
    ) -> Result<BuildOutput> {
        let package = project.package();
        let target_type = match target_type {
            None => {
                if let Some(target) = package.library_target() {
                    target.ty
                } else if package.executable_targets().len() > 1 {
                    bail!(
                        "cannot build project '{}': multiple targets are present and --target \
                         wasn't provided to disambiguate",
                        package.name()
                    );
                } else {
                    TargetType::Executable
                }
            }
            Some(ty) => ty,
        };

        let tmp = TempDir::new()?;
        let mut default_registry =
            midenc_session::registry::HybridPackageRegistry::new(compiler_opts.as_ref())
                .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;
        let registry = registry.unwrap_or(&mut default_registry);
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
        let dependency_graph = dependency_graph
            .build(package.clone())
            .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;

        crate::dependencies::load_cargo_based_source_dependencies(
            &package,
            &dependency_graph,
            registry,
            &compiler_opts,
            cargo_opts,
            &source_manager,
        )?;

        let cargo_build_args = build_cargo_args(cargo_opts);

        // Enable memcopy and 128-bit arithmetic ops
        let mut extra_rust_flags = String::from("-C target-feature=+bulk-memory,+wide-arithmetic");
        // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it for
        // cfg-based dependency selection.
        extra_rust_flags.push_str(" --cfg miden");
        // Enable errors on missing stub functions
        extra_rust_flags.push_str(" -C link-args=--fatal-warnings");
        // Remove the source file paths in the data segment for panics
        // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
        extra_rust_flags.push_str(" -Zlocation-detail=none");
        // Build with panic=immediate-abort
        extra_rust_flags.push_str(" -Zunstable-options");
        extra_rust_flags.push_str(" -Cpanic=immediate-abort");
        if let Ok(inherited) = std::env::var("RUSTFLAGS")
            && !inherited.is_empty()
        {
            extra_rust_flags.push(' ');
            extra_rust_flags.push_str(&inherited);
        }

        let wasi = if compiler_opts.target_requires_protocol() {
            "wasip2"
        } else {
            "wasip1"
        };

        let wasm_outputs = run_cargo(wasi, &cargo_build_args, [("RUSTFLAGS", extra_rust_flags)])?;

        assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
        let wasm_output = wasm_outputs.first().expect("expected at least one Wasm artifact");

        // Set midenc flags from target environment defaults
        modify_midenc_options_for_target(&mut compiler_opts, target_type, wasm_output)?;

        // When debug info is enabled, automatically add -Ztrim-path-prefix to normalize
        // source paths in debug information.
        let package_source_dir = package
            .manifest_path()
            .expect("expected package to have an on-disk manifest")
            .parent();
        if compiler_opts.debug != midenc_session::DebugInfo::None
            && let Some(source_dir) = package_source_dir
        {
            compiler_opts.trim_path_prefixes.push(source_dir.to_path_buf());
        }

        match build_output_type {
            OutputType::Wasm => Ok(BuildOutput::Wasm {
                artifact_path: wasm_output.clone(),
                options: compiler_opts,
            }),
            OutputType::Masm => {
                let metadata_out_dir =
                    compiler_opts.target_dir.join("miden").join(if cargo_opts.release {
                        "release"
                    } else {
                        "debug"
                    });
                if !metadata_out_dir.exists() {
                    std::fs::create_dir_all(&metadata_out_dir)?;
                }

                let output =
                    compile_masm::wasm_to_masm(wasm_output, &metadata_out_dir, compiler_opts)
                        .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;

                Ok(BuildOutput::Masm {
                    artifact_path: output,
                })
            }
        }
    }
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
}

impl CargoOptions {
    /// Extract cargo-specific options from a Compiler struct.
    fn from_compiler(options: &midenc_session::Options) -> Result<Self> {
        let packages = options
            .packages
            .iter()
            .map(|s| {
                CargoPackageSpec::new(s.clone())
                    .with_context(|| format!("invalid package spec '{s}'"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            release: options.profile == "release",
            manifest_path: options.manifest_path.clone(),
            workspace: options.workspace,
            packages,
        })
    }
}

/// Builds the argument vector for the underlying `cargo build` invocation.
fn build_cargo_args(cargo_opts: &CargoOptions) -> Vec<String> {
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

fn run_cargo<E>(wasi: &str, spawn_args: &[String], env: E) -> Result<Vec<PathBuf>>
where
    E: IntoIterator<Item = (&'static str, String)>,
{
    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut cargo = Command::new(&cargo_path);
    cargo.envs(env);
    // This env var is used by crates (e.g. `miden-field`) to distinguish compiling to Wasm for a
    // "real" Wasm runtime vs compiling to Wasm as an intermediate artifact that will be compiled
    // to Miden VM code by `midenc`.
    cargo.env("MIDENC_TARGET_IS_MIDEN_VM", "1");
    cargo.args(spawn_args);

    // Handle the target for buildable commands
    midenc_compile::rust::install_wasm32_target(wasi, None).map_err(|err| anyhow!("{err}"))?;

    cargo.arg("--target").arg(format!("wasm32-{wasi}"));

    // It will output the message as json so we can extract the wasm files
    // that will be componentized
    cargo.arg("--message-format").arg("json-render-diagnostics");
    cargo.stdout(Stdio::piped());

    let artifacts =
        midenc_compile::rust::spawn_cargo(cargo, &cargo_path).map_err(|err| anyhow!("{err}"))?;

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

/// Produces the `midenc` CLI flags implied by the detected target environment and project type.
fn modify_midenc_options_for_target(
    options: &mut midenc_session::Options,
    target_type: TargetType,
    wasm_output: &Path,
) -> Result<()> {
    options.target_type = Some(target_type);
    match target_type {
        TargetType::Executable => {
            let masm_module_name = wasm_output
                .file_stem()
                .expect("invalid wasm file path: no file stem")
                .to_str()
                .unwrap();
            options.entrypoint = Some(format!("{masm_module_name}::entrypoint"));
        }
        TargetType::Kernel => {
            bail!("kernels are not currently supported via midenc")
        }
        TargetType::Library | TargetType::AccountComponent | TargetType::Note => (),
        TargetType::TransactionScript => {
            options.entrypoint = Some("miden:base/transaction-script@1.0.0::run".to_string());
        }
        _ => bail!("unsupported --target-type: {target_type}"),
    }
    Ok(())
}
