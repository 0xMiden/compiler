use alloc::format;
use std::{
    boxed::Box,
    env,
    path::{Path, PathBuf},
    process::Command,
    string::{String, ToString},
    vec::Vec,
};

use cargo_metadata::{Artifact, Message};
use midenc_hir::Report;

use crate::CompilerResult;

pub fn install_wasm32_target(wasi: &str, toolchain: Option<&str>) -> CompilerResult<()> {
    let Some(toolchain) = toolchain.map(ToString::to_string).or_else(rustup_toolchain) else {
        return Err(Report::msg(format!(
            "failed to find the `wasm32-{wasi}` target and `rustup` is not available. If you're \
             using rustup make sure that it's correctly installed; if not, make sure to install \
             the `wasm32-{wasi}` target before using this command"
        )));
    };

    log::info!(target: "driver", "verifying wasm32-{wasi} target is installed for the {toolchain} toolchain..");

    let sysroot = get_sysroot(Some(&toolchain))?;
    if sysroot.join(format!("lib/rustlib/wasm32-{wasi}")).exists() {
        log::info!(target: "driver", "wasm32-{wasi} is available");
        return Ok(());
    }

    log::info!(target: "driver", "installing wasm32-{wasi} target");

    let target = format!("wasm32-{wasi}");
    let output = Command::new("rustup")
        .arg("target")
        .arg("add")
        .args(["--toolchain", toolchain.as_str()])
        .arg(&target)
        .stderr(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .output()
        .map_err(|err| Report::msg(format!("failed to execute rustup: {err}")))?;

    if !output.status.success() {
        return Err(Report::msg(format!("failed to install the `{target}` target")));
    }

    log::info!(target: "driver", "ensuring required rustup components are available..");

    ensure_rustup_components_are_installed_for_target(
        &toolchain,
        &target,
        &["rust-src", "rust-std"],
    )?;

    Ok(())
}

pub fn get_sysroot(toolchain: Option<&str>) -> CompilerResult<PathBuf> {
    let mut command = Command::new("rustc");
    if let Some(toolchain) = toolchain {
        command.arg(format!("+{toolchain}"));
    }
    let output = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .arg("--print")
        .arg("sysroot")
        .output()
        .map_err(|err| Report::msg(format!("failed to execute rustc: {err}")))?;

    if !output.status.success() {
        return Err(Report::msg(format!(
            "failed to execute `rustc --print sysroot`: {output}",
            output = String::from_utf8_lossy(&output.stdout)
        )));
    }

    let sysroot = PathBuf::from(
        String::from_utf8(output.stdout)
            .map_err(|err| Report::msg(format!("unable to parse sysroot as valid utf-8: {err}")))?
            .trim(),
    );

    Ok(sysroot)
}

pub fn spawn_cargo(mut cmd: Command, cargo: &Path) -> CompilerResult<Vec<Artifact>> {
    use std::io::BufRead;

    log::debug!(target: "driver", "spawning command {cmd:?}");

    let mut child = cmd.spawn().map_err(|err| {
        Report::msg(format!("failed to spawn `{cargo}`: {err}", cargo = cargo.display()))
    })?;

    let mut artifacts = Vec::new();
    let stdout = child.stdout.take().expect("no stdout");
    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines() {
        let line =
            line.map_err(|err| Report::msg(format!("failed to read output from `cargo`: {err}")))?;

        if line.is_empty() {
            continue;
        }

        for message in Message::parse_stream(line.as_bytes()) {
            let message = message
                .map_err(|err| Report::msg(format!("unexpected JSON message from cargo: {err}")))?;
            if let Message::CompilerArtifact(artifact) = message {
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

    let status = child.wait().map_err(|err| {
        Report::msg(format!(
            "failed to wait for `{cargo}` to finish: {err}",
            cargo = cargo.display()
        ))
    })?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(artifacts)
}

pub fn rustup_toolchain() -> Option<String> {
    if let Ok(toolchain) = env::var("RUSTUP_TOOLCHAIN") {
        return Some(toolchain);
    }
    let output = std::process::Command::new("rustup")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .arg("show")
        .arg("active-toolchain")
        .output()
        .ok()?;
    if !output.status.success() {
        log::error!(target: "driver", "failed to execute `rustc --print sysroot`: {}", String::from_utf8_lossy(&output.stdout));
        None
    } else {
        let output = core::str::from_utf8(&output.stdout).ok()?;
        output.trim_start().split_ascii_whitespace().next().map(ToString::to_string)
    }
}

pub fn ensure_rustup_components_are_installed_for_target(
    toolchain: &str,
    target: &str,
    components: &[&str],
) -> CompilerResult<()> {
    let output = std::process::Command::new("rustup")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .arg("component")
        .arg("add")
        .args(["--toolchain", toolchain])
        .args(["--target", target])
        .args(components)
        .output()
        .map_err(|err| Report::msg(format!("failed to execute `rustup component add`: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Report::msg(format!(
            "failed to install rustup components: {output}",
            output = String::from_utf8_lossy(&output.stdout)
        )))
    }
}

/// Represents the specific artifact produced by the `build` command.
#[derive(Debug, Clone)]
pub enum BuildOutput {
    /// Miden Assembly (.masm) output.
    Masm {
        /// Path to the compiled MASM file or directory containing artifacts.
        artifact_path: PathBuf,
        // Potentially add other relevant info like package name, component type etc.
    },
    /// WebAssembly (.wasm) output.
    Wasm {
        /// Path to the compiled WASM file.
        artifact_path: PathBuf,
        /// The compiler options extracted from the arguments given to `cargo miden build`
        options: Box<midenc_session::Options>,
    },
}

impl BuildOutput {
    /// Get a reference to the filesystem path where the build artifact was placed
    pub fn artifact_path(&self) -> &Path {
        match self {
            Self::Masm { artifact_path } | Self::Wasm { artifact_path, .. } => artifact_path,
        }
    }

    /// Convert this build output to the underlying filesystem path of the build artifact
    pub fn into_artifact_path(self) -> PathBuf {
        match self {
            Self::Masm { artifact_path } | Self::Wasm { artifact_path, .. } => artifact_path,
        }
    }
}
