//! The authoritative release policy, loaded from `.release/config.toml`.
//!
//! Release units are declared here rather than compiled in, so the tool serves
//! any Cargo package or workspace. Every workspace package must be classified
//! exactly once. Classification is explicit rather than inferred from directory
//! layout, so that adding a package forces a deliberate decision about whether
//! it is published.
//!
//! Validation is deliberately front-loaded. A unit naming a peer that does not
//! exist, or a cycle in publish order, is a mistake that would otherwise
//! surface phases later, possibly after something irreversible has happened.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The schema version this build understands. A mismatch is a hard error rather
/// than a best-effort parse, so a newer config cannot be silently
/// misinterpreted by an older tool.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 2;

/// Changelog headings for a unit that declares none.
pub const DEFAULT_CHANGELOG_HEADINGS: [&str; 3] = ["Added", "Changed", "Fixed"];

/// What a release unit is.
///
/// The kinds differ along two independent axes -- whether they publish crates to
/// a registry, and whether they have a tag, changelog, and GitHub release.
/// Conflating those is what leaves a shared library crate with nowhere to go.
///
/// | kind      | publishes crates | is released |
/// |-----------|------------------|-------------|
/// | `crates`  | yes              | yes         |
/// | `library` | yes              | no          |
/// | `artifact`| no               | yes         |
/// | `private` | no               | no          |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnitKind {
    /// Crates published to a registry under this unit's tag and version.
    Crates,
    /// Crates published to a registry, with no tag, changelog, or release of
    /// their own. A shared crate several units depend on.
    Library,
    /// A single file attached to the GitHub release.
    Artifact,
    /// Repository infrastructure that is never published.
    Private,
}

impl UnitKind {
    /// Whether packages in this unit go to a registry.
    pub fn publishes_crates(self) -> bool {
        matches!(self, Self::Crates | Self::Library)
    }

    /// Whether this unit has a tag, a changelog, and a GitHub release.
    pub fn is_releasable(self) -> bool {
        matches!(self, Self::Crates | Self::Artifact)
    }

    /// The name this kind is spelled with in the configuration file. Error
    /// messages quote it back exactly as the author wrote it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crates => "crates",
            Self::Library => "library",
            Self::Artifact => "artifact",
            Self::Private => "private",
        }
    }
}

impl std::fmt::Display for UnitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a crates or library unit's version lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionSource {
    /// The root `[workspace.package]` version, inherited by members -- or, in a
    /// single-crate repository with no workspace table, the root
    /// `[package].version` itself.
    Workspace,
    /// The consensus of the domain's own member manifest versions.
    Own,
}

/// Where an artifact unit's version is recorded.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VersionFile {
    /// Relative to the workspace root.
    pub path: PathBuf,
    #[serde(default = "default_version_key")]
    pub key: String,
}

fn default_version_key() -> String {
    "version".to_string()
}

/// How an artifact unit's asset is produced.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ArtifactSource {
    /// Archive git-tracked files beneath this directory, relative to the
    /// workspace root. Not a whole-subtree walk: see `include` and `manifest`.
    #[serde(default)]
    pub directory: Option<PathBuf>,
    /// Attach an existing file, produced by whatever built it.
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// Paths relative to `directory`, each enumerated with `git ls-files`.
    /// Files or directories. The inline form of the include list.
    #[serde(default)]
    pub include: Vec<PathBuf>,
    /// A TOML manifest inside `directory` whose entries supply the include
    /// list, and which also holds the unit's version. The named form.
    #[serde(default)]
    pub manifest: Option<PathBuf>,
    /// The default `--output` name for `release-tool bundle`. Not a routing
    /// mechanism: the file reaches its release through `--artifacts` and the
    /// unit's `assets` globs, because CI builds it in one job and stages it in
    /// another.
    #[serde(default)]
    pub asset: Option<String>,
    /// A committed copy of the archive that `lint` verifies against freshly
    /// built bytes. Drift here is otherwise invisible.
    #[serde(default)]
    pub embedded_copy: Option<PathBuf>,
    /// Where the version lives, when it is not `manifest`'s `version` key.
    #[serde(default)]
    pub version_file: Option<VersionFile>,
}

