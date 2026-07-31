//! Closure verification against real workspaces.
//!
//! The point of these tests is the negative cases. A gate that passes on good
//! input proves little; what matters is that it fails on a crate which builds
//! perfectly well in the workspace and is broken once packaged. That is exactly
//! the failure production cannot catch, because it publishes with `--no-verify`.

use std::{fs, path::Path};

use midenc_release::closure::{self, Options};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-closure-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Two interdependent crates with no third-party dependencies, so the test
/// needs no network.
fn fixture(dir: &Path, root_extra: &str, root_src: &str) {
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"leaf\", \"root\"]\n",
    );

    write(
        &dir.join("leaf/Cargo.toml"),
        "[package]\nname = \"closure-leaf\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \
         \"MIT\"\ndescription = \"fixture\"\n",
    );
    write(&dir.join("leaf/src/lib.rs"), "pub fn leaf() -> u32 { 1 }\n");

    write(
        &dir.join("root/Cargo.toml"),
        &format!(
            "[package]\nname = \"closure-root\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense \
             = \"MIT\"\ndescription = \"fixture\"\n{root_extra}\n[dependencies]\nclosure-leaf = \
             {{ version = \"0.1.0\", path = \"../leaf\" }}\n"
        ),
    );
    write(&dir.join("root/src/lib.rs"), root_src);
    // Cargo requires a lockfile for `--locked`.
    fs::write(dir.join("Cargo.lock"), "").ok();
}

fn options(packages: &[&str], build: bool) -> Options {
    Options {
        packages: packages.iter().map(|p| p.to_string()).collect(),
        build_consumer: build,
        allow_upstream: false,
        cache_dir: None,
    }
}

fn generate_lockfile(dir: &Path) {
    let status = std::process::Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
        .current_dir(dir)
        .args(["generate-lockfile"])
        .status()
        .unwrap();
    assert!(status.success(), "failed to generate a lockfile for the fixture");
}

#[test]
fn a_sound_closure_packages_resolves_and_builds() {
    let dir = temp_dir("sound");
    fixture(&dir, "", "pub fn root() -> u32 { closure_leaf::leaf() + 1 }\n");
    generate_lockfile(&dir);

    let result = closure::verify(&dir, &options(&["closure-leaf", "closure-root"], true)).unwrap();

    assert_eq!(result.crates.len(), 2);
    assert_eq!(result.crates[0].name, "closure-leaf", "dependency order is preserved");
    assert_eq!(result.crates[1].name, "closure-root");
    for packaged in &result.crates {
        assert_eq!(packaged.digest.len(), 64, "digest is a sha256 hex string");
        assert!(packaged.size > 0);
    }
}

/// The case the gate exists for: a crate that compiles in the workspace but
/// whose archive is missing a file it needs. `exclude` drops the module from
/// the package while leaving the workspace build perfectly healthy.
#[test]
fn a_crate_whose_archive_omits_a_source_file_is_rejected() {
    let dir = temp_dir("missing-file");
    fixture(
        &dir,
        "exclude = [\"src/helper.rs\"]\n",
        "mod helper;\npub fn root() -> u32 { helper::help() }\n",
    );
    write(&dir.join("root/src/helper.rs"), "pub fn help() -> u32 { 7 }\n");
    generate_lockfile(&dir);

    // Sanity: the workspace itself builds, so only packaging is broken.
    let status = std::process::Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
        .current_dir(&dir)
        .args(["build", "-p", "closure-root"])
        .status()
        .unwrap();
    assert!(status.success(), "the fixture must build in the workspace");

    let err = closure::verify(&dir, &options(&["closure-leaf", "closure-root"], true))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("do not build when resolved from a registry"),
        "the consumer build is what catches this: {err}"
    );
    // Assert on the underlying compiler error too. Without this the test would
    // also pass if the consumer failed to build for an unrelated reason.
    assert!(
        err.contains("helper") || err.contains("file not found"),
        "the failure must be the missing module, not something incidental: {err}"
    );
}

/// Skipping the consumer build is much weaker, and this pins that difference:
/// the same broken crate passes when only resolution is checked.
#[test]
fn skipping_the_consumer_build_misses_what_only_a_build_can_catch() {
    let dir = temp_dir("no-build");
    fixture(
        &dir,
        "exclude = [\"src/helper.rs\"]\n",
        "mod helper;\npub fn root() -> u32 { helper::help() }\n",
    );
    write(&dir.join("root/src/helper.rs"), "pub fn help() -> u32 { 7 }\n");
    generate_lockfile(&dir);

    let result = closure::verify(&dir, &options(&["closure-leaf", "closure-root"], false));
    assert!(
        result.is_ok(),
        "resolution alone cannot see inside an archive; this is why the build is the default"
    );
}

#[test]
fn every_selected_package_must_reach_the_registry() {
    let dir = temp_dir("missing-package");
    fixture(&dir, "", "pub fn root() -> u32 { closure_leaf::leaf() }\n");
    generate_lockfile(&dir);

    let err = closure::verify(&dir, &options(&["closure-leaf", "not-a-package"], false))
        .unwrap_err()
        .to_string();
    assert!(err.contains("not-a-package"), "{err}");
}
