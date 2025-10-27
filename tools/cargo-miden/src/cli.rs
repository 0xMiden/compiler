use clap::{Parser, Subcommand};

use crate::commands::{BuildCommand, ExampleCommand, NewCommand};

/// Top-level command-line interface for `cargo-miden`.
#[derive(Debug, Parser)]
#[command(
    bin_name = "cargo miden",
    version,
    propagate_version = true,
    arg_required_else_help = true
)]
pub struct CargoMidenCli {
    /// The subcommand to execute.
    #[command(subcommand)]
    pub command: CargoMidenCommand,
}

/// Subcommands supported by `cargo-miden`.
#[derive(Debug, Subcommand)]
pub enum CargoMidenCommand {
    /// Create a new Miden project from a template.
    New(NewCommand),
    /// Compile the current crate to Miden package.
    Build(BuildCommand),
    /// Scaffold one of the curated example projects.
    Example(ExampleCommand),
}
