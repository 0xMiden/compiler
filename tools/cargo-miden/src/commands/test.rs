use std::{path::PathBuf, process::Command};

use clap::Args;

/// Command-line arguments accepted by `cargo miden build`.
///
/// This command is a thin wrapper around `cargo test`, forwarding all arguments
/// to the underlying test invocation.
#[derive(Clone, Debug, Args)]
#[command(disable_version_flag = true, trailing_var_arg = true)]
pub struct TestCommand {
    /// Arguments forwarded to `cargo test`.
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

    if !cli_args.is_empty() {
        args.extend(["--".into()]);
        args.extend(cli_args);
    }

    args
}

fn run_cargo_test(spawn_args: &[String]) -> anyhow::Result<()> {
    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut cargo = Command::new(&cargo_path);

    cargo.args(spawn_args);

    let status = cargo.status()?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
