//! Populates the Miden package cache for builds that `midenc` does not drive.
//!
//! Plain `cargo check`, `cargo build`, and IDE analysis expand the Miden SDK macros without a
//! surrounding `cargo miden build`. Those macros read compiled dependency packages from the
//! directory named by `MIDENC_PACKAGE_CACHE`. The compiler's own package cache is deleted when
//! each build ends, so this script stages generations of its own under `OUT_DIR`, fills the
//! next generation with a nested `cargo miden build` that adopts it through the same variable,
//! and exports the variable to the compilation of this crate.
//!
//! Staging is generational so the exported directory is always one consistent build: the
//! nested build fills a fresh generation, and only a fully successful build moves the current
//! pointer. A failed build keeps exporting the previous generation whole — never a mix of new
//! and old packages — and is retried on the next check; if no good generation exists yet, the
//! script fails the outer build.
//!
//! The nested build runs whenever cargo re-runs this script. The script watches the project
//! manifests and, through the watch lists the compiler writes next to the staged packages, the
//! source inputs of every resolved dependency — so editing a dependency re-stages its package
//! on the next check.
//!
//! One sharing caveat: cargo keys build-script output by crate name and version, not by
//! project path. Two different projects with the same package name and version that share one
//! `CARGO_TARGET_DIR` reuse each other's script output, including this staged cache. Use
//! per-checkout target directories for such layouts.
//!
//! See <https://github.com/0xMiden/compiler/issues/1298>.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn main() {
    // The manifests declare the dependency set this script stages packages for. Naming any
    // watch disables cargo's watch-everything default, which would otherwise re-run the
    // nested build after every source edit of this crate.
    println!("cargo:rerun-if-changed=miden-project.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // Re-evaluate when the build mode or the tool selection changes.
    println!("cargo:rerun-if-env-changed=MIDENC_PACKAGE_CACHE");
    println!("cargo:rerun-if-env-changed=CARGO_MIDEN");
    // These inputs shape the compiled packages. Cargo prefers the encoded rustflags variable
    // over the plain one, so both spellings are watched.
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");

    // Inside a midenc-driven build the compiler owns the package cache, macro expansion
    // already sees the variable, and a nested build would recurse into this script forever.
    // An empty value counts as unset, matching the compiler and the SDK macros.
    if env::var_os("MIDENC_PACKAGE_CACHE").is_some_and(|value| !value.is_empty()) {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Generations live under this script's OUT_DIR, so they belong to this crate and build
    // configuration and are removed by `cargo clean`.
    let generations = out_dir.join("miden-packages");
    let pointer = out_dir.join("miden-packages.current");

    // Alternate between two generations: the pointer names the last good one, the nested
    // build fills the other from scratch (which also drops packages of removed
    // dependencies). Only a missing pointer means "no current generation": treating a
    // transient read error that way would select the last-good generation for clearing.
    let current = match fs::read_to_string(&pointer) {
        Ok(name) => Some(name.trim().to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            panic!("failed to read the package cache pointer '{}': {err}", pointer.display())
        }
    }
    .filter(|name| name == "gen-0" || name == "gen-1")
    .filter(|name| generations.join(name).is_dir());
    let next = match current.as_deref() {
        Some("gen-0") => "gen-1",
        _ => "gen-0",
    };
    let next_dir = generations.join(next);
    match fs::remove_dir_all(&next_dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            panic!("failed to clear the staging generation '{}': {err}", next_dir.display())
        }
    }
    fs::create_dir_all(&generations).expect("failed to create the Miden package cache directory");
    // `create_dir`, not `create_dir_all`: it fails if anything survived the removal, so a
    // partially cleared generation can never masquerade as a fresh one.
    fs::create_dir(&next_dir).expect("failed to create the Miden package cache generation");

    // Stage the dependency packages: the nested compiler adopts the generation through
    // MIDENC_PACKAGE_CACHE, publishes every dependency package into it before the root
    // target compiles, and leaves the directory in place for the outer build to read.
    let build = run_cargo_miden_build(&manifest_dir, &next_dir, &out_dir.join("nested-target"));
    let exported = if build.status.success() {
        // Only a complete generation becomes current. The pointer write is the publication
        // step: a crash before it leaves the previous generation exported.
        let staged = pointer.with_extension("current.tmp");
        fs::write(&staged, next).expect("failed to stage the Miden package cache pointer");
        fs::rename(&staged, &pointer).expect("failed to publish the Miden package cache pointer");
        next_dir
    } else {
        let stderr = String::from_utf8_lossy(&build.stderr);
        // A missing `cargo miden` plugin is a setup error, not a broken build; surface it
        // with instructions instead of a stale-packages warning.
        if stderr.contains("no such command") || stderr.contains("no such subcommand") {
            panic!(
                "`cargo miden build` failed ({}): the `cargo miden` plugin was not \
                 found.\nInstall cargo-miden (`cargo install cargo-miden`) or point the \
                 CARGO_MIDEN environment variable at a cargo-miden binary.",
                build.status,
            );
        }
        match current {
            // "Stale beats broken": the previous generation is a consistent package set, and
            // analyzing against it beats failing the whole check while a dependency is
            // mid-edit.
            Some(current) => {
                println!(
                    "cargo:warning=`cargo miden build --release --stop-after=dependencies` failed \
                     ({}); analyzing against \
                     the previously staged dependency packages: {}",
                    build.status,
                    last_stderr_line(&build.stderr),
                );
                // Watch a path that never exists: cargo re-runs the script unconditionally
                // while a watched path is missing, so the failed nested build is retried on
                // the next check instead of the fallback being cached as a success.
                println!(
                    "cargo:rerun-if-changed={}",
                    out_dir.join("miden-packages.retry").display()
                );
                generations.join(current)
            }
            // With no good generation there is nothing consistent to export; failing here is
            // clearer than every macro expansion failing on missing packages.
            None => panic!(
                "`cargo miden build --release --stop-after=dependencies` failed ({}) and no \
                 previously staged dependency \
                 packages exist:\n{}",
                build.status,
                String::from_utf8_lossy(&build.stderr),
            ),
        }
    };

    // Watch the resolved dependency inputs recorded by the compiler, so editing a dependency
    // re-runs this script and re-stages its package. One absolute path per line, one file per
    // resolved consumer project. Read errors fail loud: swallowing them would cache the
    // staged generation with no dependency watches at all. Only a missing directory is fine —
    // a project without path dependencies records none.
    let watch_dir = exported.join("miden-deps");
    let entries = match fs::read_dir(&watch_dir) {
        Ok(entries) => Some(entries),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => panic!("failed to list the watch lists in '{}': {err}", watch_dir.display()),
    };
    for entry in entries.into_iter().flatten() {
        let entry = entry.expect("failed to read a watch-list directory entry");
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "watch") {
            let watch_list = fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("failed to read the watch list '{}': {err}", path.display())
            });
            for line in watch_list.lines().map(str::trim).filter(|line| !line.is_empty()) {
                // Watch only paths that exist: cargo re-runs a build script
                // unconditionally while a watched path is missing.
                if Path::new(line).exists() {
                    println!("cargo:rerun-if-changed={line}");
                }
            }
        }
    }

    println!("cargo:rustc-env=MIDENC_PACKAGE_CACHE={}", exported.display());
}

