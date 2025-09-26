//! `cargo-miden` as a library

#![deny(warnings)]
#![deny(missing_docs)]

use std::path::Path;

use anyhow::{bail, Result};
use cargo_component::{
    config::{CargoArguments, Config},
    load_component_metadata, load_metadata, run_cargo_command, PackageComponentMetadata,
};
use cargo_metadata::{camino, Metadata};
use clap::{CommandFactory, Parser};
pub use commands::new_project::WIT_DEPS_PATH;
use commands::{ExampleCommand, NewCommand};
use compile_masm::wasm_to_masm;
use dependencies::process_miden_dependencies;
use midenc_session::{ProjectType, RollupTarget, TargetEnv};
use non_component::run_cargo_command_for_non_component;
use path_absolutize::Absolutize;
pub use target::{
    detect_project_type, detect_target_environment, target_environment_to_project_type,
};

mod cargo_component;
mod commands;
mod compile_masm;
mod dependencies;
mod non_component;
mod outputs;
mod target;
mod utils;

pub use cargo_component::core::terminal::{Color, Terminal, Verbosity};
pub use outputs::{BuildOutput, CommandOutput};

/// Returns true if the current invocation context points at a Cargo workspace root
/// (i.e. the manifest contains only a `[workspace]` without a `[package]`).
fn is_workspace_root_context(metadata: &Metadata, manifest_path: Option<&Path>) -> bool {
    // If cargo metadata exposes a root package, this is not a pure workspace root manifest.
    if metadata.root_package().is_some() {
        return false;
    }
    let ws_root = metadata.workspace_root.as_std_path();
    if let Some(path) = manifest_path {
        // If an explicit manifest was provided and it is the workspace root manifest,
        // then we are running from the workspace root context.
        return path == ws_root.join("Cargo.toml");
    }
    // Otherwise, treat the current directory as the context
    if let Ok(cwd) = std::env::current_dir() {
        return cwd == ws_root;
    }
    false
}

// All wasm stub symbols are provided by miden-stdlib-sys and miden-base-sys
// via their respective build.rs scripts.

fn version() -> &'static str {
    option_env!("CARGO_VERSION_INFO").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The list of commands that are built-in to `cargo-miden`.
const BUILTIN_COMMANDS: &[&str] = &[
    "miden", // for indirection via `cargo miden`
    "new", "example",
];

/// The list of commands that are explicitly unsupported by `cargo-miden`.
///
/// These commands are intended to integrate with `crates.io` and have no
/// analog in `cargo-miden` currently.
const UNSUPPORTED_COMMANDS: &[&str] =
    &["install", "login", "logout", "owner", "package", "search", "uninstall"];

const AFTER_HELP: &str = "Unrecognized subcommands will be passed to cargo verbatim
     and the artifacts will be processed afterwards (e.g. `build` command compiles MASM).
     \nSee `cargo help` for more information on available cargo commands.";

/// Cargo integration for Miden
#[derive(Parser)]
#[clap(
    bin_name = "cargo miden",
    version,
    propagate_version = true,
    arg_required_else_help = true,
    after_help = AFTER_HELP
)]
#[command(version = version())]
enum CargoMiden {
    /// Cargo integration for Miden
    #[clap(subcommand, hide = true, after_help = AFTER_HELP)]
    Miden(Command), // indirection via `cargo miden`
    #[clap(flatten)]
    Command(Command),
}

#[derive(Parser)]
enum Command {
    New(NewCommand),
    Example(ExampleCommand),
}

fn detect_subcommand<I, T>(args: I) -> Option<String>
where
    I: IntoIterator<Item = T>,
    T: Into<String> + Clone,
{
    let mut iter = args.into_iter().map(Into::into).peekable();

    // Skip the first argument if it is `miden` (i.e. `cargo miden`)
    if let Some(arg) = iter.peek() {
        if arg == "miden" {
            iter.next().unwrap();
        }
    }

    for arg in iter {
        // Break out of processing at the first `--`
        if arg == "--" {
            break;
        }

        if !arg.starts_with('-') {
            return Some(arg);
        }
    }

    None
}

/// Requested output type for the `build` command
pub enum OutputType {
    /// Wasm component or core Wasm module
    Wasm,
    /// Miden package
    Masm,
    // Hir,
}

