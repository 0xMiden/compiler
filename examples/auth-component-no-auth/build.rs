//! Populates the Miden package cache for builds that `midenc` does not drive.
//!
//! Plain `cargo check`, `cargo build`, and IDE analysis expand the Miden SDK macros without a
//! surrounding `cargo miden build`. Those macros read compiled dependency packages from the
//! directory named by `MIDENC_PACKAGE_CACHE`. This script asks `cargo-miden` for that
//! directory, populates it with a nested build when the project has source dependencies, and
//! exports the variable to the compilation of this crate.
//!
//! See <https://github.com/0xMiden/compiler/issues/1298>.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn main() {
    // Re-evaluate this script when the build mode or the tool selection changes.
    println!("cargo:rerun-if-env-changed=MIDENC_PACKAGE_CACHE");
    println!("cargo:rerun-if-env-changed=CARGO_MIDEN");
    // These inputs shape the compiler's package-cache fingerprint.
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");

    // Inside a midenc-driven build the compiler owns the package cache, macro expansion
    // already sees the variable, and a nested build would recurse into this script forever.
    if env::var_os("MIDENC_PACKAGE_CACHE").is_some() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());

    // Ask the compiler where this project's package cache lives and which inputs shape it.
    let query = run_cargo_miden(&manifest_dir, "package-cache");
    if !query.status.success() {
        panic!(
            "`cargo miden package-cache` failed ({}).\nInstall cargo-miden (`cargo install \
             cargo-miden`) or point the CARGO_MIDEN environment variable at a cargo-miden \
             binary.\n--- stderr ---\n{}",
            query.status,
            String::from_utf8_lossy(&query.stderr),
        );
    }

    let stdout =
        String::from_utf8(query.stdout).expect("cargo miden package-cache output is UTF-8");
    let mut cache_dir = None;
    let mut source_dependencies = 0usize;
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("cache-dir=") {
            cache_dir = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("source-dependencies=") {
            source_dependencies = value.parse().expect("source-dependencies is a number");
        } else if let Some(value) = line.strip_prefix("watch=") {
            println!("cargo:rerun-if-changed={value}");
        }
    }
    let cache_dir = cache_dir.expect("cargo miden package-cache printed no cache-dir");

    if source_dependencies > 0 {
        // Populate the cache. Dependency packages publish before the root target compiles, so
        // even a failing build (for example, this crate is mid-edit) usually leaves the
        // dependency packages usable; the macros report anything that is genuinely missing.
        // The cache is still exported below on failure: analyzing against the last built
        // packages beats failing the whole check, at the accepted cost that a broken
        // dependency keeps its previous interface until it builds again.
        let build = run_cargo_miden(&manifest_dir, "build");
        if !build.status.success() {
            println!(
                "cargo:warning=`cargo miden build --release` failed ({}); dependency packages \
                 may be stale or missing: {}",
                build.status,
                last_stderr_line(&build.stderr),
            );
        }
    }

    // The macros treat a missing directory as an empty cache; create it so the exported
    // variable always points at a real location. Watching the directory re-runs this script
    // when another build rewrites or prunes the cache (cargo re-runs unconditionally while a
    // watched path is missing), which keeps the exported path and its packages live.
    fs::create_dir_all(&cache_dir).expect("failed to create the Miden package cache directory");
    println!("cargo:rerun-if-changed={}", cache_dir.display());
    println!("cargo:rustc-env=MIDENC_PACKAGE_CACHE={}", cache_dir.display());
}

/// Runs `cargo miden <subcommand> --release` for the project in `manifest_dir`.
///
/// `CARGO_MIDEN` selects a specific `cargo-miden` binary; otherwise the `cargo miden` plugin
/// is resolved through the `cargo` that drives this build. The nested build gets its own
/// cargo target directory: the outer cargo holds a lock on this build's target directory
/// while build scripts run, and a nested build against the same directory would deadlock.
/// `--release` keeps the cache fingerprint identical to a `cargo miden build --release` run
/// by hand, so both share one cache.
fn run_cargo_miden(manifest_dir: &Path, subcommand: &str) -> Output {
    let mut command = match env::var_os("CARGO_MIDEN") {
        Some(cargo_miden) => Command::new(cargo_miden),
        None => Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
    };
    command
        .args(["miden", subcommand, "--release"])
        .current_dir(manifest_dir)
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target").join("miden").join("build-script"),
        );
    command.output().unwrap_or_else(|err| {
        panic!(
            "failed to run `cargo miden {subcommand}`: {err}.\nInstall cargo-miden (`cargo \
             install cargo-miden`) or point the CARGO_MIDEN environment variable at a \
             cargo-miden binary."
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
