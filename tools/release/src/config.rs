//! The authoritative release policy, loaded from `.release/config.toml`.
//!
//! Every workspace package must be classified here exactly once. Classification
//! is explicit rather than inferred from directory layout, so that adding a
//! package to the workspace forces a deliberate decision about whether it is
//! published.

use std::{collections::BTreeMap, fmt, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The version domain a package draws its version from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionSource {
    /// Inherits the root `[workspace.package]` version.
    Workspace,
    /// Shares the SDK version, anchored by the `miden` crate.
    Sdk,
}

/// The release unit a package belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    Compiler,
    Sdk,
    /// Repository infrastructure that is never published.
    Private,
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Compiler => "compiler",
            Self::Sdk => "sdk",
            Self::Private => "private",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageConfig {
    pub name: String,
    pub unit: Unit,
    pub publish: bool,
    /// Absent for private packages, which have no version domain of their own.
    #[serde(default)]
    pub version_source: Option<VersionSource>,
}

/// Per-unit policy. These fields are validated on load and consumed by the
/// version, changelog, and tagging commands.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct UnitConfig {
    pub tag: String,
    pub changelog: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub schema_version: u32,
    /// Per-unit policy, keyed by unit name. Consumed by the version, changelog,
    /// and tagging commands.
    #[allow(dead_code)]
    pub units: BTreeMap<String, UnitConfig>,
    #[serde(rename = "packages")]
    pub packages: Vec<PackageConfig>,
}

/// The schema version this build understands. A mismatch is a hard error rather
/// than a best-effort parse, so that a newer config cannot be silently
/// misinterpreted by an older tool.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read release config at {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("failed to parse release config at {}", path.display()))?;

        if config.schema_version != SUPPORTED_SCHEMA_VERSION {
            bail!(
                "release config schema version {} is not supported by this release-tool (expected \
                 {})",
                config.schema_version,
                SUPPORTED_SCHEMA_VERSION
            );
        }

        let mut seen = BTreeMap::new();
        for package in &config.packages {
            if seen.insert(package.name.as_str(), ()).is_some() {
                bail!("package '{}' is classified more than once", package.name);
            }
            match (package.unit, package.publish) {
                (Unit::Private, true) => {
                    bail!("package '{}' is private but marked publishable", package.name)
                }
                (Unit::Private, false) => {}
                (_, false) => bail!(
                    "package '{}' belongs to unit '{}' but is not publishable; unpublished \
                     packages belong to the 'private' unit",
                    package.name,
                    package.unit
                ),
                (_, true) if package.version_source.is_none() => bail!(
                    "package '{}' is publishable but declares no version-source",
                    package.name
                ),
                (_, true) => {}
            }
        }

        Ok(config)
    }

    /// The packages belonging to a unit, in configuration order.
    pub fn packages_in(&self, unit: Unit) -> impl Iterator<Item = &PackageConfig> {
        self.packages.iter().filter(move |p| p.unit == unit)
    }
}
