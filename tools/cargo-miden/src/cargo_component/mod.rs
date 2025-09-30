//! Cargo support for WebAssembly components.

use std::{
    fmt::{self},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use cargo_config2::{PathAndArgs, TargetTripleRef};
use cargo_metadata::{Artifact, Message, Metadata, MetadataCommand, Package};
use config::{CargoArguments, CargoPackageSpec, Config};
use target::install_wasm32_wasip2;

pub mod config;
pub mod core;
mod target;

fn is_wasm_target(target: &str) -> bool {
    target == "wasm32-wasi"
        || target == "wasm32-wasip1"
        || target == "wasm32-wasip2"
        || target == "wasm32-unknown-unknown"
}

/// Represents a cargo package paired with its component metadata.
#[derive(Debug)]
pub struct PackageComponentMetadata<'a> {
    /// The cargo package.
    pub package: &'a Package,
}

impl<'a> PackageComponentMetadata<'a> {
    /// Creates a new package metadata from the given package.
    pub fn new(package: &'a Package) -> Result<Self> {
        Ok(Self { package })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum CargoCommand {
    #[default]
    Other,
    Help,
    Build,
    Run,
    Test,
    Bench,
    Serve,
}

impl CargoCommand {
    fn buildable(self) -> bool {
        matches!(self, Self::Build | Self::Run | Self::Test | Self::Bench | Self::Serve)
    }

    fn runnable(self) -> bool {
        matches!(self, Self::Run | Self::Test | Self::Bench | Self::Serve)
    }

    fn testable(self) -> bool {
        matches!(self, Self::Test | Self::Bench)
    }
}

impl fmt::Display for CargoCommand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Help => write!(f, "help"),
            Self::Build => write!(f, "build"),
            Self::Run => write!(f, "run"),
            Self::Test => write!(f, "test"),
            Self::Bench => write!(f, "bench"),
            Self::Serve => write!(f, "serve"),
            Self::Other => write!(f, "<unknown>"),
        }
    }
}

impl From<&str> for CargoCommand {
    fn from(s: &str) -> Self {
        match s {
            "h" | "help" => Self::Help,
            "b" | "build" | "rustc" => Self::Build,
            "r" | "run" => Self::Run,
            "t" | "test" => Self::Test,
            "bench" => Self::Bench,
            "serve" => Self::Serve,
            _ => Self::Other,
        }
    }
}

