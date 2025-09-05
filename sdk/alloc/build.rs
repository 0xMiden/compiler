// Build the Miden alloc stubs and link them for dependents.
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

fn main() {
    let target = env::var("TARGET").unwrap_or_else(|_| "wasm32-wasip1".to_string());

    // Only build the wasm stub when targeting wasm32
    if !target.starts_with("wasm32") {
        println!("cargo:rerun-if-changed=stubs/heap_base.rs");
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");
    println!("cargo:rerun-if-changed={}", manifest_dir.join("stubs/heap_base.rs").display());

    let out_obj = out_dir.join("miden_alloc_heap_base.o");
    let out_a = out_dir.join("libmiden_alloc_intrinsics.a");

    // Compile a single object with the stub
    let status = Command::new("rustc")
        .arg("--crate-name")
        .arg("miden_alloc_heap_base_stub")
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
        .arg(manifest_dir.join("stubs/heap_base.rs"))
        .status()
        .expect("failed to spawn rustc for heap_base stub object");
    if !status.success() {
        panic!("failed to compile heap_base stub object: {status}");
    }

    // Archive
    let status = Command::new("rust-ar")
        .arg("crs")
        .arg(&out_a)
        .arg(&out_obj)
        .status()
        .expect("failed to spawn ar for alloc stubs");
    if !status.success() {
        panic!("failed to archive alloc stubs: {status}");
    }

    // Link for dependents of this crate
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=miden_alloc_intrinsics");
}