/// A version requirement this unit's sources embed on another unit's packages.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Tracks {
    /// Packages from the tracked unit whose requirements appear in these
    /// sources. A subset, not necessarily the whole unit.
    pub packages: Vec<String>,
    /// The key in the artifact manifest holding the declared requirement.
    /// Defaults to `"{tracked-unit}-requirement"`.
    #[serde(default)]
    pub requirement_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct UnitConfig {
    pub kind: UnitKind,
    /// Tag template; `{version}` is substituted.
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub changelog_headings: Option<Vec<String>>,
    #[serde(default)]
    pub version_source: Option<VersionSource>,
    /// Units that must publish before this one.
    #[serde(default)]
    pub after: Vec<String>,
    /// Units whose release forces this one into the same candidate, for
    /// reasons unrelated to version requirements. Requirement-driven
    /// co-release is decided from content in `candidate::validate`.
    #[serde(default)]
    pub release_when: Vec<String>,
    /// Version requirements this unit's sources embed, keyed by tracked unit.
    #[serde(default)]
    pub tracks: BTreeMap<String, Tracks>,
    /// Whether this unit may claim the repository's "latest release" slot.
    #[serde(default)]
    pub latest: bool,
    /// Globs routing `stage --artifacts` files to this unit.
    #[serde(default)]
    pub assets: Vec<String>,
    /// Globs that must match at least one staged file, when this unit is in
    /// the plan.
    #[serde(default)]
    pub required_assets: Vec<String>,
    #[serde(default)]
    pub source: Option<ArtifactSource>,
}

impl UnitConfig {
    /// The tag template. Validation guarantees releasable units have one.
    pub fn tag(&self) -> &str {
        self.tag.as_deref().expect("a releasable unit has a tag; validated on load")
    }

    /// The changelog path. Validation guarantees releasable units have one.
    pub fn changelog(&self) -> &str {
        self.changelog
            .as_deref()
            .expect("a releasable unit has a changelog; validated on load")
    }

    pub fn headings(&self) -> Vec<&str> {
        match &self.changelog_headings {
            Some(headings) => headings.iter().map(String::as_str).collect(),
            None => DEFAULT_CHANGELOG_HEADINGS.to_vec(),
        }
    }

    pub fn publishes_crates(&self) -> bool {
        self.kind.publishes_crates()
    }

    pub fn is_releasable(&self) -> bool {
        self.kind.is_releasable()
    }

    /// The manifest key holding the requirement this unit declares on `tracked`.
    pub fn requirement_key(&self, tracked: &str) -> String {
        self.tracks
            .get(tracked)
            .and_then(|t| t.requirement_key.clone())
            .unwrap_or_else(|| format!("{tracked}-requirement"))
    }

    /// Units that must publish first: `after` plus every tracked unit.
    ///
    /// Tracking implies ordering because a unit embedding a requirement on
    /// another cannot resolve until that other unit is on the registry. It
    /// deliberately does not imply co-release: see the spec's "Co-release is
    /// content-driven" section.
    pub fn after_all(&self) -> BTreeSet<&str> {
        self.after
            .iter()
            .map(String::as_str)
            .chain(self.tracks.keys().map(String::as_str))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PackageConfig {
    pub name: String,
    pub unit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    /// The frozen version every private package carries. Absent means the
    /// check does not run.
    #[serde(default)]
    pub private_version: Option<String>,
    pub units: BTreeMap<String, UnitConfig>,
    #[serde(default)]
    pub packages: Vec<PackageConfig>,
}

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

        config.validate().with_context(|| format!("in {}", path.display()))?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        self.validate_units()?;
        self.validate_packages()?;
        self.validate_relations()?;
        self.order()?;
        Ok(())
    }

