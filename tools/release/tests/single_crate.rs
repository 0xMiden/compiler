//! A repository that is one package, with no workspace.
//!
//! Every other fixture in this crate is a workspace, which is how the root
//! manifest's `[package].version` came to be unreachable from `set-version`.

use std::{fs, path::Path, process::Command};

use midenc_release::{config::Config, version, workspace::Workspace};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

// `changelog::prepare` shells out to `git log`, which is fatal outside a
// repository regardless of the pathspec — so the fixture needs a real commit
// for the changelog test to reach the assertion it's checking.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn fixture(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-single-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // No [workspace] table at all: the version lives at [package].version.
    write(
        &dir.join("Cargo.toml"),
        r#"[package]
name = "mytool"
version = "0.3.0"
edition = "2021"
"#,
    );
    write(&dir.join("src/main.rs"), "fn main() {}\n");
    write(
        &dir.join(".release/config.toml"),
        r#"schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
latest = true

[[packages]]
name = "mytool"
unit = "main"
"#,
    );

    git(&dir, &["init", "-q"]);
    git(
        &dir,
        &["-c", "user.name=test", "-c", "user.email=test@example.com", "add", "-A"],
    );
    git(
        &dir,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    );

    dir
}

#[test]
fn a_single_crate_repositorys_version_can_be_bumped() {
    let dir = fixture("bump");
    let ws = Workspace::load(&dir).unwrap();
    assert_eq!(
        ws.root.canonicalize().unwrap(),
        dir.canonicalize().unwrap(),
        "fixture must be its own workspace root, not swallowed by an enclosing workspace"
    );
    let config = Config::load(&dir.join(".release/config.toml")).unwrap();

    let plan = version::plan(
        &ws,
        &config,
        "main",
        Some(semver::Version::parse("0.4.0").unwrap()),
        version::Force::No,
    )
    .unwrap();

    assert_eq!(plan.old.to_string(), "0.3.0");
    assert!(!plan.edits.is_empty(), "the root [package].version must be an edit");

    version::apply(&ws, &config, &plan).unwrap();
    let after = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(after.contains(r#"version = "0.4.0""#), "{after}");
}

#[test]
fn a_root_package_has_a_usable_changelog_path() {
    let dir = fixture("paths");
    let ws = Workspace::load(&dir).unwrap();
    assert_eq!(
        ws.root.canonicalize().unwrap(),
        dir.canonicalize().unwrap(),
        "fixture must be its own workspace root, not swallowed by an enclosing workspace"
    );
    let config = Config::load(&dir.join(".release/config.toml")).unwrap();

    // `.`, not `""`: an empty pathspec is fatal to `git log`.
    let prompt = midenc_release::changelog::prepare(&ws, &config, "main", Some("HEAD".into()));
    let paths = prompt.map(|p| p.paths).unwrap_or_default();
    assert_eq!(paths, ["."], "a root package's path must be '.'");
}
