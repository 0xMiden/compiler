use std::{collections::BTreeSet, path::PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use midenc_release::{
    candidate::{Candidate, UnitDeclaration, render_tag},
    closure,
    config::{Config, Unit, VersionSource},
    executor,
    github::rest::RestGitHub,
    intent, lint, order, plan as release_plan,
    reconcile::{self, Disposition, Planned},
    registry::{CurlUpstream, Faults, NoUpstream, Registry, Upstream},
    staging, version,
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
    /// Generate the release intent from the committed candidate.
    ///
    /// Deterministic by construction: identical inputs produce byte-identical
    /// output, so a reviewed intent and an executed one are provably the same.
    Plan {
        /// Path to the candidate declaration.
        #[arg(long, default_value = ".release/release.toml")]
        candidate: PathBuf,
        /// The commit whose source would be packaged.
        #[arg(long)]
        subject: String,
        /// Write the intent here instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Verify the packaged crates build when resolved from a registry.
    ///
    /// This is what justifies publishing with `--no-verify`: production skips
    /// Cargo's verification so no build script runs beside a live token, so
    /// this is the only proof the archives are usable. Required, not optional.
    VerifyClosure {
        /// Which unit to verify. Omit for every publishable package.
        #[arg(long)]
        unit: Option<UnitArg>,
        /// Skip the consumer build. Much faster, and much weaker: resolution
        /// alone cannot prove the archives contain every file they need.
        #[arg(long)]
        no_build: bool,
        /// Cache upstream index responses here between runs.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Seal an intent against the artifacts built from it.
    ///
    /// Packages the closure, then binds the reviewed scope to the exact bytes
    /// that will be published. Sealed plans are never edited: anything that
    /// would change one requires a new intent.
    Seal {
        /// The intent to seal.
        #[arg(long)]
        intent: PathBuf,
        /// Write the plan here instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Skip the consumer build while packaging. Faster, weaker.
        #[arg(long)]
        no_build: bool,
        /// Cache upstream index responses here between runs.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
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
        /// Reconcile against a sealed plan, so an existing version with
        /// different content is reported as a conflict rather than skipped.
        #[arg(long)]
        plan: Option<PathBuf>,
    },
    /// Publish a sealed plan.
    ///
    /// Stages run in order and each is verified before the next begins.
    /// Reconciliation runs first, so this is the same operation on a first
    /// attempt and on a resume.
    Publish {
        /// The sealed plan to publish.
        #[arg(long)]
        plan: PathBuf,
        /// Publish to a rehearsal registry instead of crates.io.
        #[arg(long)]
        rehearsal_index: Option<String>,
        /// Report what would be published and stop.
        #[arg(long)]
        dry_run: bool,
        /// Write the journal here.
        #[arg(long)]
        journal: Option<PathBuf>,
    },
    /// Build the template bundle archive.
    ///
    /// The archive is deterministic: its digest identifies the template
    /// contents, which is what a compiler release checks its embedded copy
    /// against.
    Bundle {
        /// Write the archive here.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Create and populate the draft releases for a sealed plan.
    ///
    /// The last reversible step: drafts can still be deleted, no tag exists,
    /// and nothing is published.
    Stage {
        /// The sealed plan.
        #[arg(long)]
        plan: PathBuf,
        /// Directory holding the artifacts to attach.
        #[arg(long)]
        artifacts: Option<PathBuf>,
        /// GitHub API base; defaults to the real one via GITHUB_API_URL.
        #[arg(long)]
        api_base: Option<String>,
    },
    /// Delete the still-draft releases for a plan.
    Discard {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        api_base: Option<String>,
    },
    /// Package a built executable into a deterministic archive.
    ArchiveBinary {
        /// The executable to package.
        #[arg(long)]
        binary: PathBuf,
        /// Its name inside the archive, and on `PATH` after extraction.
        #[arg(long)]
        name: String,
        #[arg(long)]
        output: PathBuf,
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
    /// The template bundle. Publishes no crates; its version lives in
    /// `extra/templates/bundle.toml`.
    Templates,
}

impl From<UnitArg> for Unit {
    fn from(arg: UnitArg) -> Self {
        match arg {
            UnitArg::Compiler => Self::Compiler,
            UnitArg::Sdk => Self::Sdk,
            UnitArg::Templates => Self::Templates,
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
            // Templates carry no crates, so their version lives in the bundle
            // manifest rather than in a version domain of Cargo manifests.
            if let UnitArg::Templates = unit {
                let templates = ws.root.join("extra/templates");
                let bundle = midenc_release::bundle::Bundle::load(&templates.join("bundle.toml"))?;
                let new = requested.unwrap_or_else(|| version::next_minor(&bundle.version));
                if new <= bundle.version {
                    bail!("refusing to move {} to {new}: versions must increase", bundle.version);
                }
                println!("templates: {} -> {new}", bundle.version);
                if dry_run {
                    println!("\ndry run: nothing written");
                    return Ok(());
                }
                midenc_release::bundle::set_version(&templates, &new)?;
                update_candidate(&manifest_dir.join(".release/release.toml"), &config, unit, &new)?;
                println!("recorded the candidate; review the diff");
                return Ok(());
            }

            let domain = match unit {
                UnitArg::Compiler => VersionSource::Workspace,
                UnitArg::Sdk => VersionSource::Sdk,
                UnitArg::Templates => unreachable!("handled above"),
            };
            let plan = version::plan(&ws, &config, domain, requested)?;
            print!("{}", plan.summary());

            if dry_run {
                println!("\ndry run: nothing written");
                return Ok(());
            }
            version::apply(&ws, &config, &plan)?;
            println!("\nupdated {} manifest edit(s) and refreshed Cargo.lock", plan.edits.len());

            let candidate_path = manifest_dir.join(".release/release.toml");
            update_candidate(&candidate_path, &config, unit, &plan.new)?;
            println!("recorded the candidate in {}", candidate_path.display());
            println!("review the diff, then open the release-candidate pull request");
            Ok(())
        }
        Command::Plan {
            candidate,
            subject,
            output,
        } => {
            let candidate = Candidate::load(&manifest_dir.join(&candidate))?;
            let intent = intent::generate(&ws, &config, &candidate, &subject)?;
            let json = intent.to_canonical_json();

            match output {
                Some(path) => {
                    std::fs::write(&path, format!("{json}\n"))?;
                    println!("wrote intent to {} (digest {})", path.display(), intent.digest());
                }
                None => println!("{json}"),
            }
            Ok(())
        }
        Command::VerifyClosure {
            unit,
            no_build,
            cache_dir,
        } => {
            let selected: BTreeSet<String> = match unit {
                Some(unit) => config.packages_in(unit.into()).map(|p| p.name.clone()).collect(),
                None => {
                    config.packages.iter().filter(|p| p.publish).map(|p| p.name.clone()).collect()
                }
            };
            let packages = order::topological(&ws, &selected)?;

            // Fail early and legibly on a cross-unit dependency that is neither
            // selected nor published, rather than letting it surface as a Cargo
            // resolution error during packaging.
            let index = midenc_release::registry::client::SparseIndex::new(
                "sparse+https://index.crates.io/",
            );
            let problems = closure::check_external_dependencies(&ws, &index, &packages)?;
            if !problems.is_empty() {
                for problem in &problems {
                    eprintln!("error: {problem}");
                }
                bail!("the selection is not self-contained");
            }

            println!("verifying the closure of {} package(s)...", packages.len());
            let options = closure::Options {
                packages,
                build_consumer: !no_build,
                allow_upstream: true,
                cache_dir,
            };
            let result = closure::verify(&ws.root, &options)?;

            for packaged in &result.crates {
                println!(
                    "  {} {}  {}  {} bytes",
                    packaged.name,
                    packaged.version,
                    &packaged.digest[..16],
                    packaged.size
                );
            }
            println!();
            if no_build {
                println!(
                    "{} package(s) resolve from a registry; the consumer build was skipped",
                    result.crates.len()
                );
            } else {
                println!(
                    "{} package(s) package, resolve, and build from a registry",
                    result.crates.len()
                );
            }
            Ok(())
        }
        Command::Seal {
            intent: intent_path,
            output,
            no_build,
            cache_dir,
        } => {
            let text = std::fs::read_to_string(&intent_path)?;
            let intent: intent::Intent = serde_json::from_str(&text)?;

            let packages: Vec<String> =
                intent.stages.iter().flat_map(|s| s.packages.iter().cloned()).collect();
            println!("sealing {} package(s) from {}", packages.len(), intent_path.display());

            let options = closure::Options {
                packages,
                build_consumer: !no_build,
                allow_upstream: true,
                cache_dir,
            };
            let built = closure::verify(&ws.root, &options)?;
            let plan = release_plan::seal(&intent, &built)?;
            let json = plan.to_canonical_json();

            match output {
                Some(path) => {
                    std::fs::write(&path, format!("{json}\n"))?;
                    println!("sealed plan at {} (digest {})", path.display(), plan.digest());
                }
                None => println!("{json}"),
            }
            Ok(())
        }
        Command::Reconcile { index, unit, plan } => {
            let planned: Vec<Planned> = match &plan {
                // A sealed plan carries digests, so an existing version with
                // different content is a conflict rather than a skip.
                Some(path) => {
                    let plan = release_plan::Plan::load(path)?;
                    match unit {
                        Some(unit) => plan.planned_for(match unit {
                            UnitArg::Compiler => "compiler",
                            UnitArg::Sdk => "sdk",
                            UnitArg::Templates => "templates",
                        }),
                        None => plan.planned(),
                    }
                }
                None => {
                    let selected: BTreeSet<String> = match unit {
                        Some(unit) => {
                            config.packages_in(unit.into()).map(|p| p.name.clone()).collect()
                        }
                        None => config
                            .packages
                            .iter()
                            .filter(|p| p.publish)
                            .map(|p| p.name.clone())
                            .collect(),
                    };
                    order::topological(&ws, &selected)?
                        .into_iter()
                        .map(|name| {
                            let version = ws.packages[&name].version.clone();
                            Planned {
                                name,
                                version,
                                // Without a sealed plan, presence is all we can check.
                                expected_cksum: None,
                            }
                        })
                        .collect()
                }
            };

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
        Command::Publish {
            plan,
            rehearsal_index,
            dry_run,
            journal,
        } => {
            let plan = release_plan::Plan::load(&plan)?;
            let (target, index_url) = match &rehearsal_index {
                Some(url) => (
                    executor::Target::Rehearsal {
                        index_url: url.clone(),
                        token: "rehearsal".to_string(),
                    },
                    url.clone(),
                ),
                None => (executor::Target::CratesIo, "sparse+https://index.crates.io/".to_string()),
            };

            println!("publishing plan {} to {target}", plan.digest());
            if dry_run {
                println!("(dry run: nothing will be published)");
            }

            let index = midenc_release::registry::client::SparseIndex::new(index_url);
            let options = executor::Options {
                dry_run,
                cargo_home: ws.root.join("target/release-publish/cargo-home"),
            };
            std::fs::create_dir_all(&options.cargo_home)?;

            let record = executor::execute(&ws, &plan, &index, &target, &options)?;
            for entry in &record.entries {
                println!("  [{}] {}: {}", entry.stage, entry.action, entry.detail);
            }
            if let Some(path) = journal {
                std::fs::write(&path, format!("{}\n", record.to_json()))?;
                println!("journal written to {}", path.display());
            }
            Ok(())
        }
        Command::Bundle { output } => {
            let root = ws.root.join("extra/templates");
            let bundle = midenc_release::bundle::Bundle::load(&root.join("bundle.toml"))?;

            let problems = midenc_release::bundle::check_sdk_requirements(&root, &bundle)?;
            if !problems.is_empty() {
                for problem in &problems {
                    eprintln!("error: {problem}");
                }
                bail!("the templates disagree with the bundle's declared SDK requirement");
            }

            // The archive is built from tracked files, so anything else in a
            // template directory is silently absent. Say so before writing it.
            for stray in midenc_release::bundle::untracked(&bundle, &root)? {
                eprintln!(
                    "warning: extra/templates/{} is not tracked by git and is not in the bundle",
                    stray.display()
                );
            }

            let (bytes, digest) = midenc_release::bundle::archive(&root, &bundle)?;
            let files = bundle.files(&root)?.len();
            println!("templates {} — {files} files, {} bytes", bundle.version, bytes.len());
            println!("sha256 {digest}");

            if let Some(path) = output {
                std::fs::write(&path, &bytes)?;
                println!("wrote {}", path.display());
            }
            Ok(())
        }
        Command::Stage {
            plan,
            artifacts,
            api_base,
        } => {
            let plan = release_plan::Plan::load(&plan)?;
            let github = github_client(api_base)?;

            // Artifacts are discovered by unit from the directory the build jobs
            // populated, so the workflow decides what exists and this decides
            // where it goes.
            let mut payloads: std::collections::BTreeMap<String, staging::Payload> =
                Default::default();
            if let Some(dir) = artifacts {
                for entry in std::fs::read_dir(&dir)? {
                    let path = entry?.path();
                    if !path.is_file() {
                        continue;
                    }
                    let name =
                        path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    let unit = if name.starts_with("templates") {
                        "templates"
                    } else {
                        "compiler"
                    };
                    payloads.entry(unit.to_string()).or_default().add(name, path);
                }
            }

            let staged = staging::stage(github.as_ref(), &plan, &payloads)?;
            for entry in &staged {
                println!(
                    "{} draft {} ({} asset(s))",
                    entry.tag,
                    entry.release_id,
                    entry.assets.len()
                );
                for asset in &entry.assets {
                    println!("    {} {} bytes {}", asset.name, asset.size, &asset.digest[..16]);
                }
            }
            Ok(())
        }
        Command::Discard { plan, api_base } => {
            let plan = release_plan::Plan::load(&plan)?;
            let github = github_client(api_base)?;
            for tag in staging::discard(github.as_ref(), &plan)? {
                println!("deleted draft {tag}");
            }
            Ok(())
        }
        Command::ArchiveBinary {
            binary,
            name,
            output,
        } => {
            let archive = staging::archive_binary(&binary, &name)?;
            std::fs::write(&output, &archive)?;
            println!(
                "{} -> {} ({} bytes, sha256 {})",
                binary.display(),
                output.display(),
                archive.len(),
                midenc_release::registry::sha256_hex(&archive)
            );
            Ok(())
        }
        Command::FakeRegistry { .. } => unreachable!("handled before config loading"),
    }
}

/// Record the bumped unit in the candidate declaration, preserving any other
/// units already selected.
fn update_candidate(
    path: &std::path::Path,
    config: &Config,
    unit: UnitArg,
    version: &semver::Version,
) -> Result<()> {
    let name = match unit {
        UnitArg::Compiler => "compiler",
        UnitArg::Sdk => "sdk",
        UnitArg::Templates => "templates",
    };
    let unit_config = config
        .units
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("unit '{name}' is not defined in .release/config.toml"))?;

    let mut candidate = Candidate::load(path).unwrap_or_else(|_| Candidate {
        schema_version: midenc_release::candidate::SUPPORTED_SCHEMA_VERSION,
        units: Vec::new(),
        declarations: Default::default(),
    });

    if !candidate.units.iter().any(|u| u == name) {
        candidate.units.push(name.to_string());
        candidate.units.sort();
    }
    candidate.declarations.insert(
        name.to_string(),
        UnitDeclaration {
            version: version.clone(),
            tag: render_tag(&unit_config.tag, version),
            prerelease: !version.pre.is_empty(),
        },
    );

    candidate.save(path)
}

/// A GitHub client, pointed at a stub when a base URL is supplied.
fn github_client(api_base: Option<String>) -> Result<Box<dyn midenc_release::github::GitHub>> {
    Ok(match api_base {
        Some(base) => {
            let repo =
                std::env::var("GITHUB_REPOSITORY").unwrap_or_else(|_| "owner/repo".to_string());
            Box::new(RestGitHub::for_testing(base, repo))
        }
        None => Box::new(RestGitHub::from_env()?),
    })
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
