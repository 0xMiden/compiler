use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Result};
use midenc_session::{RollupTarget, TargetEnv};

/// Represents whether the Cargo project is a Miden program or a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    /// Miden program
    Program,
    /// Miden library
    Library,
}

/// Detects the target environment based on Cargo metadata.
pub fn detect_target_environment(metadata: &cargo_metadata::Metadata) -> TargetEnv {
    let Some(root_pkg) = metadata.root_package() else {
        return TargetEnv::Base;
    };
    let Some(meta_obj) = root_pkg.metadata.as_object() else {
        return TargetEnv::Base;
    };
    let Some(miden_meta) = meta_obj.get("miden") else {
        return TargetEnv::Base;
    };
    let Some(miden_meta_obj) = miden_meta.as_object() else {
        return TargetEnv::Base;
    };
    if miden_meta_obj.contains_key("supported-types") {
        TargetEnv::Rollup {
            target: RollupTarget::Account,
        }
    } else {
        TargetEnv::Rollup {
            target: RollupTarget::Script,
        }
    }
}

/// Determines the project type based on the target environment
pub fn target_environment_to_project_type(target_env: TargetEnv) -> ProjectType {
    match target_env {
        TargetEnv::Base => ProjectType::Program,
        TargetEnv::Rollup { target } => match target {
            RollupTarget::Account => ProjectType::Library,
            RollupTarget::Script => ProjectType::Program,
        },
        TargetEnv::Emu => {
            panic!("Emulator target environment is not supported for project type detection",)
        }
    }
}

/// Detect the project type
pub fn detect_project_type(metadata: &cargo_metadata::Metadata) -> ProjectType {
    let target_env = detect_target_environment(metadata);
    target_environment_to_project_type(target_env)
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
