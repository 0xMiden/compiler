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
use midenc_frontend_wasm_metadata::NESTED_CARGO_SCRUB_ENV;

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

/// Writes a Miden package through the production atomic package publisher.
pub fn write_masp_file_atomic(
    package: &Package,
    output_dir: impl AsRef<Path>,
) -> std::io::Result<()> {
    midenc_session::registry::write_package_atomically(package, output_dir.as_ref())
        .map(|_| ())
        .map_err(std::io::Error::other)
}

/// Removes outer build settings that would poison a nested Cargo invocation.
pub fn scrub_nested_cargo_env(cmd: &mut Command) {
    for &variable in NESTED_CARGO_SCRUB_ENV {
        cmd.env_remove(variable);
    }
}

/// Returns true when the active Rust sysroot contains the codec component target.
pub fn wasm_target_is_installed() -> bool {
    const WASM_TARGET: &str = "wasm32-wasip2";

    let output = match Command::new("rustc").args(["--print", "sysroot"]).output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!(
                "`rustc --print sysroot` failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return false;
        }
        Err(error) => {
            eprintln!("could not run `rustc --print sysroot` (rustup may be unavailable): {error}");
            return false;
        }
    };
    let sysroot = Path::new(String::from_utf8_lossy(&output.stdout).trim()).to_path_buf();
    sysroot.join("lib").join("rustlib").join(WASM_TARGET).exists()
}