    fn validate_units(&self) -> Result<()> {
        let mut latest: Vec<&str> = Vec::new();
        let mut workspace_domain: Vec<&str> = Vec::new();

        for (name, unit) in &self.units {
            // Fields no unit of this kind may declare. Listed exhaustively so a
            // field added later has to be classified deliberately.
            let forbidden: &[(&str, bool)] = match unit.kind {
                UnitKind::Private => &[
                    ("tag", unit.tag.is_some()),
                    ("changelog", unit.changelog.is_some()),
                    ("changelog-headings", unit.changelog_headings.is_some()),
                    ("version-source", unit.version_source.is_some()),
                    ("after", !unit.after.is_empty()),
                    ("release-when", !unit.release_when.is_empty()),
                    ("tracks", !unit.tracks.is_empty()),
                    ("latest", unit.latest),
                    ("assets", !unit.assets.is_empty()),
                    ("required-assets", !unit.required_assets.is_empty()),
                    ("source", unit.source.is_some()),
                ],
                UnitKind::Library => &[
                    ("tag", unit.tag.is_some()),
                    ("changelog", unit.changelog.is_some()),
                    ("changelog-headings", unit.changelog_headings.is_some()),
                    ("after", !unit.after.is_empty()),
                    ("release-when", !unit.release_when.is_empty()),
                    ("tracks", !unit.tracks.is_empty()),
                    ("latest", unit.latest),
                    ("assets", !unit.assets.is_empty()),
                    ("required-assets", !unit.required_assets.is_empty()),
                    ("source", unit.source.is_some()),
                ],
                UnitKind::Crates => {
                    &[("source", unit.source.is_some()), ("tracks", !unit.tracks.is_empty())]
                }
                UnitKind::Artifact => &[("version-source", unit.version_source.is_some())],
            };

            for (field, present) in forbidden {
                if *present {
                    bail!("unit '{name}' is of kind '{}' and may not declare '{field}'", unit.kind);
                }
            }

            match unit.kind {
                UnitKind::Private => {}
                UnitKind::Library => {
                    if unit.version_source.is_none() {
                        bail!("unit '{name}' is a library but declares no version-source");
                    }
                }
                UnitKind::Crates => {
                    if unit.tag.is_none() {
                        bail!("unit '{name}' declares no tag template");
                    }
                    if unit.changelog.is_none() {
                        bail!("unit '{name}' declares no changelog");
                    }
                    if unit.version_source.is_none() {
                        bail!(
                            "unit '{name}' publishes crates but declares no version-source; use \
                             \"workspace\" or \"own\""
                        );
                    }
                }
                UnitKind::Artifact => {
                    if unit.tag.is_none() {
                        bail!("unit '{name}' declares no tag template");
                    }
                    if unit.changelog.is_none() {
                        bail!("unit '{name}' declares no changelog");
                    }
                    let Some(source) = &unit.source else {
                        bail!(
                            "unit '{name}' releases an artifact but declares no source; give it a \
                             'directory' to archive or a 'file' to attach"
                        );
                    };
                    match (&source.directory, &source.file) {
                        (None, None) => bail!(
                            "unit '{name}' declares a source with neither 'directory' nor 'file'"
                        ),
                        (Some(_), Some(_)) => bail!(
                            "unit '{name}' declares both 'directory' and 'file'; an artifact is \
                             either archived from sources or attached as-is"
                        ),
                        (Some(_), None) => {
                            // Not a whole-subtree walk: repository furniture
                            // beside the sources would ship in the archive.
                            match (source.include.is_empty(), source.manifest.is_some()) {
                                (true, false) => bail!(
                                    "unit '{name}' archives a directory but declares no include \
                                     list; give it 'include' or a 'manifest'"
                                ),
                                (false, true) => bail!(
                                    "unit '{name}' declares both 'include' and 'manifest'; the \
                                     include list comes from exactly one of them"
                                ),
                                _ => {}
                            }
                        }
                        (None, Some(_)) => {
                            if !source.include.is_empty() {
                                bail!("unit '{name}' declares 'include' on a file source");
                            }
                            if source.embedded_copy.is_some() {
                                bail!(
                                    "unit '{name}' declares 'embedded-copy' on a file source; \
                                     there is nothing to rebuild it from, so nothing to compare"
                                );
                            }
                        }
                    }
                }
            }

            if unit.latest {
                latest.push(name);
            }
            if unit.is_releasable() && unit.version_source == Some(VersionSource::Workspace) {
                workspace_domain.push(name);
            }
        }

        if latest.len() > 1 {
            bail!(
                "units {} all claim 'latest'; only one release can hold the repository's latest \
                 slot",
                latest.join(", ")
            );
        }

        if workspace_domain.len() > 1 {
            bail!(
                "releasable units {} all use version-source \"workspace\", so bumping one \
                 silently moves the others' crates to an unpublished version. Two releasable \
                 units sharing a version domain are one unit; give each its own with \"own\", or \
                 merge them. A 'library' unit may share the workspace domain, because it is never \
                 released on its own",
                workspace_domain.join(", ")
            );
        }
        Ok(())
    }

    fn validate_packages(&self) -> Result<()> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();

        for package in &self.packages {
            if !seen.insert(package.name.as_str()) {
                bail!("package '{}' is classified more than once", package.name);
            }
            if !self.units.contains_key(&package.unit) {
                bail!(
                    "package '{}' names unit '{}', which is not declared",
                    package.name,
                    package.unit
                );
            }
            *counts.entry(package.unit.as_str()).or_default() += 1;
        }

