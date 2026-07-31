//! End-to-end version bumps against a real workspace on disk.
//!
//! The fixture reproduces the two shapes this repository actually has: crates
//! that inherit the workspace version, and a second version domain whose crates
//! carry their own version and are depended on from the first. That second edge
//! is the one worth testing, because forgetting it leaves the two domains
//! pinned to different versions of a shared contract.

use std::{fs, path::Path};

use midenc_release::{
    config::{Config, VersionSource},
    version,
    workspace::Workspace,
};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A workspace with a `compiler` domain at 0.9.2 and an `sdk` domain at 0.13.1,
/// where a compiler crate depends on an SDK crate.
fn fixture(dir: &Path) {
    write(
        &dir.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["comp", "shared", "shared-dep", "shared-tests"]

[workspace.package]
version = "0.9.2"
edition = "2021"
license = "MIT"

[workspace.dependencies]
fixture-comp = { version = "0.9.2", path = "comp" }
# Lives in the sdk domain but is consumed by the compiler domain.
fixture-shared = { version = "0.13.1", path = "shared" }
fixture-shared-dep = { version = "0.13.1", path = "shared-dep" }
"#,
    );

    write(
        &dir.join("comp/Cargo.toml"),
        r#"[package]
name = "fixture-comp"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "fixture"

[dependencies]
fixture-shared.workspace = true
"#,
    );
    write(&dir.join("comp/src/lib.rs"), "");

    write(
        &dir.join("shared/Cargo.toml"),
        r#"[package]
name = "fixture-shared"
version = "0.13.1"
edition.workspace = true
license.workspace = true
description = "fixture"

[dependencies]
fixture-shared-dep.workspace = true
"#,
    );
    write(&dir.join("shared/src/lib.rs"), "");

    write(
        &dir.join("shared-dep/Cargo.toml"),
        r#"[package]
name = "fixture-shared-dep"
version = "0.13.1"
edition.workspace = true
license.workspace = true
description = "fixture"
"#,
    );
    write(&dir.join("shared-dep/src/lib.rs"), "");

    // Private, and versioned like the sdk domain it lives beside.
    write(
        &dir.join("shared-tests/Cargo.toml"),
        r#"[package]
name = "fixture-shared-tests"
version = "0.13.1"
edition.workspace = true
license.workspace = true
description = "fixture"
publish = false

[dependencies]
fixture-shared.workspace = true
"#,
    );
    write(&dir.join("shared-tests/src/lib.rs"), "");

    write(
        &dir.join(".release/config.toml"),
        r#"schema-version = 1

[units.compiler]
tag = "v{version}"
changelog = "CHANGELOG.md"

[units.sdk]
tag = "sdk/v{version}"
changelog = "sdk/CHANGELOG.md"

[[packages]]
name = "fixture-comp"
unit = "compiler"
publish = true
version-source = "workspace"

[[packages]]
name = "fixture-shared"
unit = "sdk"
publish = true
version-source = "sdk"

[[packages]]
name = "fixture-shared-dep"
unit = "sdk"
publish = true
version-source = "sdk"

[[packages]]
name = "fixture-shared-tests"
unit = "private"
publish = false
"#,
    );
}

fn setup(label: &str) -> (std::path::PathBuf, Workspace, Config) {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-version-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fixture(&dir);

    let ws = Workspace::load(&dir).unwrap();
    let config = Config::load(&dir.join(".release/config.toml")).unwrap();
    (dir, ws, config)
}

