use std::{collections::BTreeSet, path::PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use midenc_release::{
    config::{Config, Unit, VersionSource},
    lint, order,
    reconcile::{self, Disposition, Planned},
    registry::{CurlUpstream, Faults, NoUpstream, Registry, Upstream},
    version,
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
    /// Move a version domain, updating every requirement that names it.
    ///
    /// Compiler crates inherit the workspace version; SDK crates carry their
    /// own but move together. Defaults to the next minor.
    SetVersion {
        /// Which domain to move.
        #[arg(long)]
        unit: UnitArg,
        /// The new version. Defaults to the next minor.
        version: Option<semver::Version>,
        /// Print the edits without writing them.
        #[arg(long)]
        dry_run: bool,
    },
    /// Report what still needs publishing, against live registry state.
    ///
    /// This runs identically on a first attempt and on a resume; the only
    /// difference is what the registry already contains.
    Reconcile {
        /// Index to query. Defaults to crates.io; point it at a rehearsal
        /// registry to reconcile against one.
        #[arg(long, default_value = "sparse+https://index.crates.io/")]
        index: String,
        /// Which unit to reconcile. Omit for every publishable package.
        #[arg(long)]
        unit: Option<UnitArg>,
    },
    /// Run the rehearsal registry until interrupted.
    ///
    /// Prints the configuration a rehearsal needs. Source replacement redirects
    /// dependency resolution and `--index` redirects the upload target; both are
    /// required, and `--index` without replacement fails to resolve
    /// interdependent unpublished crates.
    FakeRegistry {
        /// Port to bind on; 0 selects an ephemeral port.
        #[arg(long, default_value_t = 8732)]
        port: u16,
        /// Directory for the upstream index cache.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Serve only locally published crates, never contacting crates.io.
        #[arg(long)]
        offline: bool,
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

    // The registry stands alone: it needs neither release config nor workspace.
    if let Command::FakeRegistry {
        port,
        cache_dir,
        offline,
    } = &cli.command
    {
        return run_fake_registry(*port, cache_dir.clone(), *offline);
    }

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
        Command::SetVersion {
            unit,
            version: requested,
            dry_run,
        } => {
            let domain = match unit {
                UnitArg::Compiler => VersionSource::Workspace,
                UnitArg::Sdk => VersionSource::Sdk,
            };
            let plan = version::plan(&ws, &config, domain, requested)?;
            print!("{}", plan.summary());

            if dry_run {
                println!("\ndry run: nothing written");
                return Ok(());
            }
            version::apply(&ws, &config, &plan)?;
            println!("\nupdated {} manifest edit(s) and refreshed Cargo.lock", plan.edits.len());
            println!("review the diff, then record the release candidate in .release/release.toml");
            Ok(())
        }
        Command::Reconcile { index, unit } => {
            let selected: BTreeSet<String> = match unit {
                Some(unit) => config.packages_in(unit.into()).map(|p| p.name.clone()).collect(),
                None => {
                    config.packages.iter().filter(|p| p.publish).map(|p| p.name.clone()).collect()
                }
            };

            let planned: Vec<Planned> = order::topological(&ws, &selected)?
                .into_iter()
                .map(|name| {
                    let version = ws.packages[&name].version.clone();
                    Planned {
                        name,
                        version,
                        // No sealed plan yet, so presence is taken as a match.
                        expected_cksum: None,
                    }
                })
                .collect();

            let client = midenc_release::registry::client::SparseIndex::new(index);
            let result = reconcile::reconcile(&ws, &client, &planned)?;

            for outcome in &result.outcomes {
                let label = match &outcome.disposition {
                    Disposition::Publish => "publish".to_string(),
                    Disposition::Skip => "skip   ".to_string(),
                    Disposition::Conflict(conflict) => format!("CONFLICT {conflict}"),
                };
                println!("{label}  {} {}", outcome.name, outcome.version);
            }

            println!();
            if !result.is_publishable() {
                bail!(
                    "{} conflict(s); resolve them or abandon the release before publishing",
                    result.conflicts().count()
                );
            }
            if result.is_complete() {
                println!("nothing to publish: every planned version is already published");
                return Ok(());
            }
            println!("{} crate(s) to publish, in order:", result.to_publish.len());
            let args: Vec<String> =
                result.to_publish.iter().flat_map(|n| ["-p".to_string(), n.clone()]).collect();
            println!("  {}", args.join(" "));
            Ok(())
        }
        Command::FakeRegistry { .. } => unreachable!("handled before config loading"),
    }
}

fn run_fake_registry(port: u16, cache_dir: Option<PathBuf>, offline: bool) -> Result<()> {
    let upstream: std::sync::Arc<dyn Upstream> = if offline {
        std::sync::Arc::new(NoUpstream)
    } else {
        std::sync::Arc::new(CurlUpstream::new(cache_dir))
    };

    let registry = Registry::start(port, Faults::default(), upstream)?;
    let index = registry.index_url();

    println!("rehearsal registry listening");
    println!();
    println!("  publish with:");
    println!(
        "    cargo publish --no-verify --index {index} --token rehearsal $(release-tool \
         package-order --cargo-args)"
    );
    println!();
    println!("  and this in $CARGO_HOME/config.toml, so resolution finds unpublished crates:");
    println!("    [source.crates-io]");
    println!("    replace-with = \"rehearsal\"");
    println!("    [source.rehearsal]");
    println!("    registry = \"{index}\"");
    println!();
    println!("press ctrl-c to stop");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
