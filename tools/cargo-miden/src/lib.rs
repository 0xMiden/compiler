#![deny(warnings)]

use anyhow::{bail, Context, Result};
use cargo_component::{
    config::{CargoArguments, Config},
    load_component_metadata, load_metadata, run_cargo_command,
};
use cargo_component_core::terminal::{Color, Terminal, Verbosity};
use clap::{CommandFactory, Parser};
use commands::NewCommand;
use compile_masm::wasm_to_masm;
use dependencies::process_miden_dependencies;
use non_component::run_cargo_command_for_non_component;
pub use target::{
    detect_project_type, detect_target_environment, target_environment_to_project_type, ProjectType,
};

mod commands;
mod compile_masm;
mod dependencies;
mod non_component;
mod outputs;
mod target;

pub use outputs::{BuildOutput, CommandOutput};

fn version() -> &'static str {
    option_env!("CARGO_VERSION_INFO").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The list of commands that are built-in to `cargo-miden`.
const BUILTIN_COMMANDS: &[&str] = &[
    "miden", // for indirection via `cargo miden`
    "new",
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
    bin_name = "cargo",
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
    Wasm,
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

            let target_env = target::detect_target_environment(&metadata);
            let project_type = target::target_environment_to_project_type(target_env);

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

            // Get the root package being built
            let root_package =
                metadata.root_package().context("Metadata is missing a root package")?;

            let dependency_packages_paths = process_miden_dependencies(root_package, &cargo_args)?;

            for package in packages.iter_mut() {
                package.metadata.section.bindings.with = [
                    ("miden:base/core-types@1.0.0/felt", "miden::Felt"),
                    ("miden:base/core-types@1.0.0/word", "miden::Word"),
                    ("miden:base/core-types@1.0.0/core-asset", "miden::CoreAsset"),
                    ("miden:base/core-types@1.0.0/account-id", "miden::AccountId"),
                    ("miden:base/core-types@1.0.0/tag", "miden::Tag"),
                    ("miden:base/core-types@1.0.0/note-type", "miden::NoteType"),
                    ("miden:base/core-types@1.0.0/recipient", "miden::Recipient"),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
                // skip functions that are provided by the Miden SDK and/or intrinsics
                // only function names (no CM path)
                package.metadata.section.bindings.skip = vec![
                    // Our function names can clash with user's function names leading to
                    // skipping the bindings generation of the user's function names
                    // see https://github.com/0xMiden/compiler/issues/341
                    "remove-asset",
                    "create-note",
                    "heap-base",
                    "hash-one-to-one",
                    "hash-two-to-one",
                    "add-asset",
                    "add",
                    "unchecked-from-u64",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            }

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

            std::env::set_var("RUSTFLAGS", "-C target-feature=+bulk-memory");
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
            // dbg!(&packages);
            let mut wasm_outputs = rt.block_on(async {
                let config = Config::new(terminal, None).await?;
                let client = config.client(None, cargo_args.offline).await?;
                run_cargo_command(
                    client,
                    &config,
                    &metadata,
                    &packages,
                    subcommand.as_deref(),
                    &cargo_args,
                    &spawn_args,
                )
                .await
            })?;
            // dbg!(&wasm_outputs);
            if wasm_outputs.is_empty() {
                // crates that don't have a WIT component are ignored by the
                // `cargo-component` run_cargo_command and return no outputs.
                // Build them with our own version of run_cargo_command
                wasm_outputs = run_cargo_command_for_non_component(
                    subcommand.as_deref(),
                    &cargo_args,
                    &spawn_args,
                )?;
            }
            assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
            let wasm_output = wasm_outputs.first().unwrap();
            match build_output_type {
                OutputType::Wasm => Ok(Some(CommandOutput::BuildCommandOutput {
                    output: BuildOutput::Wasm {
                        artifact_path: wasm_output.clone(),
                        dependencies: dependency_packages_paths,
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

                    let output = wasm_to_masm(
                        wasm_output,
                        miden_out_dir.as_std_path(),
                        &dependency_packages_paths,
                        project_type,
                        target_env,
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
}
