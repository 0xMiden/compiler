use std::path::Path;

use anyhow::{bail, Result};
use cargo_metadata::{camino, Metadata};
use clap::Args;
use path_absolutize::Absolutize;

use crate::{
    cargo_component::{
        config::{CargoArguments, Config},
        load_component_metadata, load_metadata, run_cargo_command, PackageComponentMetadata,
    },
    compile_masm,
    dependencies::process_miden_dependencies,
    non_component::run_cargo_command_for_non_component,
    target, BuildOutput, CommandOutput, OutputType, Terminal, Verbosity,
};

/// Command-line arguments accepted by `cargo miden build`.
///
/// We capture all tokens following the `build` subcommand so that the build pipeline can
/// interpret them and forward the appropriate options to Cargo.
#[derive(Clone, Debug, Args)]
#[command(disable_version_flag = true, trailing_var_arg = true)]
pub struct BuildCommand {
    /// Additional arguments forwarded to the underlying Cargo invocation.
    #[arg(value_name = "CARGO_ARG", allow_hyphen_values = true)]
    pub cargo_args: Vec<String>,
}

impl BuildCommand {
    /// Executes `cargo miden build`, returning the resulting command output.
    pub fn exec(self, build_output_type: OutputType) -> Result<Option<CommandOutput>> {
        let mut invocation = Vec::with_capacity(self.cargo_args.len() + 1);
        invocation.push("build".to_string());
        invocation.extend(self.cargo_args);

        let cargo_args = CargoArguments::parse_from(invocation.clone().into_iter())?;
        let metadata = load_metadata(cargo_args.manifest_path.as_deref())?;

        if is_workspace_root_context(&metadata, cargo_args.manifest_path.as_deref())
            && cargo_args.packages.is_empty()
            && !cargo_args.workspace
        {
            bail!(
                "You're running `cargo miden` from a Cargo workspace root. Building the entire \
                 workspace is not supported yet. Build a single member instead, for example:\n  - \
                 cd <member>/ && cargo miden build --release"
            );
        }

        let mut packages =
            load_component_metadata(&metadata, cargo_args.packages.iter(), cargo_args.workspace)?;

        if packages.is_empty() {
            bail!(
                "manifest `{path}` contains no package or the workspace has no members",
                path = metadata.workspace_root.join("Cargo.toml")
            );
        }

        let root_package = determine_root_package(&metadata, &cargo_args)?;

        let target_env = target::detect_target_environment(root_package)?;
        let project_type = target::target_environment_to_project_type(target_env);

        if !packages.iter().any(|p| p.package.id == root_package.id) {
            packages.push(PackageComponentMetadata::new(root_package)?);
        }

        let dependency_packages_paths = process_miden_dependencies(root_package, &cargo_args)?;

        let mut spawn_args: Vec<_> = invocation.clone();
        spawn_args.extend_from_slice(
            &[
                "-Z",
                "build-std=std,core,alloc,panic_abort",
                "-Z",
                "build-std-features=panic_immediate_abort",
            ]
            .map(|s| s.to_string()),
        );

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
            spawn_args.push("--config".to_string());
            spawn_args.push(format!("{key}={value}"));
        }

        let extra_rust_flags = String::from(
            "-C target-feature=+bulk-memory,+wide-arithmetic -C link-args=--fatal-warnings",
        );
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

        let terminal = Terminal::new(
            if cargo_args.quiet {
                Verbosity::Quiet
            } else {
                match cargo_args.verbose {
                    0 => Verbosity::Normal,
                    _ => Verbosity::Verbose,
                }
            },
            cargo_args.color.unwrap_or_default(),
        );
        let mut builder = tokio::runtime::Builder::new_current_thread();
        let rt = builder.enable_all().build()?;
        let wasm_outputs = if matches!(target_env, midenc_session::TargetEnv::Rollup { .. }) {
            rt.block_on(async {
                let config = Config::new(terminal).await?;
                let wasm_outputs_res =
                    run_cargo_command(&config, Some("build"), &cargo_args, &spawn_args).await;

                if let Err(e) = wasm_outputs_res.as_ref() {
                    config.terminal().error(format!("{e:?}"))?;
                    std::process::exit(1);
                };
                wasm_outputs_res
            })?
        } else {
            run_cargo_command_for_non_component(Some("build"), &cargo_args, &spawn_args)?
        };

        if let Some(old_rustflags) = maybe_old_rustflags {
            std::env::set_var("RUSTFLAGS", old_rustflags);
        } else {
            std::env::remove_var("RUSTFLAGS");
        }

        assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
        let wasm_output = wasm_outputs.first().expect("expected at least one Wasm artifact");

        let mut midenc_flags = midenc_flags_from_target(target_env, project_type, wasm_output);

        for dep_path in dependency_packages_paths {
            midenc_flags.push("--link-library".to_string());
            midenc_flags.push(dep_path.to_string_lossy().to_string());
        }

        match build_output_type {
            OutputType::Wasm => Ok(Some(CommandOutput::BuildCommandOutput {
                output: BuildOutput::Wasm {
                    artifact_path: wasm_output.clone(),
                    midenc_flags,
                },
            })),
            OutputType::Masm => {
                let metadata_out_dir =
                    metadata.target_directory.join("miden").join(if cargo_args.release {
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

fn determine_root_package<'a>(
    metadata: &'a cargo_metadata::Metadata,
    cargo_args: &CargoArguments,
) -> Result<&'a cargo_metadata::Package> {
    Ok(match metadata.root_package() {
        Some(pkg) => pkg,
        None => {
            if let Some(manifest_path) = cargo_args.manifest_path.as_deref() {
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
                            "unable to determine root package: manifest `{}` does not match any \
                             workspace package",
                            manifest_path.display()
                        )
                    })?
            } else {
                let cwd = std::env::current_dir()?;
                metadata
                    .packages
                    .iter()
                    .find(|p| {
                        p.manifest_path.parent().map(|d| d.as_std_path()) == Some(cwd.as_path())
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "unable to determine root package from workspace; run inside a member \
                             directory or pass `-p <name>` / `--manifest-path <path>`"
                        )
                    })?
            }
        }
    })
}

/// Returns true if the current invocation context points at a Cargo workspace root
/// (i.e. the manifest contains only a `[workspace]` without a `[package]`).
fn is_workspace_root_context(metadata: &Metadata, manifest_path: Option<&Path>) -> bool {
    if metadata.root_package().is_some() {
        return false;
    }
    let ws_root = metadata.workspace_root.as_std_path();
    if let Some(path) = manifest_path {
        return path == ws_root.join("Cargo.toml");
    }
    if let Ok(cwd) = std::env::current_dir() {
        return cwd == ws_root;
    }
    false
}

/// Produces the `midenc` CLI flags implied by the detected target environment and project type.
fn midenc_flags_from_target(
    target_env: midenc_session::TargetEnv,
    project_type: midenc_session::ProjectType,
    wasm_output: &Path,
) -> Vec<String> {
    let mut midenc_args = Vec::new();

    match target_env {
        midenc_session::TargetEnv::Base | midenc_session::TargetEnv::Emu => match project_type {
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
        midenc_session::TargetEnv::Rollup { target } => {
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
