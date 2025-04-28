use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Result};

/// Represents whether the Cargo project is a Miden program or a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    Program,
    Library,
}

/// Detects whether the project is a Miden program or library based on Cargo metadata.
pub fn detect_project_type(metadata: &cargo_metadata::Metadata) -> ProjectType {
    // is `[package.metadata.miden]` is present then it's a library, otherwise it's a program
    metadata.root_package().map_or(ProjectType::Program, |root_pkg| {
        root_pkg.metadata.as_object().map_or(ProjectType::Program, |meta_obj| {
            if meta_obj.contains_key("miden") {
                ProjectType::Library
            } else {
                ProjectType::Program
            }
        })
    })
}

pub fn install_wasm32_wasip1() -> Result<()> {
    let sysroot = get_sysroot()?;
    if sysroot.join("lib/rustlib/wasm32-wasip1").exists() {
        return Ok(());
    }

    if env::var_os("RUSTUP_TOOLCHAIN").is_none() {
        bail!(
            "failed to find the `wasm32-wasip1` target and `rustup` is not available. If you're \
             using rustup make sure that it's correctly installed; if not, make sure to install \
             the `wasm32-wasip1` target before using this command"
        );
    }

    log::info!("Installing wasm32-wasip1 target");

    let output = Command::new("rustup")
        .arg("target")
        .arg("add")
        .arg("wasm32-wasip1")
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        bail!("failed to install the `wasm32-wasip1` target");
    }

    Ok(())
}

fn get_sysroot() -> Result<PathBuf> {
    let output = Command::new("rustc").arg("--print").arg("sysroot").output()?;

    if !output.status.success() {
        bail!(
            "failed to execute `rustc --print sysroot`, command exited with error: {output}",
            output = String::from_utf8_lossy(&output.stderr)
        );
    }

    let sysroot = PathBuf::from(String::from_utf8(output.stdout)?.trim());

    Ok(sysroot)
}
