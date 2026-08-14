//! Tests for the contract `build.rs` package-cache population (#1298).
//!
//! The script under test is the file the templates and examples ship, included byte-for-byte
//! from the canonical copy (the account template); [`template_build_scripts_are_identical`]
//! pins every other copy to those bytes, so these tests cover exactly what users get. The script
//! makes plain `cargo check`/`cargo build` and IDE analysis resolve compiled dependency
//! packages: outside a midenc-driven build it locates the fingerprinted package cache with
//! `cargo miden package-cache`, populates it with a nested `cargo miden build --release`, and
//! exports `MIDENC_PACKAGE_CACHE` to macro expansion.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};

use super::basic_wallet_swapp_note_project;
use crate::cargo_proj::project;

/// The canonical contract build script; every template ships these exact bytes.
const TEMPLATE_BUILD_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../extra/templates/rust/account/template/build.rs"
));

/// Returns the repository root of this workspace.
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the integration tests live under tests/integration")
}

/// Every template and every Miden example must ship the canonical build script byte-for-byte.
///
/// This is what entitles the tests in this module to speak for all of them while executing
/// one included copy.
#[test]
fn template_build_scripts_are_identical() {
    let templates = workspace_root().join("extra").join("templates");
    let mut copies = Vec::new();

    // Every cargo-generate contract template must carry the script; discovery instead of a
    // hardcoded list, so a future template cannot escape the pin.
    for entry in fs::read_dir(templates.join("rust")).expect("failed to list the rust templates") {
        let template =
            entry.expect("failed to read a rust templates entry").path().join("template");
        if template.is_dir() {
            copies.push(template.join("build.rs"));
        }
    }
    // Every scaffold contract must carry it too.
    for entry in fs::read_dir(templates.join("project").join("contracts"))
        .expect("failed to list the scaffold contracts")
    {
        let contract = entry.expect("failed to read a scaffold contracts entry").path();
        if contract.join("miden-project.toml").is_file() {
            copies.push(contract.join("build.rs"));
        }
    }
    // And every example that is a Miden project, so IDE analysis of the examples works the
    // same way it does for generated projects.
    let examples = workspace_root().join("examples");
    for entry in fs::read_dir(&examples).expect("failed to list the examples directory") {
        let example = entry.expect("failed to read an examples entry").path();
        if example.join("miden-project.toml").is_file() {
            copies.push(example.join("build.rs"));
        }
    }
    assert!(
        copies.len() > 7,
        "the discovery walks must find the contract templates and example projects"
    );

    for copy_path in copies {
        let bytes = fs::read(&copy_path)
            .unwrap_or_else(|err| panic!("missing build.rs copy '{}': {err}", copy_path.display()));
        assert_eq!(
            bytes,
            TEMPLATE_BUILD_SCRIPT.as_bytes(),
            "'{}' differs from the canonical rust/account/template/build.rs; keep every build.rs \
             copy byte-identical",
            copy_path.display()
        );
    }
}

/// Builds the workspace's `cargo-miden` binary once and returns its path.
fn cargo_miden_binary() -> &'static Path {
    static BINARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        let workspace_root = workspace_root();
        let output = std::process::Command::new("cargo")
            .args(["build", "-p", "cargo-miden", "--bin", "cargo-miden"])
            .current_dir(workspace_root)
            .output()
            .expect("failed to spawn cargo to build cargo-miden");
        assert!(
            output.status.success(),
            "failed to build cargo-miden:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target"));
        // A relative target dir resolves against the build's working directory above.
        let target_dir = if target_dir.is_absolute() {
            target_dir
        } else {
            workspace_root.join(target_dir)
        };
        target_dir.join("debug").join("cargo-miden")
    })
}

/// Runs a plain (non-midenc) `cargo check` of `consumer`, the way an IDE does.
fn plain_cargo_check(consumer: &Path) -> Output {
    std::process::Command::new("cargo")
        .arg("check")
        .env("CARGO_MIDEN", cargo_miden_binary())
        .env_remove("MIDENC_PACKAGE_CACHE")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .current_dir(consumer)
        .output()
        .expect("failed to spawn cargo check")
}

/// Asserts one check succeeded, with its stderr in the failure message.
#[track_caller]
fn assert_check_succeeded(phase: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{phase}: plain cargo check must succeed with the template build script:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Finds the cached `basic-wallet.masp` under the consumer's fingerprinted package cache.
fn cached_basic_wallet(consumer: &Path) -> Option<PathBuf> {
    let packages_root = consumer.join("target").join("miden").join("packages");
    fs::read_dir(&packages_root)
        .ok()?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.is_dir())
        .map(|fingerprint_dir| fingerprint_dir.join("basic-wallet.masp"))
        .find(|path| path.is_file())
}

