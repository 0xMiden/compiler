use clap::{Parser, Subcommand};
use miden_assembly_syntax::{Report, diagnostics::reporting};
use miden_objtool::decorators;

#[derive(Debug, Parser)]
#[command(
    name = "miden-objtool",
    version,
    about = "Common utilities for analyzing Miden artifacts",
    long_about = None,
    arg_required_else_help = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Compare serialized MAST forest sizes after stripping decorators.
    Decorators(decorators::DecoratorsCommand),
}

fn main() -> Result<(), Report> {
    use reporting::ReportHandlerOpts;

    let cli = Cli::parse();

    let result = reporting::set_hook(Box::new(|_| Box::new(ReportHandlerOpts::new().build())));
    if result.is_ok() {
        reporting::set_panic_hook();
    }

    match &cli.command {
        Commands::Decorators(command) => decorators::run(command).map_err(Report::msg),
    }
}