/// Runs the cargo command as specified in the configuration.
///
/// Note: if the command returns a non-zero status, or if the
/// `--help` option was given on the command line, this
/// function will exit the process.
///
/// Returns any relevant output components.
pub async fn run_cargo_command(
    config: &Config,
    subcommand: Option<&str>,
    cargo_args: &CargoArguments,
    spawn_args: &[String],
) -> Result<Vec<PathBuf>> {
    // resolve_component_dependencies(client, config, metadata, packages, cargo_args).await?;

    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let command = if cargo_args.help {
        // Treat `--help` as the help command
        CargoCommand::Help
    } else {
        subcommand.map(CargoCommand::from).unwrap_or_default()
    };

    let (build_args, output_args) = match spawn_args.iter().position(|a| a == "--") {
        Some(position) => spawn_args.split_at(position),
        None => (spawn_args, &[] as _),
    };
    let needs_runner = !build_args.iter().any(|a| a == "--no-run");

    let mut args = build_args.iter().peekable();
    if let Some(arg) = args.peek() {
        if *arg == "component" {
            args.next().unwrap();
        }
    }

    // Spawn the actual cargo command
    log::debug!(
        "spawning cargo `{path}` with arguments `{args:?}`",
        path = cargo_path.display(),
        args = args.clone().collect::<Vec<_>>(),
    );

    let mut cargo = Command::new(&cargo_path);
    if matches!(command, CargoCommand::Run | CargoCommand::Serve) {
        // Treat run and serve as build commands as we need to componentize the output
        cargo.arg("build");
        if let Some(arg) = args.peek() {
            if Some((*arg).as_str()) == subcommand {
                args.next().unwrap();
            }
        }
    }
    cargo.args(args);

    let cargo_config = cargo_config2::Config::load()?;

    // Handle the target for buildable commands
    if command.buildable() {
        install_wasm32_wasip2(config)?;

        // Add an implicit wasm32-wasip2 target if there isn't a wasm target present
        if !cargo_args.targets.iter().any(|t| is_wasm_target(t))
            && !cargo_config
                .build
                .target
                .as_ref()
                .is_some_and(|v| v.iter().any(|t| is_wasm_target(t.triple())))
        {
            cargo.arg("--target").arg("wasm32-wasip2");
        }

        if let Some(format) = &cargo_args.message_format {
            if format != "json-render-diagnostics" {
                bail!("unsupported cargo message format `{format}`");
            }
        }

        // It will output the message as json so we can extract the wasm files
        // that will be componentized
        cargo.arg("--message-format").arg("json-render-diagnostics");
        cargo.stdout(Stdio::piped());
    } else {
        cargo.stdout(Stdio::inherit());
    }

    // At this point, spawn the command for help and terminate
    if command == CargoCommand::Help {
        let mut child = cargo
            .spawn()
            .context(format!("failed to spawn `{path}`", path = cargo_path.display()))?;

        let status = child.wait().context(format!(
            "failed to wait for `{path}` to finish",
            path = cargo_path.display()
        ))?;

        std::process::exit(status.code().unwrap_or(0));
    }

    if needs_runner && command.testable() {
        // Only build for the test target; running will be handled
        // after the componentization
        cargo.arg("--no-run");
    }

    let runner = if needs_runner && command.runnable() {
        Some(get_runner(&cargo_config, command == CargoCommand::Serve)?)
    } else {
        None
    };

    let artifacts = spawn_cargo(cargo, &cargo_path, cargo_args, command.buildable())?;

    let outputs: Vec<Output> = artifacts
        .into_iter()
        .filter_map(|a| {
            let path: PathBuf = a.filenames.first().unwrap().clone().into();
            if path.to_str().unwrap().contains("wasm32-wasip2") {
                Some(Output {
                    path,
                    display: Some(a.target.name),
                })
            } else {
                None
            }
        })
        .collect();

    if let Some(runner) = runner {
        spawn_outputs(config, &runner, output_args, &outputs, command)?;
    }

    Ok(outputs.into_iter().map(|o| o.path).collect())
}

fn get_runner(cargo_config: &cargo_config2::Config, serve: bool) -> Result<PathAndArgs> {
    // We check here before we actually build that a runtime is present.
    // We first check the runner for `wasm32-wasip2` in the order from
    // cargo's convention for a user-supplied runtime (path or executable)
    // and use the default, namely `wasmtime`, if it is not set.
    let (runner, using_default) = cargo_config
        .runner(TargetTripleRef::from("wasm32-wasip2"))
        .unwrap_or_default()
        .map(|runner_override| (runner_override, false))
        .unwrap_or_else(|| {
            (
                PathAndArgs::new("wasmtime")
                    .args(if serve {
                        vec!["serve", "-S", "cli", "-S", "http"]
                    } else {
                        vec!["-S", "preview2", "-S", "cli", "-S", "http"]
                    })
                    .to_owned(),
                true,
            )
        });

    // Treat the runner object as an executable with list of arguments it
    // that was extracted by splitting each whitespace. This allows the user
    // to provide arguments which are passed to wasmtime without having to
    // add more command-line argument parsing to this crate.
    let wasi_runner = runner.path.to_string_lossy().into_owned();

    if !using_default {
        // check if the override runner exists
        if !(runner.path.exists() || which::which(&runner.path).is_ok()) {
            bail!(
                "failed to find `{wasi_runner}` specified by either the \
                 `CARGO_TARGET_WASM32_WASIP2_RUNNER`environment variable or as the \
                 `wasm32-wasip2` runner in `.cargo/config.toml`"
            );
        }
    } else if which::which(&runner.path).is_err() {
        bail!(
            "failed to find `{wasi_runner}` on PATH\n\nensure Wasmtime is installed before \
             running this command\n\n{msg}:\n\n  {instructions}",
            msg = if cfg!(unix) {
                "Wasmtime can be installed via a shell script"
            } else {
                "Wasmtime can be installed via the GitHub releases page"
            },
            instructions = if cfg!(unix) {
                "curl https://wasmtime.dev/install.sh -sSf | bash"
            } else {
                "https://github.com/bytecodealliance/wasmtime/releases"
            },
        );
    }

    Ok(runner)
}

