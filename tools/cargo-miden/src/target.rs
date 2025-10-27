use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Result};
use cargo_metadata::Package;
use midenc_session::{ProjectType, RollupTarget, TargetEnv};

/// Detects the target environment based on Cargo metadata.
pub fn detect_target_environment(root_pkg: &Package) -> Result<TargetEnv> {
    let Some(meta_obj) = root_pkg.metadata.as_object() else {
        return Ok(TargetEnv::Base);
    };
    let Some(miden_meta) = meta_obj.get("miden") else {
        return Ok(TargetEnv::Base);
    };
    let Some(miden_meta_obj) = miden_meta.as_object() else {
        return Ok(TargetEnv::Base);
    };

    // project-kind field is required
    let Some(project_kind) = miden_meta_obj.get("project-kind") else {
        bail!(
            "Missing required field 'project-kind' in [package.metadata.miden]. Must be one of: \
             'account', 'note-script', or 'transaction-script'"
        );
    };

    let Some(kind_str) = project_kind.as_str() else {
        bail!(
            "Field 'project-kind' in [package.metadata.miden] must be a string. Must be one of: \
             'account', 'note-script', or 'transaction-script'"
        );
    };

    match kind_str {
        "account" => Ok(TargetEnv::Rollup {
            target: RollupTarget::Account,
        }),
        "note-script" => Ok(TargetEnv::Rollup {
            target: RollupTarget::NoteScript,
        }),
        "transaction-script" => Ok(TargetEnv::Rollup {
            target: RollupTarget::TransactionScript,
        }),
        "authentication-component" => Ok(TargetEnv::Rollup {
            target: RollupTarget::AuthComponent,
        }),
        _ => bail!(
            "Invalid value '{}' for 'project-kind' in [package.metadata.miden]. Must be one of: \
             'account', 'note-script', or 'transaction-script'",
            kind_str
        ),
    }
}

/// Determines the project type based on the target environment
pub fn target_environment_to_project_type(target_env: TargetEnv) -> ProjectType {
    match target_env {
        TargetEnv::Base => ProjectType::Program,
        TargetEnv::Rollup { target } => match target {
            RollupTarget::Account => ProjectType::Library,
            RollupTarget::AuthComponent => ProjectType::Library,
            RollupTarget::NoteScript | RollupTarget::TransactionScript => ProjectType::Program,
        },
        TargetEnv::Emu => {
            panic!("Emulator target environment is not supported for project type detection",)
        }
    }
}

/// Detect the project type
pub fn detect_project_type(root_pkg: &Package) -> Result<ProjectType> {
    let target_env = detect_target_environment(root_pkg)?;
    Ok(target_environment_to_project_type(target_env))
}

pub fn install_wasm32_target(wasi: &str) -> Result<()> {
    let sysroot = get_sysroot()?;
    if sysroot.join(format!("lib/rustlib/wasm32-{wasi}")).exists() {
        return Ok(());
    }

    if env::var_os("RUSTUP_TOOLCHAIN").is_none() {
        bail!(
            "failed to find the `wasm32-{wasi}` target and `rustup` is not available. If you're \
             using rustup make sure that it's correctly installed; if not, make sure to install \
             the `wasm32-{wasi}` target before using this command"
        );
    }

    log::info!("Installing wasm32-{wasi} target");

    let output = Command::new("rustup")
        .arg("target")
        .arg("add")
        .arg(format!("wasm32-{wasi}"))
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        bail!("failed to install the `wasm32-{wasi}` target");
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
