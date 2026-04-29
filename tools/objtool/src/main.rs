use clap::Parser;
use miden_assembly_syntax::{
    Report,
    diagnostics::{IntoDiagnostic, reporting},
};
use miden_objtool::{decorators, dump};

/// Common utilities for analyzing Miden artifacts
#[derive(Debug, Parser)]
#[command(name = "miden-objtool", version, arg_required_else_help = true)]
enum Cli {
    /// Compare serialized MAST forest sizes after stripping decorators.
    Decorators(decorators::DecoratorsCommand),
    /// Dump various types of information from assembled packages
    #[command(subcommand)]
    Dump(dump::Dump),
}

fn main() -> Result<(), Report> {
    use reporting::ReportHandlerOpts;

    let cli = Cli::parse();

    let result = reporting::set_hook(Box::new(|_| Box::new(ReportHandlerOpts::new().build())));
    if result.is_ok() {
        reporting::set_panic_hook();
    }

    match &cli {
        Cli::Decorators(command) => decorators::run(command).map_err(Report::msg),
        Cli::Dump(command) => dump::run(command).into_diagnostic(),
    }
}
