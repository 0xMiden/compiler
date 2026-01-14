use std::{path::PathBuf, process::Command};

use anyhow::bail;
use clap::Args;

use crate::{
    commands::{build, BuildCommand},
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
pub struct TestCommand {
    /// Arguments parsed by midenc (includes cargo-compatible options).
    #[arg(value_name = "ARG", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl TestCommand {
    pub fn exec(self) -> anyhow::Result<()> {
        let build = BuildCommand {
            args: self.args.clone(),
        };

        let Some(CommandOutput::BuildCommandOutput {
            output: BuildOutput::Masm { artifact_path },
        }) = build.exec(OutputType::Masm)?
        else {
            // This should never happend since we are hardcoding the output as MASM.
            bail!("cargo miden test requires projects to be compiled to a masm artifact.")
        };

        let spawn_args = test_cargo_args();
        unsafe {
            std::env::set_var("CARGO_MIDEN_TEST_PACKAGE_PATH", artifact_path);
        }

        run_cargo_test(&spawn_args)?;

        Ok(())
    }
}

/// Builds the argument vector for the underlying `cargo test` invocation.
fn test_cargo_args() -> Vec<String> {
    let mut args = vec!["test".to_string()];

    // Add build-std flags required for Miden compilation
    args.extend(["-Z", "build-std=std,core,alloc"].into_iter().map(|s| s.to_string()));

    args
}

fn run_cargo_test(spawn_args: &[String]) -> anyhow::Result<()> {
    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut cargo = Command::new(&cargo_path);

    cargo.args(spawn_args);

    let _artifacts = build::spawn_cargo(cargo, &cargo_path)?;

    Ok(())
}