/// Runs `cargo miden build --release --stop-after=dependencies` for the project in
/// `manifest_dir`, staging the
/// dependency packages into `cache_dir`.
///
/// `CARGO_MIDEN` selects a specific `cargo-miden` binary; otherwise the `cargo miden` plugin
/// is resolved through the `cargo` that drives this build. The nested build's cargo and
/// midenc target directories live under `nested_target` (inside this script's `OUT_DIR`),
/// so every write lands beneath the outer build's configured target directory and is
/// removed by `cargo clean`. The nested cargo target must stay disjoint from the outer
/// target directory itself: the outer cargo holds a lock on it while build scripts run,
/// and a nested build against the same directory would deadlock.
fn run_cargo_miden_build(manifest_dir: &Path, cache_dir: &Path, nested_target: &Path) -> Output {
    let mut command = match env::var_os("CARGO_MIDEN") {
        Some(cargo_miden) => Command::new(cargo_miden),
        None => Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
    };
    command
        // `--stop-after=dependencies` stages the dependency packages and the compiler's
        // resolution records, then stops before compiling this crate itself — the macros
        // only ever read the dependencies, and the outer cargo build compiles the crate.
        .args(["miden", "build", "--release", "--stop-after=dependencies"])
        .current_dir(manifest_dir)
        .env("MIDENC_PACKAGE_CACHE", cache_dir)
        .env("CARGO_TARGET_DIR", nested_target.join("cargo"))
        .env("MIDENC_TARGET_DIR", nested_target.join("miden"));
    command.output().unwrap_or_else(|err| {
        panic!(
            "failed to run `cargo miden build`: {err}.\nInstall cargo-miden (`cargo install \
             cargo-miden`) or point the CARGO_MIDEN environment variable at a cargo-miden \
             binary."
        )
    })
}

/// Returns the last non-empty stderr line for a compact warning.
fn last_stderr_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no error output")
        .to_string()
}
