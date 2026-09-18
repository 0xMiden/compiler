//! A Miden Assembly project as the root of a `midenc` build, and the options that locate it.
//!
//! Every Miden Assembly project elsewhere in the workspace's tests is either handed to the
//! compiler by manifest path or reached as a dependency of a Rust root. These tests pin the
//! locator rules on a project that only the Miden manifest can name: the working directory by
//! default, `--working-dir` and `--manifest-path` when given, and the refusal when an input file
//! and a `--manifest-path` disagree. The driver is called as a library, which is what `midenc`
//! and `cargo miden build` both do.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use midenc_driver::{Midenc, diagnostics::Report};

/// The library's root module, with a submodule so that the sources are a module tree.
const ROOT: &str = "\
pub mod support

pub proc entry() -> u32
    push.1
    exec.support::clean
end
";

/// The submodule [`ROOT`] declares.
const SUPPORT: &str = "\
pub proc clean
    push.1
    u32wrapping_add
end
";

/// The project's manifest: a library rooted at `lib/mod.masm`, with nothing to depend on.
const MANIFEST: &str = r#"[package]
name = "masm-root"
version = "0.1.0"

[lib]
path = "lib/mod.masm"

[dependencies]
"#;

/// Writes the Miden Assembly project into `<parent>/masm-root` and returns that directory.
///
/// The project holds no `Cargo.toml`, which is what makes it a test of the Miden manifest: no
/// Cargo flavor can stand in for it, in any of the locator rules below.
fn masm_project(parent: &Path) -> PathBuf {
    let project = parent.join("masm-root");
    fs::create_dir_all(project.join("lib")).unwrap();
    fs::write(project.join("miden-project.toml"), MANIFEST).unwrap();
    fs::write(project.join("lib/mod.masm"), ROOT).unwrap();
    fs::write(project.join("lib/support.masm"), SUPPORT).unwrap();
    assert!(
        !project.join("Cargo.toml").exists(),
        "the project must be MASM-only, so that only the Miden manifest can name it"
    );
    project
}

/// Runs the driver from `cwd` with `args`, as a command line without its program name.
///
/// `Ok(Some(path))` is the package a finished build wrote, `Ok(None)` a deliberate stop.
fn midenc(cwd: &Path, args: &[&str]) -> Result<Option<PathBuf>, Report> {
    let argv = std::iter::once(OsString::from("midenc")).chain(args.iter().map(OsString::from));
    Midenc::exec(cwd.to_path_buf(), argv, None)
}

/// Every `.masp` file anywhere under `dir`; a directory that does not exist holds none.
fn masp_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.map(Result::unwrap) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(masp_files(&path));
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("masp") {
            found.push(path);
        }
    }
    found
}

/// A project directory is all the compiler needs: `miden build` with no arguments builds it.
///
/// This pins the default locator: a directory holding a `miden-project.toml` and no `Cargo.toml`
/// names its project by the Miden manifest, the Miden Assembly frontend builds it, and a package
/// comes out.
#[test]
fn a_masm_project_is_built_from_its_own_directory_without_arguments() {
    let scratch = tempfile::TempDir::new().unwrap();
    let project = masm_project(scratch.path());

    // No arguments at all: the working directory is what locates the project.
    let package = midenc(&project, &[])
        .unwrap_or_else(|err| panic!("midenc failed to build the MASM project: {err}"))
        .expect("a build that ran to completion writes a package");

    assert!(package.is_file(), "the compiled package must exist: {}", package.display());
    assert_eq!(
        package.extension().and_then(|extension| extension.to_str()),
        Some("masp"),
        "the build must produce a Miden package: {}",
        package.display()
    );
    assert!(
        package.starts_with(&project),
        "the package must land under the project's own target directory: {}",
        package.display()
    );
}

/// `-o` and `--manifest-path`, run from a directory that holds no project — issue 1391.
///
/// This is the command the project template's build helper runs, verbatim: the only project
/// anything can find is the one `--manifest-path` names, and the package lands exactly where
/// `-o` says.
#[test]
fn the_issues_command_shape_builds_a_project_named_by_manifest_path_from_elsewhere() {
    let scratch = tempfile::TempDir::new().unwrap();
    let project = masm_project(scratch.path());
    let elsewhere = scratch.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let out = elsewhere.join("out.masp");

    let package = midenc(
        &elsewhere,
        &[
            "-o",
            out.to_str().unwrap(),
            "--manifest-path",
            project.join("miden-project.toml").to_str().unwrap(),
        ],
    )
    .unwrap_or_else(|err| {
        panic!("midenc failed to build the project named by --manifest-path: {err}")
    })
    .expect("a build that ran to completion writes a package");

    assert_eq!(package, out, "the package must be the file `-o` named");
    assert!(out.is_file(), "the compiled package must exist: {}", out.display());
}

/// A stop before assembly writes no package and reports none.
///
/// The SDK's build-script support relies on this contract through
/// `cargo miden build --stop-after=dependencies`: it wants the dependencies materialized, and
/// nothing of the root project left behind.
#[test]
fn a_deliberate_stop_writes_nothing_and_returns_nothing() {
    let scratch = tempfile::TempDir::new().unwrap();
    let project = masm_project(scratch.path());

    let stopped = midenc(&project, &["--stop-after=parse"])
        .unwrap_or_else(|err| panic!("midenc failed to stop after parsing: {err}"));

    assert_eq!(stopped, None, "a stopped run has no package to name");
    let written = masp_files(&project);
    assert!(written.is_empty(), "a stopped run must write no package, but wrote {written:?}");
}

/// `--working-dir` moves the directory the default locator looks in.
///
/// The default locator is the one option-free rule that is resolved in the working directory, so
/// pointing `--working-dir` at a project builds it from anywhere.
#[test]
fn the_working_directory_option_locates_the_project() {
    let scratch = tempfile::TempDir::new().unwrap();
    let project = masm_project(scratch.path());
    let elsewhere = scratch.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();

    let package = midenc(&elsewhere, &["--working-dir", project.to_str().unwrap()])
        .unwrap_or_else(|err| panic!("midenc failed to build the project in --working-dir: {err}"))
        .expect("a build that ran to completion writes a package");

    assert!(
        package.starts_with(&project),
        "the package must land under the project `--working-dir` named: {}",
        package.display()
    );
}

/// An input file and a `--manifest-path` are accepted together only when they agree.
///
/// Given both, the compiler builds the one project they name; naming two different projects is
/// refused rather than silently building one of them.
#[test]
fn an_input_and_a_matching_manifest_path_build_and_a_mismatch_is_refused() {
    let scratch = tempfile::TempDir::new().unwrap();
    let project = masm_project(scratch.path());
    let manifest = project.join("miden-project.toml");
    let manifest = manifest.to_str().unwrap();

    midenc(&project, &[manifest, "--manifest-path", manifest])
        .unwrap_or_else(|err| panic!("midenc refused an input matching its --manifest-path: {err}"))
        .expect("a build that ran to completion writes a package");

    let other_parent = scratch.path().join("other");
    fs::create_dir_all(&other_parent).unwrap();
    let other = masm_project(&other_parent);
    let report = midenc(
        &project,
        &[manifest, "--manifest-path", other.join("miden-project.toml").to_str().unwrap()],
    )
    .expect_err("an input and a --manifest-path naming different projects must be refused");

    let rendered = format!("{report}");
    assert!(
        rendered.contains("name different files"),
        "the refusal must say the two name different files, but got: {rendered}"
    );
}
