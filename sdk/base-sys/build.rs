// Build the Miden base stubs and link them for dependents.
//
// We produce a native static library (.a) that contains only the stub object
// files (no panic handler) to avoid duplicate panic symbols in downstream
// component builds. We do this by compiling a single object with rustc and
// packaging it into an archive with `ar`.
//
// Why not an rlib?
// - `cargo:rustc-link-lib`/`cargo:rustc-link-search` are for native archives;
//   .rlib doesn’t fit that model and attempts to use `rustc-link-arg` don’t
//   propagate to dependents.
// Why not a staticlib via rustc directly?
// - A no_std staticlib usually requires a `#[panic_handler]`, which then
//   collides at link time with other crates that also define panic symbols.
// - Packaging a single object keeps the archive minimal and free of panic
//   symbols.

use std::{env, path::PathBuf, process::Command};

fn ensure_rust_ar() {
    // Preflight: ensure `rust-ar` is available. This typically comes from the
    // `llvm-tools` rustup component or via `cargo-binutils`.
    if Command::new("rust-ar").arg("--version").output().is_ok() {
        return;
    }

    let _ = Command::new("cargo").args(["install", "cargo-binutils"]).status();
    if Command::new("rust-ar").arg("--version").output().is_ok() {
        return;
    }
    panic!(
        "`rust-ar` was not found. Ensure the `llvm-tools` component is present (listed in \
         rust-toolchain.toml) and try installing proxies via `cargo install cargo-binutils`."
    );
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_else(|_| "wasm32-wasip1".to_string());

    if !target.starts_with("wasm32") {
        // Still track files for re-run if changed in case of cross compilation.
        let stubs_root = manifest_dir.join("stubs");
        let src_root = stubs_root.join("lib.rs");
        if src_root.exists() {
            println!("cargo:rerun-if-changed={}", src_root.display());
        }
        return;
    }

    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");

    let stubs_root = manifest_dir.join("stubs");
    let src_root = stubs_root.join("lib.rs");
    // Ensure build script reruns when any stub file changes
    println!("cargo:rerun-if-changed={}", src_root.display());
    if let Ok(read_dir) = std::fs::read_dir(&stubs_root) {
        for entry in read_dir.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&p) {
                    for e in inner.flatten() {
                        let pp = e.path();
                        if pp.extension().and_then(|s| s.to_str()) == Some("rs") {
                            println!("cargo:rerun-if-changed={}", pp.display());
                        }
                    }
                }
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }

    // Build a single object, then package into a static archive to avoid
    // bringing in panic symbols.
    let out_static = out_dir.join("libmiden_base_sys_stubs.a");
    let out_obj = out_dir.join("miden_base_sys_stubs.o");

    // Ensure tools are present before invoking them.
    ensure_rust_ar();

    // 1) Compile object
    // These stubs intentionally compile to `unreachable` so the frontend recognizes
    // and lowers their exported symbol names to MASM calls.
    // LLVM MergeFunctions pass https://llvm.org/docs/MergeFunctions.html considers some
    // functions in the stub library identical (e.g. `intrinsics::felt::add` and
    // `intrinsics::felt::mul`) because besides the same sig they have the same body
    // (`unreachable`). The pass merges them which manifests in the compiled Wasm as if both
    // `add` and `mul` are linked to the same (`add` in this case) function.
    // Setting `opt-level=1` seems to be skipping this pass and is enough on its own, but I
    // also put `-Z merge-functions=disabled` in case `opt-level=1` behaviour changes
    // in the future and runs the MergeFunctions pass.
    // `opt-level=0` - introduces import for panic infra leading to WIT encoder error (unsatisfied import).
    let status = Command::new("rustc")
        .arg("--crate-name")
        .arg("miden_base_sys_stubs")
        .arg("--edition=2021")
        .arg("--crate-type=rlib")
        .arg("--target")
        .arg(&target)
        .arg("-C")
        .arg("opt-level=1")
        .arg("-C")
        .arg("panic=abort")
        .arg("-C")
        .arg("codegen-units=1")
        .arg("-C")
        .arg("debuginfo=0")
        .arg("-Z")
        .arg("merge-functions=disabled")
        .arg("-C")
        .arg("target-feature=+bulk-memory,+wide-arithmetic")
        .arg("--emit=obj")
        .arg("-o")
        .arg(&out_obj)
        .arg(&src_root)
        .status()
        .expect("failed to spawn rustc for base stubs object");
    if !status.success() {
        panic!("failed to compile miden-base-sys stubs object: {status}");
    }

    // 2) Archive
    let status = Command::new("rust-ar")
        .arg("crs")
        .arg(&out_static)
        .arg(&out_obj)
        .status()
        .expect("failed to spawn ar for base stubs");
    if !status.success() {
        panic!("failed to archive miden-base-sys stubs: {status}");
    }

    // Emit link directives for dependents
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=miden_base_sys_stubs");
}