        for (name, unit) in &self.units {
            let count = counts.get(name.as_str()).copied().unwrap_or(0);
            match unit.kind {
                UnitKind::Crates | UnitKind::Library if count == 0 => {
                    bail!("unit '{name}' publishes crates but has no packages assigned to it")
                }
                UnitKind::Artifact if count > 0 => bail!(
                    "unit '{name}' releases an artifact but has {count} package(s) assigned to \
                     it; artifact units publish no crates"
                ),
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_relations(&self) -> Result<()> {
        let packages_by_unit: BTreeMap<&str, BTreeSet<&str>> = self
            .units
            .keys()
            .map(|unit| {
                let names = self
                    .packages
                    .iter()
                    .filter(|p| &p.unit == unit)
                    .map(|p| p.name.as_str())
                    .collect();
                (unit.as_str(), names)
            })
            .collect();

        for (name, unit) in &self.units {
            let relations: [(&str, Vec<&String>); 3] = [
                ("after", unit.after.iter().collect()),
                ("release-when", unit.release_when.iter().collect()),
                ("tracks", unit.tracks.keys().collect()),
            ];

            for (relation, targets) in relations {
                for target in targets {
                    if target == name {
                        bail!("unit '{name}' names itself in '{relation}'");
                    }
                    let Some(other) = self.units.get(target.as_str()) else {
                        bail!(
                            "unit '{name}' names '{target}' in '{relation}', which is not declared"
                        );
                    };
                    if !other.is_releasable() {
                        bail!(
                            "unit '{name}' names '{target}' in '{relation}', but '{target}' is \
                             never released on its own"
                        );
                    }
                }
            }

            for (tracked, tracks) in &unit.tracks {
                let target = &self.units[tracked.as_str()];
                if target.kind != UnitKind::Crates {
                    bail!(
                        "unit '{name}' tracks '{tracked}', which does not publish crates under \
                         its own version; there are no requirements to embed"
                    );
                }
                if unit.source.as_ref().and_then(|s| s.manifest.as_ref()).is_none() {
                    bail!(
                        "unit '{name}' tracks '{tracked}' but declares no 'source.manifest'; the \
                         declared requirement has nowhere to live"
                    );
                }
                if tracks.packages.is_empty() {
                    bail!("unit '{name}' tracks '{tracked}' but names no packages");
                }
                for package in &tracks.packages {
                    if !packages_by_unit[tracked.as_str()].contains(package.as_str()) {
                        bail!(
                            "unit '{name}' tracks package '{package}', which does not belong to \
                             unit '{tracked}'"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub fn unit(&self, name: &str) -> Result<&UnitConfig> {
        self.units
            .get(name)
            .with_context(|| format!("'{name}' is not a release unit; see .release/config.toml"))
    }

    /// A unit that is released on its own, with a tag, a changelog, and a
    /// GitHub release.
    ///
    /// [`Config::unit`] proves only that a name is *declared*. The commands
    /// that render a tag or read a changelog need more than that: a `library`
    /// or `private` unit is forbidden to declare either, so reaching
    /// [`UnitConfig::tag`] with one panics on an internal assertion. This is
    /// the boundary check that turns that into a message.
    pub fn releasable_unit(&self, name: &str) -> Result<&UnitConfig> {
        let unit = self.unit(name)?;
        if !unit.is_releasable() {
            bail!(
                "unit '{name}' is a '{}' unit and is never released on its own, so it has no tag \
                 or changelog",
                unit.kind
            );
        }
        Ok(unit)
    }

    /// The packages belonging to a unit, in configuration order.
    pub fn packages_in<'a>(&'a self, unit: &'a str) -> impl Iterator<Item = &'a PackageConfig> {
        self.packages.iter().filter(move |p| p.unit == unit)
    }

    /// Whether a package goes to a registry, derived from its unit's kind.
    pub fn is_publishable(&self, package: &PackageConfig) -> bool {
        self.units.get(&package.unit).is_some_and(UnitConfig::publishes_crates)
    }

    pub fn publishable(&self) -> impl Iterator<Item = &PackageConfig> {
        self.packages.iter().filter(move |p| self.is_publishable(p))
    }

    pub fn units_of_kind(&self, kind: UnitKind) -> impl Iterator<Item = (&String, &UnitConfig)> {
        self.units.iter().filter(move |(_, unit)| unit.kind == kind)
    }

    /// Every unit that gets a tag, a changelog, and a release.
    pub fn releasable(&self) -> impl Iterator<Item = (&String, &UnitConfig)> {
        self.units.iter().filter(|(_, unit)| unit.is_releasable())
    }

    /// The unit a package belongs to, if any.
    pub fn unit_of(&self, package: &str) -> Option<&str> {
        self.packages.iter().find(|p| p.name == package).map(|p| p.unit.as_str())
    }

    /// Releasable units in publish order.
    ///
    /// A topological sort over `after` (including the edges `tracks` implies),
    /// with a name tiebreak so the result is a function of the configuration and
    /// nothing else. Determinism matters: this ordering reaches the intent, and
    /// a reviewed intent and an executed one must be byte-identical.
    pub fn order(&self) -> Result<Vec<String>> {
        let nodes: BTreeSet<&str> = self.releasable().map(|(name, _)| name.as_str()).collect();

        let mut pending: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for name in &nodes {
            let unit = &self.units[*name];
            let deps: BTreeSet<&str> =
                unit.after_all().into_iter().filter(|d| nodes.contains(d)).collect();
            pending.insert(name, deps);
        }

        let mut ordered = Vec::with_capacity(nodes.len());
        while !pending.is_empty() {
            // BTreeMap iteration is sorted, so the first ready node is the
            // alphabetically smallest: the tiebreak is the ordering itself.
            let Some(next) =
                pending.iter().find(|(_, deps)| deps.is_empty()).map(|(name, _)| *name)
            else {
                let stuck: Vec<&str> = pending.keys().copied().collect();
                bail!(
                    "release units form a cycle in 'after': {}. Publish order must be acyclic",
                    stuck.join(", ")
                );
            };

            pending.remove(next);
            for deps in pending.values_mut() {
                deps.remove(next);
            }
            ordered.push(next.to_string());
        }

        Ok(ordered)
    }
}

/// Test-only fixtures, so the in-crate test modules that need a `Config` share
/// one definition rather than copying the same TOML into six places.
#[cfg(test)]
pub mod testing {
    use super::Config;

    /// Parse a configuration from TOML text, panicking on failure.
    pub fn config(body: &str) -> Config {
        let dir = std::env::temp_dir().join(format!(
            "midenc-release-cfg-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        Config::load(&path).expect("fixture config must load")
    }

    /// The shape this repository has.
    pub const THREE_UNITS: &str = r#"
schema-version = 2
private-version = "0.1.0"

[units.sdk]
kind = "crates"
version-source = "own"
tag = "sdk/v{version}"
changelog = "sdk/CHANGELOG.md"

[units.templates]
kind = "artifact"
tag = "templates/v{version}"
changelog = "extra/templates/CHANGELOG.md"
changelog-headings = ["Templates", "SDK compatibility"]
assets = ["templates.tar.gz"]

[units.templates.source]
directory = "extra/templates"
manifest = "bundle.toml"
embedded-copy = "tools/embedder/templates.tar.gz"

[units.templates.tracks.sdk]
packages = ["thesdk"]

[units.compiler]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
after = ["sdk", "templates"]
latest = true
assets = ["tool-*.tar.gz"]

[units.private]
kind = "private"

[[packages]]
name = "thesdk"
unit = "sdk"

[[packages]]
name = "thetool"
unit = "compiler"

[[packages]]
name = "internal"
unit = "private"
"#;

    /// One crate, one unit. The single-crate repository shape.
    pub const SINGLE_UNIT: &str = r#"
schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
latest = true

[[packages]]
name = "mytool"
unit = "main"
"#;

    /// Two binaries released independently, sharing one library crate.
    pub const TWO_UNITS_ONE_LIBRARY: &str = r#"
schema-version = 2

[units.tool-a]
kind = "crates"
version-source = "own"
tag = "tool-a/v{version}"
changelog = "a/CHANGELOG.md"
latest = true
assets = ["tool-a-*.tar.gz"]

[units.tool-b]
kind = "crates"
version-source = "own"
tag = "tool-b/v{version}"
changelog = "b/CHANGELOG.md"
assets = ["tool-b-*.tar.gz"]

[units.common]
kind = "library"
version-source = "workspace"

[[packages]]
name = "tool-a"
unit = "tool-a"

[[packages]]
name = "tool-b"
unit = "tool-b"

[[packages]]
name = "common"
unit = "common"
"#;
}