fn spawn_cargo(
    mut cmd: Command,
    cargo: &Path,
    cargo_args: &CargoArguments,
    process_messages: bool,
) -> Result<Vec<Artifact>> {
    log::debug!("spawning command {cmd:?}");

    let mut child = cmd
        .spawn()
        .context(format!("failed to spawn `{cargo}`", cargo = cargo.display()))?;

    let mut artifacts = Vec::new();
    if process_messages {
        let stdout = child.stdout.take().expect("no stdout");
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line.context("failed to read output from `cargo`")?;

            // If the command line arguments also had `--message-format`, echo the line
            if cargo_args.message_format.is_some() {
                println!("{line}");
            }

            if line.is_empty() {
                continue;
            }

            for message in Message::parse_stream(line.as_bytes()) {
                if let Message::CompilerArtifact(artifact) =
                    message.context("unexpected JSON message from cargo")?
                {
                    for path in &artifact.filenames {
                        match path.extension() {
                            Some("wasm") => {
                                artifacts.push(artifact);
                                break;
                            }
                            _ => continue,
                        }
                    }
                }
            }
        }
    }

    let status = child
        .wait()
        .context(format!("failed to wait for `{cargo}` to finish", cargo = cargo.display()))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(artifacts)
}

struct Output {
    /// The path to the output.
    path: PathBuf,
    /// The display name if the output is an executable.
    display: Option<String>,
}

fn spawn_outputs(
    config: &Config,
    runner: &PathAndArgs,
    output_args: &[String],
    outputs: &[Output],
    command: CargoCommand,
) -> Result<()> {
    let executables = outputs
        .iter()
        .filter_map(|output| output.display.as_ref().map(|display| (display, &output.path)))
        .collect::<Vec<_>>();

    if matches!(command, CargoCommand::Run | CargoCommand::Serve) && executables.len() > 1 {
        config.terminal().error(format!(
            "`cargo component {command}` can run at most one component, but multiple were \
             specified",
        ))
    } else if executables.is_empty() {
        config.terminal().error(format!(
            "a component {ty} target must be available for `cargo component {command}`",
            ty = if matches!(command, CargoCommand::Run | CargoCommand::Serve) {
                "bin"
            } else {
                "test"
            }
        ))
    } else {
        for (display, executable) in executables {
            config.terminal().status("Running", display)?;

            let mut cmd = Command::new(&runner.path);
            cmd.args(&runner.args)
                .arg("--")
                .arg(executable)
                .args(output_args.iter().skip(1))
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            log::debug!("spawning command {cmd:?}");

            let mut child = cmd
                .spawn()
                .context(format!("failed to spawn `{runner}`", runner = runner.path.display()))?;

            let status = child.wait().context(format!(
                "failed to wait for `{runner}` to finish",
                runner = runner.path.display()
            ))?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }

        Ok(())
    }
}

/// Loads the workspace metadata based on the given manifest path.
pub fn load_metadata(manifest_path: Option<&Path>) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command.no_deps();

    if let Some(path) = manifest_path {
        log::debug!("loading metadata from manifest `{path}`", path = path.display());
        command.manifest_path(path);
    } else {
        log::debug!("loading metadata from current directory");
    }

    command.exec().context("failed to load cargo metadata")
}

/// Loads the component metadata for the given package specs.
///
/// If `workspace` is true, all workspace packages are loaded.
pub fn load_component_metadata<'a>(
    metadata: &'a Metadata,
    specs: impl ExactSizeIterator<Item = &'a CargoPackageSpec>,
    workspace: bool,
) -> Result<Vec<PackageComponentMetadata<'a>>> {
    let pkgs = if workspace {
        metadata.workspace_packages()
    } else if specs.len() > 0 {
        let mut pkgs = Vec::with_capacity(specs.len());
        for spec in specs {
            let pkg = metadata
                .packages
                .iter()
                .find(|p| {
                    p.name == spec.name
                        && match spec.version.as_ref() {
                            Some(v) => &p.version == v,
                            None => true,
                        }
                })
                .with_context(|| {
                    format!("package ID specification `{spec}` did not match any packages")
                })?;
            pkgs.push(pkg);
        }

        pkgs
    } else {
        metadata.workspace_default_packages()
    };

    pkgs.into_iter().map(PackageComponentMetadata::new).collect::<Result<_>>()
}