/// A plain `cargo check` (the LSP flow) must resolve dependency packages through the template
/// build script, refresh them when dependency sources change, and recover a pruned cache.
///
/// Three phases against one generated basic-wallet/swapp-note pair:
/// 1. the first check populates the fingerprinted cache with a nested
///    `cargo miden build --release` and exports `MIDENC_PACKAGE_CACHE` to macro expansion;
/// 2. editing the dependency's source re-runs the script through its `watch=` list (the
///    dependency `src` directory) and republishes a package with different contents;
/// 3. deleting the fingerprint directory re-runs the script through its missing-watched-path
///    rule and repopulates the cache.
#[test]
fn rust_sdk_build_script_populates_package_cache_for_plain_cargo_check() {
    let swapp_note_source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/components/swapp-note/src/lib.rs"
    ));
    let project = basic_wallet_swapp_note_project(
        "build_script_package_cache",
        swapp_note_source,
        Some(TEMPLATE_BUILD_SCRIPT),
    );
    let consumer = project.root().join("swapp-note");
    let dependency_source = project.root().join("basic-wallet").join("src").join("lib.rs");

    // Phase 1: the first check populates the cache.
    assert_check_succeeded("initial check", &plain_cargo_check(&consumer));
    let cached = cached_basic_wallet(&consumer).expect(
        "the first check must publish basic-wallet.masp into a fingerprint directory of the \
         consumer's package cache",
    );
    let original_package = fs::read(&cached).expect("failed to read the cached package");

    // Phase 2: a dependency source edit must reach the cache through the watch list.
    let original_source = fs::read_to_string(&dependency_source).unwrap();
    let mutation_anchor = "        self.add_asset(asset);";
    assert_eq!(
        original_source.matches(mutation_anchor).count(),
        1,
        "the basic-wallet mutation anchor must match exactly once"
    );
    let changed_source = original_source.replacen(
        mutation_anchor,
        "        self.add_asset(asset);\n        self.remove_asset(asset);\n        \
         self.add_asset(asset);",
        1,
    );
    fs::write(&dependency_source, changed_source).unwrap();

    assert_check_succeeded("check after dependency edit", &plain_cargo_check(&consumer));
    let refreshed = cached_basic_wallet(&consumer)
        .expect("the cache must still hold basic-wallet.masp after the dependency edit");
    let refreshed_package = fs::read(&refreshed).expect("failed to read the refreshed package");
    assert_ne!(
        refreshed_package, original_package,
        "editing the dependency source must republish a different basic-wallet package"
    );

    // Phase 3: a pruned cache directory is a missing watched path and must be repopulated.
    let fingerprint_dir = refreshed.parent().expect("a cached package lives in a directory");
    fs::remove_dir_all(fingerprint_dir).expect("failed to prune the package cache");

    assert_check_succeeded("check after cache prune", &plain_cargo_check(&consumer));
    assert!(
        cached_basic_wallet(&consumer).is_some(),
        "the check after pruning must repopulate the package cache"
    );
}

/// The p2id-note example must pass an IDE-style plain `cargo check` in place, through its
/// shipped build script, with the basic-wallet dependency package resolved from the cache.
///
/// Other tests build this example through the driven pipeline concurrently, and cache
/// preparation prunes every unlocked sibling fingerprint directory. The test therefore joins
/// the cache liveness protocol: it resolves its fingerprint directory up front with the same
/// `cargo miden package-cache --release` query the build script runs, and holds the shared
/// sibling lock across the check and the assertion, so concurrent pruners skip this cache the
/// same way they skip any live build's.
#[test]
fn rust_sdk_build_script_p2id_note_plain_cargo_check() {
    let consumer = workspace_root().join("examples").join("p2id-note");

    let query = std::process::Command::new(cargo_miden_binary())
        .args(["miden", "package-cache", "--release"])
        .env_remove("MIDENC_PACKAGE_CACHE")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .current_dir(&consumer)
        .output()
        .expect("failed to spawn cargo miden package-cache");
    assert!(
        query.status.success(),
        "the package-cache query must succeed:\n{}",
        String::from_utf8_lossy(&query.stderr)
    );
    let stdout = String::from_utf8(query.stdout).unwrap();
    let cache_dir = PathBuf::from(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("cache-dir="))
            .expect("the query must name the cache directory"),
    );

    let lock_path = midenc_session::package_cache_lock_path(&cache_dir);
    fs::create_dir_all(lock_path.parent().expect("a fingerprint lock has a packages parent"))
        .expect("failed to create the package cache parent");
    let cache_liveness_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .expect("failed to open the cache liveness lock");
    cache_liveness_lock
        .lock_shared()
        .expect("failed to hold the cache liveness lock");

    assert_check_succeeded("p2id-note check", &plain_cargo_check(&consumer));
    // In place, a concurrent driven build's cache could also hold basic-wallet.masp, so the
    // assertion targets this check's own fingerprint directory.
    assert!(
        cache_dir.join("basic-wallet.masp").is_file(),
        "the check must publish basic-wallet.masp into '{}'",
        cache_dir.display()
    );
    drop(cache_liveness_lock);
}

/// A missing `cargo-miden` is a hard build-script error with an actionable message.
#[test]
fn rust_sdk_build_script_fails_without_cargo_miden() {
    let project = project("build_script_missing_tool")
        .file(
            "Cargo.toml",
            r#"
[package]
name = "missing-tool"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["rlib"]
"#,
        )
        .file(
            "miden-project.toml",
            r#"
[package]
name = "missing-tool"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden:missing-tool/missing-tool@0.1.0"
path = "src/lib.rs"
"#,
        )
        .file("build.rs", TEMPLATE_BUILD_SCRIPT)
        .file("src/lib.rs", "")
        .build();

    let output = std::process::Command::new("cargo")
        .arg("check")
        .env("CARGO_MIDEN", project.root().join("definitely-missing-cargo-miden"))
        .env_remove("MIDENC_PACKAGE_CACHE")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(project.root())
        .output()
        .expect("failed to spawn cargo check");
    assert!(!output.status.success(), "cargo check must fail without cargo-miden");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to run `cargo miden package-cache`"),
        "the build script must name the missing tool, got:\n{stderr}"
    );
}
