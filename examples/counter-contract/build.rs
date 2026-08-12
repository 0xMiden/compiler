//! Populates the Miden package cache for builds that `midenc` does not drive.
//!
//! Plain `cargo check`, `cargo build`, and IDE analysis expand the Miden SDK macros without a
//! surrounding `cargo miden build`. Those macros read compiled dependency packages from the
//! directory named by `MIDENC_PACKAGE_CACHE`. The compiler's own package cache is deleted when
//! each build ends, so this script stages a cache of its own under `OUT_DIR`, fills it with a
//! nested `cargo miden build` that adopts the staged directory through the same variable, and
//! exports the variable to the compilation of this crate.
//!
//! The nested build runs whenever cargo re-runs this script — an always-rebuild strategy. The
//! script watches the project manifests, so a manifest edit re-runs it; an edit inside a
//! dependency's sources does not, and the staged packages stay as they are until a watched
//! input changes or `cargo miden build` runs. Computing a precise trigger set would mean
//! re-implementing build provenance, which belongs to the compiler.
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
    // The manifests are the one precise trigger: they declare the dependency set this
    // script stages packages for. Naming any watch disables cargo's watch-everything
    // default, which would otherwise re-run the nested build after every source edit.
    println!("cargo:rerun-if-changed=miden-project.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // Re-evaluate when the build mode or the tool selection changes.
    println!("cargo:rerun-if-env-changed=MIDENC_PACKAGE_CACHE");
    println!("cargo:rerun-if-env-changed=CARGO_MIDEN");
    // These inputs shape the compiled packages.
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");

    // Inside a midenc-driven build the compiler owns the package cache, macro expansion
    // already sees the variable, and a nested build would recurse into this script forever.
    if env::var_os("MIDENC_PACKAGE_CACHE").is_some() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    // The staged cache lives in this script's OUT_DIR, so it belongs to this crate and
    // build configuration and is removed by `cargo clean`. The directory is not cleared
    // between runs: a failed nested build then leaves the previously staged packages
    // usable, because analyzing against the last built packages beats failing the check.
    let cache_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("miden-packages");
    // The macros treat a missing directory as an empty cache; create it up front so the
    // exported variable always points at a real location.
    fs::create_dir_all(&cache_dir).expect("failed to create the Miden package cache directory");

    // Stage the dependency packages: the nested compiler adopts `cache_dir` through
    // MIDENC_PACKAGE_CACHE, publishes every dependency package into it before the root
    // target compiles, and leaves the directory in place for the outer build to read.
    let build = run_cargo_miden_build(&manifest_dir, &cache_dir);
    if !build.status.success() {
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
        println!(
            "cargo:warning=`cargo miden build --release` failed ({}); dependency packages may \
             be stale or missing: {}",
            build.status,
            last_stderr_line(&build.stderr),
        );
    }

    println!("cargo:rustc-env=MIDENC_PACKAGE_CACHE={}", cache_dir.display());
}

/// Runs `cargo miden build --release` for the project in `manifest_dir`, staging the
/// dependency packages into `cache_dir`.
///
/// `CARGO_MIDEN` selects a specific `cargo-miden` binary; otherwise the `cargo miden` plugin
/// is resolved through the `cargo` that drives this build. The nested build gets its own
/// cargo target directory: the outer cargo holds a lock on this build's target directory
/// while build scripts run, and a nested build against the same directory would deadlock.
fn run_cargo_miden_build(manifest_dir: &Path, cache_dir: &Path) -> Output {
    let mut command = match env::var_os("CARGO_MIDEN") {
        Some(cargo_miden) => Command::new(cargo_miden),
        None => Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
    };
    command
        .args(["miden", "build", "--release"])
        .current_dir(manifest_dir)
        .env("MIDENC_PACKAGE_CACHE", cache_dir)
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target").join("miden").join("build-script"),
        );
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
