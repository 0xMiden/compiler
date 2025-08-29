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

    let stubs_dir = std::path::Path::new(super::STUBS_DIR);
    let src_root = stubs_dir.join("lib.rs");
    if !src_root.exists() {
        bail!("stub crate root not found: {:?}", src_root);
    }
    let miden_base = stubs_dir.join("miden_base.rs");
    let intrinsics = stubs_dir.join("intrinsics.rs");
    let src_files = [src_root.as_path(), miden_base.as_path(), intrinsics.as_path()];

    let out_path = deps_dir_std.join("libstub_miden_sdk.rlib");
    let needs_rebuild = match std::fs::metadata(&out_path) {
        Ok(out_meta) => match out_meta.modified() {
            Ok(out_mtime) => {
                // Rebuild if any source is newer than output
                src_files.iter().any(|p| match std::fs::metadata(p) {
                    Ok(meta) => meta.modified().map(|m| m > out_mtime).unwrap_or(true),
                    Err(_) => true,
                })
            }
            Err(_) => true,
        },
        Err(_) => true,
    };

    if needs_rebuild {
        log::debug!("compiling stub rlib: {} (root={})", out_path.display(), src_root.display());
        let mut rustc = std::process::Command::new("rustc");
        let status = rustc
            .arg("--crate-name")
            .arg("miden_sdk_stubs")
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
            .arg(&src_root)
            .status()?;
        if !status.success() {
            bail!("failed to compile libstub ({status})");
        }
    } else {
        log::debug!("using cached stub rlib: {}", out_path.display());
    }

    Ok(out_path)
}
