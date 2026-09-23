use std::{ffi::OsString, path::PathBuf};

use anyhow::{Result, anyhow};
use clap::Args;
use midenc_session::diagnostics::PrintDiagnostic;

/// Command-line arguments accepted by `cargo miden build`.
///
/// Every argument after `build` is forwarded verbatim to `midenc`, which parses them and decides
/// what to build: `cargo miden build <args>` compiles exactly what `midenc <args>` would, run in
/// the same directory. Run `cargo miden build --help` to see them — that help is `midenc`'s.
#[derive(Clone, Debug, Args)]
#[command(
    disable_help_flag = true,
    disable_version_flag = true,
    trailing_var_arg = true
)]
pub struct BuildCommand {
    /// Arguments forwarded to `midenc`.
    #[arg(value_name = "ARG", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl BuildCommand {
    /// Executes `cargo miden build`, returning the built package's path — or `None` when the
    /// run was deliberately stopped short of a package by `--stop-after` (e.g. the contract
    /// build script staging a consumer's dependencies without compiling the consumer).
    pub fn exec(self) -> Result<Option<PathBuf>> {
        let cwd = std::env::current_dir()?;
        // The program name `midenc` heads the argument vector because that is what a process
        // receives, and it is the name clap reports usage errors and help under.
        let argv = std::iter::once(OsString::from("midenc"))
            .chain(self.args.into_iter().map(OsString::from));

        match midenc_driver::Midenc::exec(cwd, argv, None) {
            Ok(output) => Ok(output),
            Err(report) => match report.downcast::<midenc_driver::ClapDiagnostic>() {
                // Usage errors and `--help`, which clap renders and exits on itself.
                Ok(err) => err.exit(),
                Err(report) => Err(anyhow!("{}", PrintDiagnostic::new(report))),
            },
        }
    }
}
