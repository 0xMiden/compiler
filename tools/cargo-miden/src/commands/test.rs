use std::{path::PathBuf, process::Command};

use clap::Args;

/// Command-line arguments accepted by `cargo miden build`.
///
/// All arguments following `build` are parsed by the `midenc` compiler's argument parser.
/// Cargo-specific options (`--release`, `--manifest-path`, `--workspace`, `--package`)
/// are recognized and forwarded to the underlying `cargo build` invocation.
/// All other options are passed to `midenc` for compilation.
#[derive(Clone, Debug, Args)]
#[command(disable_version_flag = true, trailing_var_arg = true)]
pub struct TestCommand {
    /// Arguments parsed by midenc (includes cargo-compatible options).
    #[arg(value_name = "ARG", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl TestCommand {
    pub fn exec(self) -> anyhow::Result<()> {
        let spawn_args = test_cargo_args(self.args);

        run_cargo_test(&spawn_args)?;

        Ok(())
    }
}

/// Builds the argument vector for the underlying `cargo test` invocation.
fn test_cargo_args(cli_args: Vec<String>) -> Vec<String> {
    let mut args = vec!["test".to_string()];

    // Add build-std flags required for Miden compilation
    args.extend(["-Z", "build-std=std,core,alloc"].into_iter().map(|s| s.to_string()));

    args.extend(["--".into()]);
    args.extend(cli_args);

    args
}

fn run_cargo_test(spawn_args: &[String]) -> anyhow::Result<()> {
    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut cargo = Command::new(&cargo_path);

    cargo.args(spawn_args);

    let _artifacts = crate::utils::spawn_cargo(cargo, &cargo_path)?;

    Ok(())
}
