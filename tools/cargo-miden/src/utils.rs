use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use cargo_metadata::{Artifact, Message};

pub(crate) fn set_default_test_compiler(define: &mut Vec<String>) {
    let compiler_path = compiler_path();
    define.push(format!("compiler_path={}", compiler_path.display()));
}

pub(crate) fn compiler_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let compiler_path = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();
    compiler_path.to_path_buf()
}

pub(crate) fn spawn_cargo(mut cmd: Command, cargo: &Path) -> Result<Vec<Artifact>> {
    log::debug!("spawning command {cmd:?}");

    let mut child = cmd
        .spawn()
        .context(format!("failed to spawn `{cargo}`", cargo = cargo.display()))?;

    let mut artifacts = Vec::new();
    let stdout = child.stdout.take().expect("no stdout");
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.context("failed to read output from `cargo`")?;

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

    let status = child
        .wait()
        .context(format!("failed to wait for `{cargo}` to finish", cargo = cargo.display()))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(artifacts)
}
