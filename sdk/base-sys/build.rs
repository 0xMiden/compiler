// Build the Miden base stubs and link them for dependents.
//
// This compiles the files under `stubs/` into a tiny native static library
// (.a) that defines `extern "C"` functions with `#[export_name]` matching the
// symbols used by the SDK bindings (e.g. `miden::account::add_asset`). The
// functions are intentionally `unreachable` and serve only as anchors for
// lowering in the frontend.
//
// Why staticlib (and not rlib)?
// - Cargo build scripts propagate native link metadata via
//   `cargo:rustc-link-lib`/`cargo:rustc-link-search` to downstream final links.
//   That mechanism expects native archives (static/dylib), not .rlib.
// - Using `cargo:rustc-link-arg` to point at an .rlib only affects the current
//   crate’s link; it does not propagate to dependents (per the docs).
//
// Therefore, we produce a staticlib and announce it with
// `cargo:rustc-link-lib=static=...`, so any crate that depends on this one will
// link the stubs automatically without additional tooling.

use std::{env, path::PathBuf, process::Command};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_else(|_| "wasm32-wasip1".to_string());

    // Only build/link stubs when compiling for Wasm targets.
    if !target.starts_with("wasm32") {
        // Still track files for re-run if changed in case of cross compilation.
        let stubs_root = manifest_dir.join("stubs");
        let src_root = stubs_root.join("lib.rs");
        if src_root.exists() {
            println!("cargo:rerun-if-changed={}", src_root.display());
        }
        return;
    }

    let stubs_root = manifest_dir.join("stubs");
    let src_root = stubs_root.join("lib.rs");

    // Ensure build script reruns when any stub file changes
    println!("cargo:rerun-if-changed={}", src_root.display());
    if let Ok(read_dir) = std::fs::read_dir(&stubs_root) {
        for entry in read_dir.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }

    // Compile the stubs to OUT_DIR as staticlib (.a). See the note above on why
    // staticlib is used instead of rlib.
    let out_static = out_dir.join("libmiden_base_sys_stubs.a");

    // Rebuild if missing or sources are newer
    let needs_rebuild = match std::fs::metadata(&out_static) {
        Ok(meta) => {
            let out_mtime = meta.modified().ok();
            if out_mtime.is_none() {
                true
            } else {
                // Check a small set of files; if needed this can be expanded
                let mut newer = false;
                for p in [
                    src_root.clone(),
                    stubs_root.join("account.rs"),
                    stubs_root.join("note.rs"),
                    stubs_root.join("tx.rs"),
                ] {
                    if let (Ok(sm), Some(omt)) = (std::fs::metadata(&p), out_mtime) {
                        if sm.modified().map(|m| m > omt).unwrap_or(true) {
                            newer = true;
                            break;
                        }
                    } else {
                        newer = true;
                        break;
                    }
                }
                newer
            }
        }
        Err(_) => true,
    };

    if needs_rebuild {
        // LLVM MergeFunctions pass https://llvm.org/docs/MergeFunctions.html considers some
        // functions in the stub library identical (e.g. `intrinsics::felt::add` and
        // `intrinsics::felt::mul`) because besides the same sig they have the same body
        // (`unreachable`). The pass merges them which manifests in the compiled Wasm as if both
        // `add` and `mul` are linked to the same (`add` in this case) function.
        // Setting `opt-level=1` seems to be skipping this pass and is enough on its own, but I
        // also put `-Z merge-functions=disabled` in case `opt-level=1` behaviour changes
        // in the future and runs the MergeFunctions pass.
        // `opt-level=0` - introduces import for panic infra leading to WIT encoder error (unsatisfied import).
        // Build staticlib (.a) for native static linking
        let mut cmd = Command::new("rustc");
        let status = cmd
            .arg("--crate-name")
            .arg("miden_base_sys_stubs")
            .arg("--edition=2021")
            .arg("--crate-type=staticlib")
            .arg("--target")
            .arg(&target)
            .arg("-C")
            .arg("opt-level=1")
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("codegen-units=1")
            .arg("-Z")
            .arg("merge-functions=disabled")
            .arg("-C")
            .arg("target-feature=+bulk-memory,+wide-arithmetic")
            .arg("-o")
            .arg(&out_static)
            .arg(&src_root)
            .status()
            .expect("failed to spawn rustc for static stubs");
        if !status.success() {
            panic!("failed to compile miden-base-sys static stubs: {status}");
        }
    }

    // Emit link directives for dependents
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=miden_base_sys_stubs");
}
