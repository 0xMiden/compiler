use clap::{Parser, Subcommand};

use crate::commands::{BuildCommand, NewCommand, PackageCacheCommand, TestCommand};

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
    /// Create a new Miden project (default) or a contract from a template (see Options).
    New(NewCommand),
    /// Compile the current crate to Miden package.
    Build(BuildCommand),
    /// Run the miden-tests in the project.
    Test(TestCommand),
    /// Print the package-cache location and build-script inputs of the current project.
    ///
    /// Contract build scripts use this to populate `MIDENC_PACKAGE_CACHE` for builds that
    /// `midenc` does not drive.
    PackageCache(PackageCacheCommand),
}
