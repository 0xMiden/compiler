//! The Miden toolchain the tests run against.
//!
//! Packages come from `MIDEN_SYSROOT/lib` (or the session's sysroot, which the compiler derives
//! from `MIDENUP_HOME` and `MIDENUP_TOOLCHAIN`), exactly as they do for a user's build under
//! `miden build`. Nothing here embeds or vendors a package.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_mast_package::Package;
use midenc_session::Session;

/// The workspace file that pins the toolchain, relative to this crate's manifest directory.
const TOOLCHAIN_FILE: &str = "../../miden-toolchain.toml";

/// Why no packages could be loaded.
#[derive(Debug, thiserror::Error)]
#[error(
    "no Miden toolchain packages at {path}: {reason}. Set MIDEN_SYSROOT to a toolchain directory \
     (its lib/ holds the .masp files), or install one with `midenup install {channel} --profile \
     empty --component core --component protocol` per miden-toolchain.toml"
)]
pub struct ToolchainError {
    path: PathBuf,
    reason: String,
    /// The channel `miden-toolchain.toml` pins, so the suggested command names the toolchain the
    /// repository actually wants. A placeholder when the file cannot be read — a missing
    /// toolchain file is not what this error is reporting.
    channel: String,
}

impl ToolchainError {
    fn new(path: impl Into<PathBuf>, reason: impl std::fmt::Display) -> Self {
        Self {
            path: path.into(),
            reason: reason.to_string(),
            channel: pinned_channel().unwrap_or_else(|_| String::from("<channel>")),
        }
    }
}

/// The sysroot named by the environment, or by the repository's toolchain file under midenup's
/// home.
///
/// `MIDEN_SYSROOT` wins outright — under `cargo make` it is always set, so nothing below runs
/// there. Otherwise the sysroot is `$MIDENUP_HOME/toolchains/<channel>`, with `MIDENUP_HOME`
/// defaulting to midenup's own default home and `<channel>` taken from `MIDENUP_TOOLCHAIN` or,
/// failing that, from the workspace's `miden-toolchain.toml` — the same derivation
/// `Makefile.toml` performs, so a direct `cargo nextest run` points at the toolchain the
/// repository pins rather than at a constant that has to be kept in step with it.
pub fn sysroot() -> PathBuf {
    if let Some(dir) = std::env::var_os("MIDEN_SYSROOT") {
        return PathBuf::from(dir);
    }
    let home = match std::env::var_os("MIDENUP_HOME") {
        Some(home) => PathBuf::from(home),
        None => home_dir().join(".local/share/midenup"),
    };
    home.join("toolchains").join(channel())
}

/// The toolchain channel: `MIDENUP_TOOLCHAIN`, else the one `miden-toolchain.toml` pins.
fn channel() -> String {
    if let Some(channel) = std::env::var_os("MIDENUP_TOOLCHAIN").filter(|c| !c.is_empty()) {
        return channel.to_string_lossy().into_owned();
    }
    pinned_channel().unwrap_or_else(|err| panic!("{err}"))
}

/// The `channel` value of the workspace's `miden-toolchain.toml`.
///
/// Matched by line rather than parsed: this crate has no `toml` dependency, and `Makefile.toml`
/// extracts the same value with a `sed` expression of the same shape.
fn pinned_channel() -> Result<String, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(TOOLCHAIN_FILE);
    let contents = std::fs::read_to_string(&manifest)
        .map_err(|err| format!("cannot read {}: {err}", manifest.display()))?;
    contents
        .lines()
        .filter_map(|line| line.trim().strip_prefix("channel"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .filter_map(|value| value.trim().strip_prefix('"'))
        .filter_map(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .next()
        .ok_or_else(|| format!("no `channel = \"...\"` entry in {}", manifest.display()))
}

/// Midenup's default home lives under the user's home directory, so an unset `HOME` has no
/// answer; say which variable to set instead of panicking on an unnamed `Option`.
fn home_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => panic!(
            "HOME is unset, so midenup's default home cannot be located: set MIDEN_SYSROOT to a \
             toolchain directory, or MIDENUP_HOME to midenup's home"
        ),
    }
}

/// Every package in `sysroot/lib`.
pub fn packages_in(sysroot: &Path) -> Result<Vec<Arc<Package>>, ToolchainError> {
    let lib = sysroot.join("lib");
    let entries = std::fs::read_dir(&lib).map_err(|err| ToolchainError::new(&lib, err))?;
    let mut packages = Vec::new();
    for entry in entries {
        let path = entry.map_err(|err| ToolchainError::new(&lib, err))?.path();
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("masp")) {
            let bytes = std::fs::read(&path).map_err(|err| ToolchainError::new(&path, err))?;
            let package = Package::read_from_bytes_trusted(&bytes)
                .map_err(|err| ToolchainError::new(&path, err))?;
            packages.push(Arc::new(package));
        }
    }
    if packages.is_empty() {
        return Err(ToolchainError::new(lib, "no .masp files"));
    }
    Ok(packages)
}

/// The packages of the session's toolchain, panicking with the install instructions if absent.
pub fn packages(session: &Session) -> Vec<Arc<Package>> {
    let root = session.options.sysroot.clone().unwrap_or_else(sysroot);
    packages_in(&root).unwrap_or_else(|err| panic!("{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sysroot_lib_directory_contains_the_core_and_protocol_packages() {
        let packages = packages_in(&sysroot()).unwrap_or_else(|err| panic!("{err}"));
        let mut names: Vec<&str> = packages.iter().map(|p| p.name.as_ref()).collect();
        names.sort();
        for expected in ["miden-core", "miden-protocol", "miden-standards", "miden-tx-kernel"] {
            assert!(names.contains(&expected), "missing {expected} in {names:?}");
        }
    }

    #[test]
    fn a_missing_sysroot_reports_how_to_install_one() {
        let err = packages_in(std::path::Path::new("/nonexistent/toolchain"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("MIDEN_SYSROOT"), "{err}");
        assert!(err.contains("midenup"), "{err}");
    }

    /// The channel is read from the workspace's toolchain file, so both the derived sysroot and
    /// the install instructions follow it rather than a constant kept in step by hand.
    #[test]
    fn the_channel_comes_from_the_workspace_toolchain_file() {
        let channel = pinned_channel().expect("the workspace pins a toolchain channel");
        assert!(!channel.is_empty(), "the pinned channel must not be empty");

        let err = packages_in(std::path::Path::new("/nonexistent/toolchain"))
            .unwrap_err()
            .to_string();
        assert!(err.contains(&format!("midenup install {channel}")), "{err}");
    }
}
