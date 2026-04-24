//! Differential fuzzing harness for the Miden compiler.
//!
//! Each test case is a self-contained `#![no_std]` Rust crate that defines
//! `entrypoint(u32, u32) -> u32`, checked in under `src/cases/`. The harness
//! builds every case twice — natively as a host `cdylib` and via `cargo-miden`
//! to a MASM package — then runs both over random `(u32, u32)` inputs and
//! asserts the results match.

use std::{path::PathBuf, process::Command};

use miden_core::Felt;
use miden_integration_tests::{CompilerTest, project, testing::executor_with_std};
use midenc_frontend_wasm::WasmTranslationConfig;
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, TestRunner},
};

/// Compiles `source` for the host and for MASM, then compares the
/// `entrypoint(u32, u32) -> u32` outputs across 16 random input pairs.
///
/// `name` must be unique per case; it is used as the generated package name.
pub fn run_case(name: &str, source: &str) {
    let pkg_name = format!("fuzza_{name}");

    let masm_proj = project(&format!("{pkg_name}_masm"))
        .file("Cargo.toml", &cargo_toml(&pkg_name))
        .file("src/lib.rs", source)
        .build();
    let mut test = CompilerTest::rust_source_cargo_miden(
        masm_proj.root(),
        WasmTranslationConfig::default(),
        [],
    );
    let package = test.compile_package();

    let native_proj = project(&format!("{pkg_name}_native"))
        .file("Cargo.toml", &cargo_toml(&pkg_name))
        .file("src/lib.rs", source)
        .build();
    let dylib_path = build_host_cdylib(&native_proj.root(), &pkg_name);

    let lib = unsafe { libloading::Library::new(&dylib_path) }
        .unwrap_or_else(|e| panic!("failed to load {}: {e}", dylib_path.display()));
    type EntryFn = unsafe extern "C" fn(u32, u32) -> u32;
    let entry: libloading::Symbol<EntryFn> = unsafe { lib.get(b"entrypoint\0") }
        .unwrap_or_else(|e| panic!("missing `entrypoint` in {}: {e}", dylib_path.display()));

    // Proptest: 16 cases, shrinking disabled — the whole case file IS the
    // reduced reproducer, so shrinking individual inputs adds no value.
    let cfg = Config {
        cases: 16,
        max_shrink_iters: 0,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..Config::default()
    };
    TestRunner::new(cfg)
        .run(&(any::<u32>(), any::<u32>()), |(a, b)| {
            let native_out = unsafe { entry(a, b) };
            let exec =
                executor_with_std(vec![Felt::new(a as u64), Felt::new(b as u64)], Some(&package));
            let masm_out: u32 =
                exec.execute_into(&package.unwrap_program(), test.session.source_manager.clone());
            prop_assert_eq!(
                native_out,
                masm_out,
                "native vs masm mismatch for inputs ({}, {})",
                a,
                b
            );
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{name}: {err}"));
}

fn cargo_toml(pkg_name: &str) -> String {
    format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = 3
panic = "abort"

[profile.dev]
panic = "abort"
"#
    )
}

/// Build `project_root` as a host-target release cdylib and return the produced library path.
fn build_host_cdylib(project_root: &std::path::Path, pkg_name: &str) -> PathBuf {
    // A `no_std` cdylib normally drops the platform runtime libraries, which on
    // macOS leaves `dyld_stub_binder` unresolved at link time. Force rustc to
    // link the default platform libs (libSystem/libc) so the resulting dylib is
    // loadable via `libloading`.
    let status = Command::new("cargo")
        .current_dir(project_root)
        .args(["build", "--release", "--lib"])
        .env("RUSTFLAGS", "-C default-linker-libraries=yes")
        .status()
        .expect("failed to spawn cargo for native build");
    assert!(status.success(), "native cargo build failed for `{pkg_name}`");

    let base = project_root.join("target").join("release");
    for leaf in [
        format!("lib{pkg_name}.dylib"),
        format!("lib{pkg_name}.so"),
        format!("{pkg_name}.dll"),
    ] {
        let candidate = base.join(leaf);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("cdylib artifact for `{pkg_name}` not found under {}", base.display());
}

#[cfg(test)]
mod tests;
