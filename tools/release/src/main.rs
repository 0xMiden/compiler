mod config;
mod lint;
mod order;
mod workspace;

use std::{collections::BTreeSet, path::PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use crate::{
    config::{Config, Unit},
    workspace::Workspace,
};

/// Release tooling for the Miden compiler repository.
#[derive(Debug, Parser)]
#[command(name = "release-tool", version, arg_required_else_help = true)]
struct Cli {
    /// Path to the release configuration.
    #[arg(long, default_value = ".release/config.toml", global = true)]
    config: PathBuf,

    /// Directory to run Cargo from; defaults to the current directory.
    #[arg(long, global = true)]
    manifest_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check release-candidate preconditions.
    Lint,
    /// Print the publication order for a release unit.
    ///
    /// Cargo's own packaging order is unreliable at this workspace's scale, so
    /// every `cargo package` and `cargo publish` invocation must take its `-p`
    /// list from here. See `tasks/design/release-tooling.md` §8.4.
    PackageOrder {
        /// Which unit to order. Omit to order every publishable package.
        #[arg(long)]
        unit: Option<UnitArg>,
        /// Emit the order as `-p NAME` arguments ready to pass to Cargo.
        #[arg(long)]
        cargo_args: bool,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum UnitArg {
    Compiler,
    Sdk,
}

impl From<UnitArg> for Unit {
    fn from(arg: UnitArg) -> Self {
        match arg {
            UnitArg::Compiler => Self::Compiler,
            UnitArg::Sdk => Self::Sdk,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let manifest_dir = match cli.manifest_dir {
        Some(dir) => dir,
        None => std::env::current_dir()?,
    };

    let config = Config::load(&manifest_dir.join(&cli.config))?;
    let ws = Workspace::load(&manifest_dir)?;

    match cli.command {
        Command::Lint => {
            let findings = lint::run(&ws, &config)?;
            if findings.is_empty() {
                println!(
                    "release lint: {} packages classified, no findings",
                    config.packages.len()
                );
                return Ok(());
            }
            for error in &findings.errors {
                eprintln!("error: {error}");
            }
            bail!("release lint found {} problem(s)", findings.errors.len());
        }
        Command::PackageOrder { unit, cargo_args } => {
            let selected: BTreeSet<String> = match unit {
                Some(unit) => config.packages_in(unit.into()).map(|p| p.name.clone()).collect(),
                None => {
                    config.packages.iter().filter(|p| p.publish).map(|p| p.name.clone()).collect()
                }
            };

            let order = order::topological(&ws, &selected)?;
            if cargo_args {
                let args: Vec<String> =
                    order.iter().flat_map(|n| ["-p".to_string(), n.clone()]).collect();
                println!("{}", args.join(" "));
            } else {
                for name in order {
                    println!("{name}");
                }
            }
            Ok(())
        }
    }
}
