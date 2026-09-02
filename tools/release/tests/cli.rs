//! The `--unit` boundary, exercised through the binary.
//!
//! `Config::unit` proves a unit is *declared*; it says nothing about whether it
//! is *releasable*. A `library` or `private` unit is forbidden to declare a tag
//! or a changelog, so any command that renders one reaches an internal
//! assertion — and `set-version` reached it only after rewriting manifests and
//! running `cargo update`, leaving the workspace changed and the run ended by a
//! panic message meant for the tool's authors.
//!
//! These run the real binary rather than a library call, because the guard's
//! whole point is *where* it sits: before anything is written.

use std::{fs, path::Path, process::Command};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Two binaries released independently, sharing one library crate, plus a
/// private unit: the multi-crate shape, with a member of every kind whose
/// releasability the `--unit` commands have to distinguish.
fn fixture(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-cli-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);

    write(
        &dir.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["tool-a", "tool-b", "common", "internal"]

[workspace.package]
version = "0.4.0"
edition = "2021"
license = "MIT"

[workspace.dependencies]
fixture-common = { version = "0.4.0", path = "common" }
"#,
    );

    write(
        &dir.join("common/Cargo.toml"),
        r#"[package]
name = "fixture-common"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "fixture"
"#,
    );
    write(&dir.join("common/src/lib.rs"), "");

    for (name, version) in [("tool-a", "1.0.0"), ("tool-b", "2.0.0")] {
        write(
            &dir.join(name).join("Cargo.toml"),
            &format!(
                r#"[package]
name = "fixture-{name}"
version = "{version}"
edition.workspace = true
license.workspace = true
description = "fixture"

[dependencies]
fixture-common.workspace = true
"#
            ),
        );
        write(&dir.join(name).join("src/lib.rs"), "");
    }

    write(
        &dir.join("internal/Cargo.toml"),
        r#"[package]
name = "fixture-internal"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "fixture"
publish = false
"#,
    );
    write(&dir.join("internal/src/lib.rs"), "");

    write(
        &dir.join(".release/config.toml"),
        r#"schema-version = 2
private-version = "0.1.0"

[units.tool-a]
kind = "crates"
version-source = "own"
tag = "tool-a/v{version}"
changelog = "tool-a/CHANGELOG.md"

[units.tool-b]
kind = "crates"
version-source = "own"
tag = "tool-b/v{version}"
changelog = "tool-b/CHANGELOG.md"

# Publishes crates, is never released on its own: no tag, no changelog.
[units.common]
kind = "library"
version-source = "workspace"

[units.private]
kind = "private"

[[packages]]
name = "fixture-tool-a"
unit = "tool-a"

[[packages]]
name = "fixture-tool-b"
unit = "tool-b"

[[packages]]
name = "fixture-common"
unit = "common"

[[packages]]
name = "fixture-internal"
unit = "private"
"#,
    );

    dir
}

struct Run {
    success: bool,
    stdout: String,
    stderr: String,
}

fn release_tool(dir: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_release-tool"))
        .arg("--manifest-dir")
        .arg(dir)
        .args(args)
        .output()
        .expect("release-tool runs");
    Run {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A panic is never an acceptable outcome here: it is an assertion aimed at the
/// tool's authors, printed to a maintainer who typed a unit name.
fn assert_no_panic(run: &Run, what: &str) {
    assert!(
        !run.stderr.contains("panicked"),
        "{what} panicked instead of reporting a problem:\n{}",
        run.stderr
    );
}

#[test]
fn a_changelog_prompt_for_a_private_unit_is_refused_rather_than_panicking() {
    let dir = fixture("changelog-private");

    let run = release_tool(&dir, &["changelog-prompt", "private"]);

    assert_no_panic(&run, "changelog-prompt");
    assert!(!run.success, "a private unit has no changelog to describe:\n{}", run.stdout);
    assert!(
        run.stderr.contains("'private'") && run.stderr.contains("never released on its own"),
        "the message must say why the unit cannot be described:\n{}",
        run.stderr
    );
}

#[test]
fn a_changelog_prompt_for_a_library_unit_is_refused_rather_than_panicking() {
    let dir = fixture("changelog-library");

    let run = release_tool(&dir, &["changelog-prompt", "common"]);

    assert_no_panic(&run, "changelog-prompt");
    assert!(!run.success, "a library unit has no changelog of its own:\n{}", run.stdout);
    assert!(run.stderr.contains("'common'"), "{}", run.stderr);
}

/// A library crate publishes, so its version has to be movable — this is the
/// only command that moves one. What it must not do is record a candidate
/// entry: the unit has no tag to render (rendering one panicked, after the
/// manifests had already been rewritten), and `candidate::validate` refuses an
/// entry for a unit that is never released.
#[test]
fn setting_a_library_units_version_moves_it_and_records_no_candidate() {
    let dir = fixture("set-version-library");

    let run = release_tool(&dir, &["set-version", "--unit", "common", "0.5.0"]);

    assert_no_panic(&run, "set-version");
    assert!(run.success, "stdout:\n{}\nstderr:\n{}", run.stdout, run.stderr);

    let root = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(root.contains(r#"version = "0.5.0""#), "the library's version must move: {root}");
    assert!(
        root.contains(r#"fixture-common = { version = "0.5.0", path = "common" }"#),
        "and every requirement naming it: {root}"
    );
    assert!(
        !root.contains(r#"version = "0.4.0""#),
        "nothing may be left at the old version: {root}"
    );

    assert!(
        !dir.join(".release/release.toml").exists(),
        "a candidate entry for a unit that is never released would be rejected by `lint`"
    );
    assert!(
        run.stdout.contains("no candidate entry was recorded"),
        "the run must say so, rather than leaving the maintainer to notice:\n{}",
        run.stdout
    );

    // The sibling units are a different version domain and must not have moved.
    let a = fs::read_to_string(dir.join("tool-a/Cargo.toml")).unwrap();
    assert!(a.contains(r#"version = "1.0.0""#), "{a}");
}
