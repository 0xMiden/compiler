//! Tests for the contract `build.rs` package-cache population (#1298).
//!
//! The script under test is the file the templates and examples ship, included byte-for-byte
//! from the canonical copy (the account template); [`template_build_scripts_are_identical`]
//! pins every other copy to those bytes, so these tests cover exactly what users get. The script
//! makes plain `cargo check`/`cargo build` and IDE analysis resolve compiled dependency
//! packages: outside a midenc-driven build it stages a package cache under its `OUT_DIR`, fills
//! it with a nested `cargo miden build --release` that adopts the staged directory through
//! `MIDENC_PACKAGE_CACHE`, and exports the same variable to macro expansion.

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

/// Finds a build script's staged package cache under `target_root`'s cargo target directory.
///
/// The script stages the cache in its `OUT_DIR`, whose path embeds a cargo-chosen hash:
/// `<target>[/<triple>]/debug/build/<crate>-<hash>/out/miden-packages`. The fixtures pin a
/// wasm build target in `.cargo/config.toml`, which puts the script's run directory under
/// the triple subtree, so the scan covers the host layout and every per-triple layout.
/// `target_root` is the directory that owns the check's `target/` — the workspace root for
/// the generated pair, the example itself for the standalone p2id check — and `crate_name`
/// is the cargo package name of the crate whose script staged the cache.
fn staged_package(target_root: &Path, crate_name: &str, expected_package: &str) -> Option<PathBuf> {
    let target = target_root.join("target");
    let mut build_roots = vec![target.join("debug").join("build")];
    if let Ok(entries) = fs::read_dir(&target) {
        for entry in entries.filter_map(Result::ok) {
            build_roots.push(entry.path().join("debug").join("build"));
        }
    }
    build_roots
        .into_iter()
        .filter_map(|build_root| fs::read_dir(build_root).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(crate_name))
        })
        .map(|build_dir| build_dir.join("out").join("miden-packages").join(expected_package))
        .find(|path| path.is_file())
}

/// Finds the staged `basic-wallet.masp` of the swapp-note consumer in its workspace.
fn cached_basic_wallet(workspace_root_dir: &Path) -> Option<PathBuf> {
    staged_package(workspace_root_dir, "swapp-note", "basic-wallet.masp")
}

/// A plain `cargo check` (the LSP flow) must resolve dependency packages through the template
/// build script and re-stage them when the script re-runs.
///
/// Two phases against one generated basic-wallet/swapp-note pair:
/// 1. the first check stages the packages under the script's `OUT_DIR` with a nested
///    `cargo miden build --release` and exports `MIDENC_PACKAGE_CACHE` to macro expansion;
/// 2. after a dependency source edit, touching the consumer's manifest re-runs the script —
///    the always-rebuild trigger the script documents — and re-stages a package with
///    different contents.
///
/// The script watches only the consumer's manifests, by design: a dependency source edit
/// alone does not re-run it (computing a precise trigger set would re-implement build
/// provenance), and the staged packages stay as they are until a manifest touch — the
/// recovery phase 2 exercises — or a `cargo clean` discards the staging.
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
    let consumer_manifest = consumer.join("miden-project.toml");

    // The project builder rewrites only changed files and keeps `target/` to cache across
    // test runs, so a previous run's staging can look fresh to cargo. Touch the consumer
    // manifest before each phase, so each check provably re-runs the script and stages
    // from the sources as they are now.
    let touch_consumer_manifest = || {
        let manifest_bytes = fs::read(&consumer_manifest).unwrap();
        fs::write(&consumer_manifest, manifest_bytes).unwrap();
    };

    // Phase 1: the check stages the dependency package built from the original sources.
    touch_consumer_manifest();
    assert_check_succeeded("initial check", &plain_cargo_check(&consumer));
    let cached = cached_basic_wallet(&project.root())
        .expect("the first check must stage basic-wallet.masp under the consumer's build OUT_DIR");
    let original_package = fs::read(&cached).expect("failed to read the cached package");

    // Phase 2: after a dependency source edit, a consumer-manifest touch re-stages.
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
    // The script watches the consumer's manifests, not the dependency's sources; the touch
    // is the documented way to ask for a re-stage.
    touch_consumer_manifest();

    assert_check_succeeded("check after dependency edit", &plain_cargo_check(&consumer));
    let refreshed = cached_basic_wallet(&project.root())
        .expect("the staging must still hold basic-wallet.masp after the manifest touch");
    let refreshed_package = fs::read(&refreshed).expect("failed to read the refreshed package");
    assert_ne!(
        refreshed_package, original_package,
        "the re-run script must re-stage the edited basic-wallet package"
    );
}

/// The p2id-note example must pass an IDE-style plain `cargo check` in place, through its
/// shipped build script, with the basic-wallet dependency package resolved from the staged
/// package cache.
///
/// The check passing is the load-bearing assertion: the macros can only expand when the
/// staged packages are readable. Concurrent driven builds of this example exchange their
/// packages through their own per-build leases and never touch the staging, so no
/// cross-test locking is needed. The staging may survive from an earlier run, so the
/// package-existence assertion does not attribute the file to this run.
#[test]
fn rust_sdk_build_script_p2id_note_plain_cargo_check() {
    let consumer = workspace_root().join("examples").join("p2id-note");

    assert_check_succeeded("p2id-note check", &plain_cargo_check(&consumer));
    // The example's cargo package is named `p2id`, and that name keys its build directory.
    let exported = staged_package(&consumer, "p2id", "basic-wallet.masp");
    assert!(
        exported.is_some(),
        "the check must stage basic-wallet.masp under p2id-note's build OUT_DIR"
    );
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
        stderr.contains("failed to run `cargo miden build`"),
        "the build script must name the missing tool, got:\n{stderr}"
    );
}
