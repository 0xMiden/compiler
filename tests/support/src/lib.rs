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
    let _build_lock = example_build_lock(&workspace_root());
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

/// Locks shared example outputs while one build is running.
pub fn example_build_lock(workspace: &Path) -> File {
    let target_dir = workspace.join("target");
    fs::create_dir_all(&target_dir).expect("failed to create the workspace target directory");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target_dir.join("example-build.lock"))
        .expect("failed to open the example build lock");
    lock.lock().expect("failed to lock example builds");
    lock
}

/// Writes a Miden package through a same-directory temporary file and atomic rename.
pub fn write_masp_file_atomic(
    package: &Package,
    output_dir: impl AsRef<Path>,
) -> std::io::Result<()> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)?;
    let temporary = tempfile::Builder::new()
        .prefix(".miden-package-")
        .tempfile_in(output_dir)?
        .into_temp_path();
    package.write_to_file(&temporary)?;
    let package_name: &str = &package.name;
    let destination = output_dir.join(package_name).with_extension(Package::EXTENSION);
    fs::rename(&temporary, destination)
}

/// Removes outer build settings that would poison a nested Cargo invocation.
pub fn scrub_nested_cargo_env(cmd: &mut Command) {
    for variable in [
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_TARGET_WASM32_WASIP2_RUSTFLAGS",
        "RUSTFLAGS",
    ] {
        cmd.env_remove(variable);
    }
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
