mod cmp_debug;
mod decorators;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "objsize",
    bin_name = "objsize",
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
    /// Compare `.masp` sizes for `cargo miden build --debug` modes, cleaning build outputs
    CmpDebug(cmp_debug::CmpDebugCommand),
    /// Compare serialized MAST forest sizes after stripping decorators.
    Decorators(decorators::DecoratorsCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::CmpDebug(command) => cmp_debug::run(command),
        Commands::Decorators(command) => decorators::run(command),
    }
}
