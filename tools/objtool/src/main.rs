mod decorators;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "objtool",
    bin_name = "objtool",
    version,
    about = "Inspect Miden compilation artifact sizes",
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Decorators(command) => decorators::run(command),
    }
}