/// Runs the cargo-miden command
/// The arguments are expected to start with `["cargo", "miden", ...]` followed by a subcommand
/// with options
/// Returns the outputs of the command.
pub fn run<T>(args: T, build_output_type: OutputType) -> Result<Option<CommandOutput>>
where
    T: Iterator<Item = String>,
{
    // The first argument is the cargo-miden binary path
    let args = args.skip_while(|arg| arg != "miden").collect::<Vec<_>>();
    let subcommand = detect_subcommand(args.clone());

    match subcommand.as_deref() {
        // Check for built-in command or no command (shows help)
        Some(cmd) if BUILTIN_COMMANDS.contains(&cmd) => {
            match CargoMiden::parse_from(args.clone()) {
                CargoMiden::Miden(cmd) | CargoMiden::Command(cmd) => match cmd {
                    Command::New(cmd) => {
                        let project_path = cmd.exec()?;
                        Ok(Some(CommandOutput::NewCommandOutput { project_path }))
                    }
                    Command::Example(cmd) => {
                        let project_path = cmd.exec()?;
                        Ok(Some(CommandOutput::NewCommandOutput { project_path }))
                    }
                },
            }
        }
        // Check for explicitly unsupported commands (e.g. those that deal with crates.io)
        Some(cmd) if UNSUPPORTED_COMMANDS.contains(&cmd) => {
            let terminal = Terminal::new(Verbosity::Normal, Color::Auto);
            terminal.error(format!(
                "command `{cmd}` is not supported by `cargo component`\n\nuse `cargo {cmd}` \
                 instead"
            ))?;
            std::process::exit(1);
        }
        // If no subcommand was detected,
        None => {
            // Attempt to parse the supported CLI (expected to fail)
            CargoMiden::parse_from(args);

            // If somehow the CLI parsed correctly despite no subcommand,
            // print the help instead
            CargoMiden::command().print_long_help()?;
            Ok(None)
        }

        _ => {
            // Not a built-in command, run the cargo command
            let args = args.into_iter().skip_while(|arg| arg == "miden").collect::<Vec<_>>();
            let cargo_args = CargoArguments::parse_from(args.clone().into_iter())?;
            // dbg!(&cargo_args);
            let metadata = load_metadata(cargo_args.manifest_path.as_deref())?;

            // If invoked at a workspace root (manifest contains only [workspace]) without
            // selecting a specific package, fail with a clear message. We only support
            // building a single crate at a time for now.
            if is_workspace_root_context(&metadata, cargo_args.manifest_path.as_deref())
                && cargo_args.packages.is_empty()
                && !cargo_args.workspace
            {
                bail!(
                    "You're running `cargo miden` from a Cargo workspace root. Building the \
                     entire workspace is not supported yet. Build a single member instead, for \
                     example:\n  - cd <member>/ && cargo miden build --release
                    "
                );
            }

            let mut packages = load_component_metadata(
                &metadata,
                cargo_args.packages.iter(),
                cargo_args.workspace,
            )?;

            if packages.is_empty() {
                bail!(
                    "manifest `{path}` contains no package or the workspace has no members",
                    path = metadata.workspace_root.join("Cargo.toml")
                );
            }

            // Determine the package being built (the "root" for our purposes).
            // Prefer cargo's root package, then `--manifest-path`, then current directory.
            let root_package = match metadata.root_package() {
                Some(pkg) => pkg,
                None => {
                    // Try to resolve via explicit manifest path
                    if let Some(manifest_path) = cargo_args.manifest_path.as_deref() {
                        let mp_utf8 =
                            camino::Utf8Path::from_path(manifest_path).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "manifest path is not valid UTF-8: {}",
                                    manifest_path.display()
                                )
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
                                    "unable to determine root package: manifest `{}` does not \
                                     match any workspace package",
                                    manifest_path.display()
                                )
                            })?
                    } else {
                        // Fall back to current working directory matching a package manifest dir
                        let cwd = std::env::current_dir()?;
                        metadata
                            .packages
                            .iter()
                            .find(|p| {
                                p.manifest_path.parent().map(|d| d.as_std_path())
                                    == Some(cwd.as_path())
                            })
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "unable to determine root package from workspace; run inside \
                                     a member directory or pass `-p <name>` / `--manifest-path \
                                     <path>`"
                                )
                            })?
                    }
                }
            };

            let target_env = target::detect_target_environment(root_package)?;
            let project_type = target::target_environment_to_project_type(target_env);

            // Ensure the selected root package is included in the list of packages for which
            // we generate bindings. This is critical in workspaces where `workspace_default_packages()`
            // may not include the current member.
            if !packages.iter().any(|p| p.package.id == root_package.id) {
                packages.push(PackageComponentMetadata::new(root_package)?);
            }

            let dependency_packages_paths = process_miden_dependencies(root_package, &cargo_args)?;

            let mut spawn_args: Vec<_> = args.clone().into_iter().collect();
            spawn_args.extend_from_slice(
                &[
                    "-Z",
                    // compile std as part of crate graph compilation
                    // https://doc.rust-lang.org/cargo/reference/unstable.html#build-std
                    // to abort on panic below
                    "build-std=std,core,alloc,panic_abort",
                    "-Z",
                    // abort on panic without message formatting (core::fmt uses call_indirect)
                    "build-std-features=panic_immediate_abort",
                ]
                .map(|s| s.to_string()),
            );

            // Convert profile options from examples/**/Cargo.toml to
            // equivalent cargo command-line overrides. This ensures the intended
            // behavior even when member profile tables are ignored by cargo
            // (e.g., when building inside a workspace).
            //
            // [profile.dev]
            //   panic = "abort"
            //   opt-level = 1
            //   debug-assertions = false
            //   overflow-checks = false
            //   debug = true
            // [profile.release]
            //   opt-level = "z"
            //   panic = "abort"
            // Configure cargo profile settings via --config overrides, mirroring
            // what used to be in example manifests.

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

            // `--fatal-warnings` will force the wasm-ld to error out in case of a function signature
            // mismatch. This will surface the stub functions signature mismatches early on.
            // Otherwise the wasm-ld will prefix the stub function name with `signature_mismatch:`.
            let extra_rust_flags = String::from(
                "-C target-feature=+bulk-memory,+wide-arithmetic -C link-args=--fatal-warnings",
            );
            // Augment RUSTFLAGS to ensure we preserve any flags set by the user
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
            let wasm_outputs = if matches!(target_env, TargetEnv::Rollup { .. }) {
                rt.block_on(async {
                    let config = Config::new(terminal, None).await?;
                    let client = config.client(None, cargo_args.offline).await?;
                    let wasm_outputs_res = run_cargo_command(
                        client,
                        &config,
                        &metadata,
                        &packages,
                        subcommand.as_deref(),
                        &cargo_args,
                        &spawn_args,
                    )
                    .await;

                    if let Err(e) = wasm_outputs_res {
                        config.terminal().error(format!("{e:?}"))?;
                        std::process::exit(1);
                    };
                    wasm_outputs_res
                })?
            } else {
                run_cargo_command_for_non_component(
                    subcommand.as_deref(),
                    &cargo_args,
                    &spawn_args,
                )?
            };

            if let Some(old_rustflags) = maybe_old_rustflags {
                std::env::set_var("RUSTFLAGS", old_rustflags);
            } else {
                std::env::remove_var("RUSTFLAGS");
            }

            assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
            let wasm_output = wasm_outputs.first().expect("expected at least one Wasm artifact");

            let mut midenc_flags = midenc_flags_from_target(target_env, project_type, wasm_output);

            // Add dependency linker arguments
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
                    let miden_out_dir =
                        metadata.target_directory.join("miden").join(if cargo_args.release {
                            "release"
                        } else {
                            "debug"
                        });
                    if !miden_out_dir.exists() {
                        std::fs::create_dir_all(&miden_out_dir)?;
                    }

                    let output =
                        wasm_to_masm(wasm_output, miden_out_dir.as_std_path(), midenc_flags)
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
}

