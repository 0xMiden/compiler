//! Module for cargo-component configuration.
//!
//! This implements an argument parser because `clap` is not
//! designed for parsing unknown or unsupported arguments.
//!
//! See https://github.com/clap-rs/clap/issues/1404 for some
//! discussion around this issue.
//!
//! To properly "wrap" `cargo` commands, we need to be able to
//! detect certain arguments, but not error out if the arguments
//! are otherwise unknown as they will be passed to `cargo` directly.
//!
//! This will allow `cargo-component` to be used as a drop-in
//! replacement for `cargo` without having to be fully aware of
//! the many subcommands and options that `cargo` supports.
//!
//! What is detected here is the minimal subset of the arguments
//! that `cargo` supports which are necessary for `cargo-component`
//! to function.

use std::{fmt, str::FromStr};

use anyhow::{bail, Context, Result};
use cargo_metadata::Metadata;
use semver::Version;
use toml_edit::DocumentMut;

/// Represents a cargo package specifier.
///
/// See `cargo help pkgid` for more information.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CargoPackageSpec {
    /// The name of the package, e.g. `foo`.
    pub name: String,
    /// The version of the package, if specified.
    pub version: Option<Version>,
}

impl CargoPackageSpec {
    /// Creates a new package specifier from a string.
    pub fn new(spec: impl Into<String>) -> Result<Self> {
        let spec = spec.into();

        // Bail out if the package specifier contains a URL.
        if spec.contains("://") {
            bail!("URL package specifier `{spec}` is not supported");
        }

        Ok(match spec.split_once('@') {
            Some((name, version)) => Self {
                name: name.to_string(),
                version: Some(
                    version
                        .parse()
                        .with_context(|| format!("invalid package specified `{spec}`"))?,
                ),
            },
            None => Self {
                name: spec,
                version: None,
            },
        })
    }

    /// Loads Cargo.toml in the current directory, attempts to find the matching package from metadata.
    #[allow(unused)]
    pub fn find_current_package_spec(metadata: &Metadata) -> Option<Self> {
        let current_manifest = std::fs::read_to_string("Cargo.toml").ok()?;
        let document: DocumentMut = current_manifest.parse().ok()?;
        let name = document["package"]["name"].as_str()?;
        let version = metadata
            .packages
            .iter()
            .find(|found| found.name == name)
            .map(|found| found.version.clone());
        Some(CargoPackageSpec {
            name: name.to_string(),
            version,
        })
    }
}

impl FromStr for CargoPackageSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl fmt::Display for CargoPackageSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{name}", name = self.name)?;
        if let Some(version) = &self.version {
            write!(f, "@{version}")?;
        }

        Ok(())
    }
}
