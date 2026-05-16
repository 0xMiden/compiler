//! `cargo-miden` as a library

#![deny(warnings)]
#![deny(missing_docs)]

use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod outputs;
mod template;
mod utils;

pub use commands::BuildCommand;
pub use outputs::CommandOutput;

/// Requested output type for the `build` command.
#[derive(Debug, Copy, Clone)]
pub enum OutputType {
    /// Return the Wasm component or core Wasm module emitted by Cargo.
    Wasm,
    /// Return the compiled Miden package.
    Masm,
}

/// Runs the `cargo-miden` entry point.
///
/// The iterator of arguments is expected to mirror the invocation of `cargo miden …`.
/// The command returns an optional [`CommandOutput`]; commands that only produce side-effects
/// (such as printing help) will return `Ok(None)`.
pub fn run<T>(args: T) -> Result<Option<CommandOutput>>
where
    T: Iterator<Item = String>,
{
    let collected: Vec<String> = args.collect();
    let command_tokens = extract_command_tokens(&collected);

    let cli = cli::CargoMidenCli::parse_from(command_tokens);

    match cli.command {
        cli::CargoMidenCommand::New(cmd) => {
            let project_path = cmd.exec()?;
            Ok(Some(CommandOutput::NewCommandOutput { project_path }))
        }
        cli::CargoMidenCommand::Build(cmd) => cmd.exec().map(|output| {
            Some(CommandOutput::BuildCommandOutput {
                output: vec![output],
            })
        }),
        cli::CargoMidenCommand::Test(cmd) => {
            cmd.exec()?;
            Ok(None)
        }
    }
}

fn extract_command_tokens(args: &[String]) -> Vec<String> {
    if args.is_empty() {
        panic!("expected `cargo miden [COMMAND]`, got empty args");
    }

    if let Some(idx) = args.iter().position(|arg| arg == "miden") {
        args.iter().skip(idx).cloned().collect()
    } else {
        panic!("expected `cargo miden [COMMAND]`, got {args:?}");
    }
}
