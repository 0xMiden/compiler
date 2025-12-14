// Build the Miden stdlib stubs and link them for dependents.
//
// We produce native static libraries (.a) that contain only the stub object
// files (no panic handler) to avoid duplicate panic symbols in downstream
// component builds. We do this by compiling rlibs with rustc and naming the
// outputs `.a` so dependents pick them up via the native link search path.
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

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_else(|_| "wasm32-wasip1".to_string());

    if !target.starts_with("wasm32") {
        // track changes, but don’t build
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
    println!("cargo:rerun-if-changed={}", src_root.display());
    // Ensure build script reruns when any stub file changes
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

    // Build separate libraries for intrinsics and stdlib stubs to keep
    // individual object sections small. Each rlib is emitted with a `.a`
    // extension so cargo treats it as a native archive.
    let out_intrinsics_rlib = out_dir.join("libmiden_stdlib_sys_intrinsics_stubs.a");
    let out_stdlib_rlib = out_dir.join("libmiden_stdlib_sys_stdlib_stubs.a");

    // LLVM MergeFunctions pass https://llvm.org/docs/MergeFunctions.html considers some
    // functions in the stub library identical (e.g. `intrinsics::felt::add` and
    // `intrinsics::felt::mul`) because besides the same sig they have the same body
    // (`unreachable`). The pass merges them which manifests in the compiled Wasm as if both
    // `add` and `mul` are linked to the same (`add` in this case) function.
    // Setting `opt-level=1` seems to be skipping this pass and is enough on its own, but I
    // also put `-Z merge-functions=disabled` in case `opt-level=1` behaviour changes
    // in the future and runs the MergeFunctions pass.
    // `opt-level=0` - introduces import for panic infra leading to WIT encoder error (unsatisfied import).

    // Although the stdlib vs intrinsics split seems redundant in the future we will have to move
    // the intrinsics to the separate crate with its own build.rs script. The reason for the
    // separation is the automatic bindings generation. For the Miden stdlib the bindings will be
    // generated from the Miden package, but for intrinsics they will be maintained manually since
    // the intrinsics are part of the compiler.

    // 1a) Compile intrinsics stubs archive
    let status = Command::new("rustc")
        .arg("--crate-name")
        .arg("miden_stdlib_sys_intrinsics_stubs")
        .arg("--edition=2024")
        .arg("--crate-type=rlib")
        .arg("--target")
        .arg(&target)
        .arg("-C")
        .arg("opt-level=1")
        .arg("-C")
        .arg("codegen-units=1")
        .arg("-C")
        .arg("debuginfo=0")
        .arg("-Z")
        .arg("merge-functions=disabled")
        .arg("-C")
        .arg("target-feature=+bulk-memory,+wide-arithmetic")
        .arg("-o")
        .arg(&out_intrinsics_rlib)
        .arg(stubs_root.join("intrinsics_root.rs"))
        .status()
        .expect("failed to spawn rustc for stdlib intrinsics stub object");
    if !status.success() {
        panic!("failed to compile stdlib intrinsics stub object: {status}");
    }

    // 1b) Compile stdlib (mem/crypto) stubs archive
    let status = Command::new("rustc")
        .arg("--crate-name")
        .arg("miden_stdlib_sys_stdlib_stubs")
        .arg("--edition=2024")
        .arg("--crate-type=rlib")
        .arg("--target")
        .arg(&target)
        .arg("-C")
        .arg("opt-level=1")
        .arg("-C")
        .arg("codegen-units=1")
        .arg("-C")
        .arg("debuginfo=0")
        .arg("-Z")
        .arg("merge-functions=disabled")
        .arg("-C")
        .arg("target-feature=+bulk-memory,+wide-arithmetic")
        .arg("-o")
        .arg(&out_stdlib_rlib)
        .arg(stubs_root.join("stdlib_root.rs"))
        .status()
        .expect("failed to spawn rustc for stdlib (mem/crypto) stub object");
    if !status.success() {
        panic!("failed to compile stdlib (mem/crypto) stub object: {status}");
    }

    // Emit link directives for dependents
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=miden_stdlib_sys_intrinsics_stubs");
    println!("cargo:rustc-link-lib=static=miden_stdlib_sys_stdlib_stubs");
}
