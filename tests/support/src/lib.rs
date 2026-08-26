//! Shared support infrastructure for integration tests.
#![deny(warnings)]
#![deny(missing_docs)]

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use miden_mast_package::Package;
use midenc_frontend_wasm::WasmTranslationConfig;

/// Utilities for generating on-disk Cargo projects for tests.
pub mod cargo_proj;
/// Compiler test builders and pipeline assertions.
pub mod compiler_test;
/// VM execution, initialization, and session setup helpers.
pub mod testing;

/// Represents an on-disk Cargo project generated for tests.
pub use self::cargo_proj::Project;
/// Builder for constructing on-disk Cargo projects used by tests.
pub use self::cargo_proj::ProjectBuilder;
/// Generates an on-disk Cargo project in the Cargo target directory for use in tests.
pub use self::cargo_proj::project;
pub use self::{
    compiler_test::{CargoTest, CompilerTest, CompilerTestBuilder, RustcTest, WasmTest},
    testing::setup::default_session,
};

/// Compiles one Cargo Miden project without debug output.
pub fn compile_project(project_path: &Path) -> Arc<Package> {
    let mut test = CompilerTest::rust_source_cargo_miden(
        project_path,
        WasmTranslationConfig::default(),
        ["--debug".to_owned(), "none".to_owned()],
    );
    test.compile_package()
}

/// Returns the compiler workspace root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Locks the shared p2id example outputs for the full build and consume span.
pub fn p2id_build_lock(workspace: &Path) -> File {
    let target_dir = workspace.join("target");
    fs::create_dir_all(&target_dir).expect("failed to create the workspace target directory");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target_dir.join("p2id-end-to-end-build.lock"))
        .expect("failed to open the p2id end-to-end build lock");
    lock.lock().expect("failed to lock the p2id end-to-end build");
    lock
}

/// Returns true when rustup reports the codec component target as installed.
pub fn wasm_target_is_installed() -> bool {
    const WASM_TARGET: &str = "wasm32-wasip2";

    let output = match Command::new("rustup").args(["target", "list"]).output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!("`rustup target list` failed:\n{}", String::from_utf8_lossy(&output.stderr));
            return false;
        }
        Err(error) => {
            eprintln!("could not run `rustup target list`: {error}");
            return false;
        }
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.starts_with(WASM_TARGET) && line.contains("(installed)"))
}