#[test]
fn bumping_the_compiler_domain_moves_the_workspace_version_and_its_requirements() {
    let (dir, ws, config) = setup("compiler");

    let plan = version::plan(&ws, &config, VersionSource::Workspace, None).unwrap();
    assert_eq!(plan.old.to_string(), "0.9.2");
    assert_eq!(plan.new.to_string(), "0.10.0", "default bump is the next minor");

    version::apply(&ws, &config, &plan).unwrap();

    let root = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(root.contains(r#"version = "0.10.0""#), "{root}");
    assert!(
        root.contains(r#"fixture-comp = { version = "0.10.0", path = "comp" }"#),
        "the workspace requirement must move with the version: {root}"
    );
    assert!(
        root.contains(r#"fixture-shared = { version = "0.13.1", path = "shared" }"#),
        "the sdk domain must not move: {root}"
    );

    // The comment above the shared dependency survives the rewrite.
    assert!(root.contains("# Lives in the sdk domain"), "{root}");

    let shared = fs::read_to_string(dir.join("shared/Cargo.toml")).unwrap();
    assert!(shared.contains(r#"version = "0.13.1""#), "sdk crate unchanged: {shared}");
}

#[test]
fn bumping_the_sdk_domain_rewrites_the_compiler_side_requirement() {
    let (dir, ws, config) = setup("sdk");

    let plan = version::plan(&ws, &config, VersionSource::Sdk, None).unwrap();
    assert_eq!(plan.old.to_string(), "0.13.1");
    assert_eq!(plan.new.to_string(), "0.14.0");

    version::apply(&ws, &config, &plan).unwrap();

    let shared = fs::read_to_string(dir.join("shared/Cargo.toml")).unwrap();
    assert!(shared.contains(r#"version = "0.14.0""#), "{shared}");
    let shared_dep = fs::read_to_string(dir.join("shared-dep/Cargo.toml")).unwrap();
    assert!(
        shared_dep.contains(r#"version = "0.14.0""#),
        "the whole domain moves: {shared_dep}"
    );

    let root = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(
        root.contains(r#"fixture-shared = { version = "0.14.0", path = "shared" }"#),
        "an sdk bump must rewrite the requirement held by the compiler domain: {root}"
    );
    assert!(
        root.contains(r#"version = "0.9.2""#),
        "the workspace version must not move on an sdk bump: {root}"
    );

    // The whole point: after the bump the workspace still resolves.
    let ws = Workspace::load(&dir).unwrap();
    assert_eq!(ws.packages["fixture-shared"].version, "0.14.0");
    assert_eq!(ws.packages["fixture-shared-dep"].version, "0.14.0");
    assert_eq!(ws.packages["fixture-comp"].version, "0.9.2");
}

#[test]
fn an_explicit_version_overrides_the_default_bump() {
    let (_dir, ws, config) = setup("explicit");
    let requested = semver::Version::parse("1.0.0-rc.1").unwrap();
    let plan = version::plan(&ws, &config, VersionSource::Workspace, Some(requested)).unwrap();
    assert_eq!(plan.new.to_string(), "1.0.0-rc.1", "prereleases are selectable");
}

#[test]
fn downgrades_are_refused() {
    let (_dir, ws, config) = setup("downgrade");
    let older = semver::Version::parse("0.9.1").unwrap();
    let err = version::plan(&ws, &config, VersionSource::Workspace, Some(older))
        .unwrap_err()
        .to_string();
    assert!(err.contains("must increase"), "{err}");
}

#[test]
fn republishing_the_same_version_is_refused() {
    let (_dir, ws, config) = setup("same");
    let same = semver::Version::parse("0.9.2").unwrap();
    let err = version::plan(&ws, &config, VersionSource::Workspace, Some(same))
        .unwrap_err()
        .to_string();
    assert!(err.contains("must increase"), "{err}");
}

#[test]
fn a_domain_whose_packages_disagree_is_rejected() {
    let (dir, _ws, config) = setup("disagree");

    // Knock one crate out of step with its domain.
    let manifest = dir.join("shared/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap().replace("0.13.1", "0.13.2");
    fs::write(&manifest, text).unwrap();

    let ws = Workspace::load(&dir).unwrap();
    let err = version::plan(&ws, &config, VersionSource::Sdk, None).unwrap_err().to_string();
    assert!(err.contains("disagree"), "{err}");
    assert!(err.contains("fixture-shared"), "the offending package is named: {err}");
}

#[test]
fn planning_writes_nothing() {
    let (dir, ws, config) = setup("dry-run");
    let before = fs::read_to_string(dir.join("Cargo.toml")).unwrap();

    let plan = version::plan(&ws, &config, VersionSource::Workspace, None).unwrap();
    assert!(!plan.edits.is_empty(), "the plan should describe real edits");

    let after = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert_eq!(before, after, "planning must not touch the workspace");
}

#[test]
fn private_packages_are_left_at_their_own_version() {
    let (dir, ws, config) = setup("private");

    let plan = version::plan(&ws, &config, VersionSource::Sdk, None).unwrap();
    version::apply(&ws, &config, &plan).unwrap();

    let private = fs::read_to_string(dir.join("shared-tests/Cargo.toml")).unwrap();
    assert!(
        private.contains(r#"version = "0.13.1""#),
        "a private package is never published, so its version is not part of any domain: {private}"
    );

    // It still resolves, because its dependency is workspace-inherited.
    let ws = Workspace::load(&dir).unwrap();
    assert_eq!(ws.packages["fixture-shared-tests"].version, "0.13.1");
    assert_eq!(ws.packages["fixture-shared"].version, "0.14.0");
}
