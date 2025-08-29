use std::path::PathBuf;

use anyhow::{bail, Result};
use cargo_metadata::Metadata;

use crate::cargo_component::config::CargoArguments;

/// Build (if needed) and return the path to the cached stub rlib
pub fn ensure_stub_rlib(metadata: &Metadata, cargo_args: &CargoArguments) -> Result<PathBuf> {
    let profile = if cargo_args.release {
        "release"
    } else {
        "debug"
    };
    let deps_dir = metadata.target_directory.join("wasm32-wasip2").join(profile).join("deps");
    let deps_dir_std = deps_dir.as_std_path();
    if !deps_dir_std.exists() {
        std::fs::create_dir_all(deps_dir_std)?;
    }

    let src_path = std::path::Path::new(super::STUBS_DIR).join("miden_base.rs");
    if !src_path.exists() {
        bail!("stub source not found: {:?}", src_path);
    }

    let out_path = deps_dir_std.join("libstub_miden_sdk.rlib");
    let needs_rebuild = match std::fs::metadata(&out_path) {
        Ok(out_meta) => match (std::fs::metadata(&src_path), out_meta.modified()) {
            (Ok(src_meta), Ok(out_mtime)) => match src_meta.modified() {
                Ok(src_mtime) => out_mtime < src_mtime,
                Err(_) => true,
            },
            _ => true,
        },
        Err(_) => true,
    };

    if needs_rebuild {
        log::debug!("compiling stub rlib: {} (src={})", out_path.display(), src_path.display());
        let mut rustc = std::process::Command::new("rustc");
        let status = rustc
            .arg("--crate-name")
            .arg("stub_add_asset")
            .arg("--edition=2021")
            .arg("--crate-type=rlib")
            .arg("--target")
            .arg("wasm32-wasip2")
            .arg("-C")
            .arg("opt-level=z")
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("codegen-units=1")
            .arg("-o")
            .arg(&out_path)
            .arg(&src_path)
            .status()?;
        if !status.success() {
            bail!("failed to compile libstub ({status})");
        }
    } else {
        log::debug!("using cached stub rlib: {}", out_path.display());
    }

    Ok(out_path)
}
