mod decorators;
mod masm2masp;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "miden-objtool",
    bin_name = "miden-objtool",
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
    /// Convert a .masm file to a .masp package
    Masm2masp(masm2masp::Masm2MaspCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Decorators(command) => decorators::run(command),
        Commands::Masm2masp(command) => masm2masp::run(command),
    }
}
