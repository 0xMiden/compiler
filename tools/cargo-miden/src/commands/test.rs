use std::{path::PathBuf, process::Command};

use clap::Args;

/// Command-line arguments accepted by `cargo miden test`.
///
/// This command is a thin wrapper around `cargo test`, forwarding all arguments
/// to the underlying test invocation. Cargo options precede `--`; test-binary
/// options follow it. For example:
///
/// `cargo miden test --release -p my-package -- --nocapture`
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

    let status = cargo.status()?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::test_cargo_args;
    use crate::{cli::CargoMidenCommand, parse_command_tokens};

    #[test]
    fn test_help_belongs_to_the_wrapper() {
        let error =
            parse_command_tokens(["cargo-miden", "test", "--help"].map(String::from).into())
                .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn forwards_cargo_options_and_test_argument_boundary() {
        for (input, expected) in [
            (vec![], vec!["test"]),
            (vec!["--release", "-p", "foo"], vec!["test", "--release", "-p", "foo"]),
            (
                vec!["--manifest-path", "fixture/Cargo.toml", "a_test"],
                vec!["test", "--manifest-path", "fixture/Cargo.toml", "a_test"],
            ),
            (vec!["--", "--nocapture"], vec!["test", "--", "--nocapture"]),
            (
                vec!["--release", "a_test", "--", "--test-threads=1"],
                vec!["test", "--release", "a_test", "--", "--test-threads=1"],
            ),
            (vec!["--", "--", "literal"], vec!["test", "--", "--", "literal"]),
        ] {
            let cli = parse_command_tokens(
                ["cargo-miden", "test"]
                    .into_iter()
                    .chain(input.iter().copied())
                    .map(String::from)
                    .collect(),
            )
            .unwrap();
            let CargoMidenCommand::Test(command) = cli.command else {
                panic!("expected test command");
            };
            assert_eq!(test_cargo_args(command.args), expected, "input: {input:?}");
        }
    }
}
