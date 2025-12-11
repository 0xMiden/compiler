use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use cargo_metadata::{camino, Artifact, Message, Metadata, MetadataCommand, Package};
use clap::Args;
use midenc_compile::Compiler;
use midenc_session::TargetEnv;
use path_absolutize::Absolutize;

use crate::{
    compile_masm,
    config::CargoPackageSpec,
    dependencies::process_miden_dependencies,
    target::{self, install_wasm32_target},
    BuildOutput, CommandOutput, OutputType,
};

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
        let compiler_opts = Compiler::try_parse_from(&self.args).map_err(|e| {
            // Render the clap error with full formatting (colors, suggestions, etc.)
            anyhow::anyhow!("failed to parse 'cargo miden build' arguments: {}", e.render())
        })?;

        // Extract cargo-specific options from parsed Compiler struct
        let cargo_opts = CargoOptions::from_compiler(&compiler_opts)?;

        let metadata = load_metadata(cargo_opts.manifest_path.as_deref())?;

        let mut packages =
            load_component_metadata(&metadata, cargo_opts.packages.iter(), cargo_opts.workspace)?;

        if packages.is_empty() {
            bail!(
                "manifest `{path}` contains no package or the workspace has no members",
                path = metadata.workspace_root.join("Cargo.toml")
            );
        }

        let cargo_package = determine_cargo_package(&metadata, &cargo_opts)?;

        let target_env = target::detect_target_environment(cargo_package)?;
        let project_type = target::target_environment_to_project_type(target_env);

        if !packages.iter().any(|p| p.package.id == cargo_package.id) {
            packages.push(PackageComponentMetadata::new(cargo_package)?);
        }

        let dependency_packages_paths = process_miden_dependencies(cargo_package, &cargo_opts)?;

        let spawn_args = build_cargo_args(&cargo_opts);

        // Enable memcopy and 128-bit arithmetic ops
        let mut extra_rust_flags = String::from("-C target-feature=+bulk-memory,+wide-arithmetic");
        // Enable errors on missing stub functions
        extra_rust_flags.push_str(" -C link-args=--fatal-warnings");
        // Remove the source file paths in the data segment for panics
        // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
        extra_rust_flags.push_str(" -Zlocation-detail=none");
        let maybe_old_rustflags = match std::env::var("RUSTFLAGS") {
            Ok(current) if !current.is_empty() => {
                std::env::set_var("RUSTFLAGS", format!("{current} {extra_rust_flags}"));
                Some(current)
            }
            _ => {
                std::env::set_var("RUSTFLAGS", extra_rust_flags);
                None
            }
        };

        let wasi = match target_env {
            TargetEnv::Rollup { .. } => "wasip2",
            _ => "wasip1",
        };

        let wasm_outputs = run_cargo(wasi, &spawn_args)?;

        if let Some(old_rustflags) = maybe_old_rustflags {
            std::env::set_var("RUSTFLAGS", old_rustflags);
        } else {
            std::env::remove_var("RUSTFLAGS");
        }

        assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
        let wasm_output = wasm_outputs.first().expect("expected at least one Wasm artifact");

        // Build midenc flags from target environment defaults
        let mut midenc_flags = midenc_flags_from_target(target_env, project_type, wasm_output);

        // Add dependency library paths
        for dep_path in dependency_packages_paths {
            midenc_flags.push("--link-library".to_string());
            midenc_flags.push(dep_path.to_string_lossy().to_string());
        }

        // Merge user-provided midenc options from parsed Compiler struct
        // User options override target-derived defaults
        midenc_flags = merge_midenc_flags(midenc_flags, &compiler_opts);

        match build_output_type {
            OutputType::Wasm => Ok(Some(CommandOutput::BuildCommandOutput {
                output: BuildOutput::Wasm {
                    artifact_path: wasm_output.clone(),
                    midenc_flags,
                },
            })),
            OutputType::Masm => {
                let metadata_out_dir =
                    metadata.target_directory.join("miden").join(if cargo_opts.release {
                        "release"
                    } else {
                        "debug"
                    });
                if !metadata_out_dir.exists() {
                    std::fs::create_dir_all(&metadata_out_dir)?;
                }

                let output = compile_masm::wasm_to_masm(
                    wasm_output,
                    metadata_out_dir.as_std_path(),
                    midenc_flags,
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;

                Ok(Some(CommandOutput::BuildCommandOutput {
                    output: BuildOutput::Masm {
                        artifact_path: output,
                    },
                }))
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
    fn from_compiler(compiler: &Compiler) -> Result<Self> {
        let packages = compiler
            .package
            .iter()
            .map(|s| {
                CargoPackageSpec::new(s.clone())
                    .with_context(|| format!("invalid package spec '{s}'"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            release: compiler.release,
            manifest_path: compiler.manifest_path.clone(),
            workspace: compiler.workspace,
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
            "build-std=std,core,alloc,panic_abort",
            "-Z",
            "build-std-features=panic_immediate_abort",
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
        ("profile.release.opt-level", "\"z\""),
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

/// Merges user-provided `--emit` option with target-derived defaults.
///
/// Only the `--emit` option is merged from user input. All other options are
/// determined by the detected target environment and project type.
fn merge_midenc_flags(mut base: Vec<String>, compiler: &Compiler) -> Vec<String> {
    // Only merge --emit options from user input
    for spec in &compiler.output_types {
        base.push("--emit".to_string());
        let spec_str = match spec {
            midenc_session::OutputTypeSpec::All { path } => {
                if let Some(p) = path {
                    format!("all={p}")
                } else {
                    "all".to_string()
                }
            }
            midenc_session::OutputTypeSpec::Typed { output_type, path } => {
                if let Some(p) = path {
                    format!("{output_type}={p}")
                } else {
                    output_type.to_string()
                }
            }
        };
        base.push(spec_str);
    }

    base
}

fn run_cargo(wasi: &str, spawn_args: &[String]) -> Result<Vec<PathBuf>> {
    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut cargo = Command::new(&cargo_path);
    cargo.args(spawn_args);

    // Handle the target for buildable commands
    install_wasm32_target(wasi)?;

    cargo.arg("--target").arg(format!("wasm32-{wasi}"));

    // It will output the message as json so we can extract the wasm files
    // that will be componentized
    cargo.arg("--message-format").arg("json-render-diagnostics");
    cargo.stdout(Stdio::piped());

    let artifacts = spawn_cargo(cargo, &cargo_path)?;

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

pub(crate) fn spawn_cargo(mut cmd: Command, cargo: &Path) -> Result<Vec<Artifact>> {
    log::debug!("spawning command {cmd:?}");

    let mut child = cmd
        .spawn()
        .context(format!("failed to spawn `{cargo}`", cargo = cargo.display()))?;

    let mut artifacts = Vec::new();
    let stdout = child.stdout.take().expect("no stdout");
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.context("failed to read output from `cargo`")?;

        if line.is_empty() {
            continue;
        }

        for message in Message::parse_stream(line.as_bytes()) {
            if let Message::CompilerArtifact(artifact) =
                message.context("unexpected JSON message from cargo")?
            {
                for path in &artifact.filenames {
                    match path.extension() {
                        Some("wasm") => {
                            artifacts.push(artifact);
                            break;
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    let status = child
        .wait()
        .context(format!("failed to wait for `{cargo}` to finish", cargo = cargo.display()))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(artifacts)
}

fn determine_cargo_package<'a>(
    metadata: &'a cargo_metadata::Metadata,
    cargo_opts: &CargoOptions,
) -> Result<&'a cargo_metadata::Package> {
    let package = if let Some(manifest_path) = cargo_opts.manifest_path.as_deref() {
        let mp_utf8 = camino::Utf8Path::from_path(manifest_path).ok_or_else(|| {
            anyhow::anyhow!("manifest path is not valid UTF-8: {}", manifest_path.display())
        })?;
        let mp_abs = mp_utf8
            .as_std_path()
            .absolutize()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to absolutize manifest path {}: {e}",
                    manifest_path.display()
                )
            })?
            .into_owned();
        metadata
            .packages
            .iter()
            .find(|p| p.manifest_path.as_std_path() == mp_abs.as_path())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unable to determine package: manifest `{}` does not match any workspace \
                     package",
                    manifest_path.display()
                )
            })?
    } else {
        let cwd = std::env::current_dir()?;
        metadata
            .packages
            .iter()
            .find(|p| p.manifest_path.parent().map(|d| d.as_std_path()) == Some(cwd.as_path()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unable to determine package; run inside a member directory or pass `-p \
                     <name>` / `--manifest-path <path>`"
                )
            })?
    };
    Ok(package)
}

/// Produces the `midenc` CLI flags implied by the detected target environment and project type.
fn midenc_flags_from_target(
    target_env: TargetEnv,
    project_type: midenc_session::ProjectType,
    wasm_output: &Path,
) -> Vec<String> {
    let mut midenc_args = Vec::new();

    match target_env {
        TargetEnv::Base | TargetEnv::Emu => match project_type {
            midenc_session::ProjectType::Program => {
                midenc_args.push("--exe".into());
                let masm_module_name = wasm_output
                    .file_stem()
                    .expect("invalid wasm file path: no file stem")
                    .to_str()
                    .unwrap();
                let entrypoint_opt = format!("--entrypoint={masm_module_name}::entrypoint");
                midenc_args.push(entrypoint_opt);
            }
            midenc_session::ProjectType::Library => midenc_args.push("--lib".into()),
        },
        TargetEnv::Rollup { target } => {
            midenc_args.push("--target".into());
            match target {
                midenc_session::RollupTarget::Account => {
                    midenc_args.push("rollup:account".into());
                    midenc_args.push("--lib".into());
                }
                midenc_session::RollupTarget::NoteScript => {
                    midenc_args.push("rollup:note-script".into());
                    midenc_args.push("--exe".into());
                    midenc_args.push("--entrypoint=miden:base/note-script@1.0.0::run".to_string())
                }
                midenc_session::RollupTarget::TransactionScript => {
                    midenc_args.push("rollup:transaction-script".into());
                    midenc_args.push("--exe".into());
                    midenc_args
                        .push("--entrypoint=miden:base/transaction-script@1.0.0::run".to_string())
                }
                midenc_session::RollupTarget::AuthComponent => {
                    midenc_args.push("rollup:authentication-component".into());
                    midenc_args.push("--lib".into());
                }
            }
        }
    }
    midenc_args
}

/// Loads the workspace metadata based on the given manifest path.
fn load_metadata(manifest_path: Option<&Path>) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command.no_deps();

    if let Some(path) = manifest_path {
        log::debug!("loading metadata from manifest `{path}`", path = path.display());
        command.manifest_path(path);
    } else {
        log::debug!("loading metadata from current directory");
    }

    command.exec().context("failed to load cargo metadata")
}

/// Loads the component metadata for the given package specs.
///
/// If `workspace` is true, all workspace packages are loaded.
fn load_component_metadata<'a>(
    metadata: &'a Metadata,
    specs: impl ExactSizeIterator<Item = &'a CargoPackageSpec>,
    workspace: bool,
) -> Result<Vec<PackageComponentMetadata<'a>>> {
    let pkgs = if workspace {
        metadata.workspace_packages()
    } else if specs.len() > 0 {
        let mut pkgs = Vec::with_capacity(specs.len());
        for spec in specs {
            let pkg = metadata
                .packages
                .iter()
                .find(|p| {
                    p.name == spec.name
                        && match spec.version.as_ref() {
                            Some(v) => &p.version == v,
                            None => true,
                        }
                })
                .with_context(|| {
                    format!("package ID specification `{spec}` did not match any packages")
                })?;
            pkgs.push(pkg);
        }

        pkgs
    } else {
        metadata.workspace_default_packages()
    };

    pkgs.into_iter().map(PackageComponentMetadata::new).collect::<Result<_>>()
}

/// Represents a cargo package paired with its component metadata.
#[derive(Debug)]
pub struct PackageComponentMetadata<'a> {
    /// The cargo package.
    pub package: &'a Package,
}

impl<'a> PackageComponentMetadata<'a> {
    /// Creates a new package metadata from the given package.
    pub fn new(package: &'a Package) -> Result<Self> {
        Ok(Self { package })
    }
}
