use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use cargo_component::config::CargoArguments;
use cargo_metadata::{Artifact, Message};

use crate::target::install_wasm32_wasip1;

fn is_wasm_target(target: &str) -> bool {
    target == "wasm32-wasi" || target == "wasm32-wasip1" || target == "wasm32-unknown-unknown"
}

pub fn run_cargo_command_for_non_component(
    subcommand: Option<&str>,
    cargo_args: &CargoArguments,
    spawn_args: &[String],
    env_vars: &HashMap<String, String>,
) -> anyhow::Result<Vec<PathBuf>> {
    let cargo_path = std::env::var("CARGO")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let is_build = matches!(subcommand, Some("b") | Some("build"));

    let (build_args, _output_args) = match spawn_args.iter().position(|a| a == "--") {
        Some(position) => spawn_args.split_at(position),
        None => (spawn_args, &[] as _),
    };

    let mut args = build_args.iter().peekable();
    if let Some(arg) = args.peek() {
        if *arg == "miden" {
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
    cargo.envs(env_vars);
    cargo.args(args);

    let cargo_config = cargo_config2::Config::load()?;

    // Handle the target for build command
    if is_build {
        install_wasm32_wasip1()?;

        // Add an implicit wasm32-wasip1 target if there isn't a wasm target present
        if !cargo_args.targets.iter().any(|t| is_wasm_target(t))
            && !cargo_config
                .build
                .target
                .as_ref()
                .is_some_and(|v| v.iter().any(|t| is_wasm_target(t.triple())))
        {
            cargo.arg("--target").arg("wasm32-wasip1");
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

    let artifacts = spawn_cargo(cargo, &cargo_path, cargo_args, is_build)?;
    Ok(artifacts
        .into_iter()
        .flat_map(|a| {
            a.filenames.into_iter().filter(|p| p.extension() == Some("wasm") && p.exists())
        })
        .map(|p| p.as_std_path().to_path_buf())
        .collect())
}

fn spawn_cargo(
    mut cmd: Command,
    cargo: &Path,
    cargo_args: &CargoArguments,
    process_messages: bool,
) -> Result<Vec<Artifact>> {
    log::debug!("spawning command {:?}", cmd);

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