fn midenc_flags_from_target(
    target_env: TargetEnv,
    project_type: ProjectType,
    wasm_output: &Path,
) -> Vec<String> {
    let mut midenc_args = Vec::new();

    match target_env {
        TargetEnv::Base | TargetEnv::Emu => match project_type {
            ProjectType::Program => {
                midenc_args.push("--exe".into());
                let masm_module_name = wasm_output
                    .file_stem()
                    .expect("invalid wasm file path: no file stem")
                    .to_str()
                    .unwrap();
                let entrypoint_opt = format!("--entrypoint={masm_module_name}::entrypoint");
                midenc_args.push(entrypoint_opt);
            }
            ProjectType::Library => midenc_args.push("--lib".into()),
        },
        TargetEnv::Rollup { target } => {
            midenc_args.push("--target".into());
            match target {
                RollupTarget::Account => {
                    midenc_args.push("rollup:account".into());
                    midenc_args.push("--lib".into());
                }
                RollupTarget::NoteScript => {
                    midenc_args.push("rollup:note-script".into());
                    midenc_args.push("--exe".into());
                    midenc_args.push("--entrypoint=miden:base/note-script@1.0.0::run".to_string())
                }
                RollupTarget::TransactionScript => {
                    midenc_args.push("rollup:transaction-script".into());
                    midenc_args.push("--exe".into());
                    midenc_args
                        .push("--entrypoint=miden:base/transaction-script@1.0.0::run".to_string())
                }
                RollupTarget::AuthComponent => {
                    midenc_args.push("rollup:authentication-component".into());
                    midenc_args.push("--lib".into());
                }
            }
        }
    }
    midenc_args
}
