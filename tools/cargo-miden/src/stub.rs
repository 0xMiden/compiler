use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use cargo_metadata::Metadata;

use crate::cargo_component::config::CargoArguments;

/// Build (if needed) and return the path to the cached stub rlib
pub fn ensure_stub_rlib(metadata: &Metadata, cargo_args: &CargoArguments) -> Result<PathBuf> {
    let target_triple =
        std::env::var("CARGO_BUILD_TARGET").unwrap_or_else(|_| "wasm32-wasip1".to_string());
    let profile = if cargo_args.release {
        "release"
    } else {
        "debug"
    };
    let deps_dir = metadata.target_directory.join(&target_triple).join(profile).join("deps");
    let deps_dir_std = deps_dir.as_std_path();
    if !deps_dir_std.exists() {
        std::fs::create_dir_all(deps_dir_std)?;
    }

    let stubs_dir = std::path::Path::new(super::STUBS_DIR);
    let src_root = stubs_dir.join("lib.rs");
    if !src_root.exists() {
        bail!("stub crate root not found: {:?}", src_root);
    }
    // Gather all source files in stubs dir (recursively) to drive rebuild decision
    let mut src_files = list_rs_files(stubs_dir);
    if !src_files.iter().any(|p| p == &src_root) {
        src_files.push(src_root.clone());
    }

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
        // LLVM MergeFunctions pass https://llvm.org/docs/MergeFunctions.html considers some
        // functions in the stub library identical (e.g. `intrinsics::felt::add` and
        // `intrinsics::felt::mul`) because besides the same sig they have the same body
        // (`unreachable`). The pass merges them which manifests in the compiled Wasm as if both
        // `add` and `mul` are linked to the same (`add` in this case) function.
        // Setting `opt-level=1` seems to be skipping this pass and is enough on its own, but I
        // also put `-Z merge-functions=disabled` in case `opt-level=1` behaviour changes
        // in the future and runs the MergeFunctions pass.
        // `opt-level=0` - introduces import for panic infra leading to WIT encoder error (unsatisfied import).
        // Build to a unique temp file to avoid races under parallel builds
        let tmp_out_path =
            deps_dir_std.join(format!("libstub_miden_sdk.{}.tmp.rlib", std::process::id()));
        let status = rustc
            .arg("--crate-name")
            .arg("miden_sdk_stubs")
            .arg("--edition=2021")
            .arg("--crate-type=rlib")
            .arg("--target")
            .arg(&target_triple)
            .arg("-C")
            .arg("opt-level=1")
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("codegen-units=1")
            .arg("-Z")
            .arg("merge-functions=disabled")
            .arg("-o")
            .arg(&tmp_out_path)
            .arg(&src_root)
            .status()?;
        if !status.success() {
            bail!("failed to compile libstub ({status})");
        }
        // Move the temp artifact into place (best-effort to avoid races)
        if let Err(_e) = std::fs::rename(&tmp_out_path, &out_path) {
            // If rename failed because another process won the race, just remove temp
            let _ = std::fs::remove_file(&tmp_out_path);
        }
    } else {
        log::debug!("using cached stub rlib: {}", out_path.display());
    }

    Ok(out_path)
}

/// Recursively collect all .rs files in a directory
fn list_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        if let Ok(read_dir) = std::fs::read_dir(&path) {
            for entry in read_dir.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    files.push(p);
                }
            }
        }
    }
    files
}
