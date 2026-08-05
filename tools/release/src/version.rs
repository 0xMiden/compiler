//! Moving a version domain.
//!
//! The repository has two: the compiler crates, which inherit the root
//! `[workspace.package]` version, and the SDK crates, which carry their own but
//! always move together. A bump has to update the version *and* every
//! requirement that names it, or the workspace resolves against versions that
//! do not exist yet.
//!
//! One edge deserves care. `midenc-frontend-wasm-metadata` lives in the SDK
//! version domain but is depended on from the compiler unit, so an SDK bump
//! must rewrite compiler-side requirements too. Missing that would leave the
//! two units silently pinned to different contract versions.
//!
//! Private packages are deliberately left alone even when they sit inside a
//! domain's directory tree. Their versions are never published, so moving them
//! is diff noise, and their dependencies on domain crates are workspace-
//! inherited and keep resolving.
//!
//! Planning is separated from applying so the change can be reviewed before
//! anything is written.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use semver::Version;
use toml_edit::{DocumentMut, Item, Value};

use crate::{
    config::{Config, VersionSource},
    workspace::Workspace,
};

/// What a bump would change in one manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub path: PathBuf,
    pub description: String,
}

#[derive(Debug)]
pub struct VersionPlan {
    pub domain: VersionSource,
    pub old: Version,
    pub new: Version,
    /// Packages whose own version field moves.
    pub packages: BTreeSet<String>,
    pub edits: Vec<Edit>,
}

impl VersionPlan {
    pub fn summary(&self) -> String {
        let mut summary = format!(
            "{:?} domain: {} -> {}\n{} package(s), {} manifest edit(s)\n",
            self.domain,
            self.old,
            self.new,
            self.packages.len(),
            self.edits.len()
        );
        for edit in &self.edits {
            summary.push_str(&format!("  {}: {}\n", edit.path.display(), edit.description));
        }
        summary
    }
}

/// The default bump: next minor, patch reset.
pub fn next_minor(current: &Version) -> Version {
    Version::new(current.major, current.minor + 1, 0)
}

/// Whether a version may move to something not greater than the current one.
///
/// Moving backwards is normally a mistake, but it is legitimate while a
/// candidate is still being prepared: a maintainer who has already bumped to
/// `0.32.0` and then decides to release `0.32.0-rc.1` first is correcting the
/// candidate, not rewriting history. Nothing has been published at that point.
/// The alternative -- discarding the branch and starting over -- is worse, and
/// it is what pushes people into hand-editing manifests, which is how a
/// candidate ends up inconsistent in ways the tooling would have prevented.
///
/// This is safe only while the target version is unpublished. Once a version is
/// on crates.io it can never be reused, and no flag here changes that; the
/// publish path reconciles against the registry and refuses regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Force {
    No,
    /// Permit a move that does not increase the version.
    Yes,
}

impl From<bool> for Force {
    fn from(force: bool) -> Self {
        if force { Self::Yes } else { Self::No }
    }
}

/// Check a requested move, returning an explanation when it is refused.
pub fn check_direction(old: &Version, new: &Version, force: Force) -> Result<()> {
    if new > old || force == Force::Yes {
        return Ok(());
    }
    bail!(
        "refusing to move {old} to {new}: versions must increase.\nIf you are correcting a \
         candidate that has not been published -- moving {old} to a prerelease of it, say -- pass \
         `--force`. A version that has already been published can never be reused, with or \
         without the flag."
    )
}

/// Compute the edits a bump requires, without writing anything.
pub fn plan(
    ws: &Workspace,
    config: &Config,
    domain: VersionSource,
    requested: Option<Version>,
    force: Force,
) -> Result<VersionPlan> {
    let packages: BTreeSet<String> = config
        .packages
        .iter()
        .filter(|p| p.version_source == Some(domain))
        .map(|p| p.name.clone())
        .collect();

    if packages.is_empty() {
        bail!("no packages are assigned to the {domain:?} version domain");
    }

    // Every package in a domain must already agree, or "the domain's version"
    // is not a well-defined thing to move.
    let mut versions: BTreeMap<String, &str> = BTreeMap::new();
    for name in &packages {
        let package = ws
            .packages
            .get(name)
            .with_context(|| format!("'{name}' is configured but not a workspace member"))?;
        versions.insert(name.clone(), package.version.as_str());
    }
    let distinct: BTreeSet<&str> = versions.values().copied().collect();
    if distinct.len() > 1 {
        let detail = versions
            .iter()
            .map(|(name, version)| format!("  {name} {version}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!("packages in the {domain:?} domain disagree about their version:\n{detail}");
    }

    let old = Version::parse(distinct.iter().next().expect("domain is non-empty"))
        .context("current version is not valid SemVer")?;
    let new = requested.unwrap_or_else(|| next_minor(&old));

    check_direction(&old, &new, force)?;

    let mut edits = Vec::new();
    for manifest in manifests(ws) {
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("failed to read {}", manifest.display()))?;
        let mut doc: DocumentMut = text
            .parse()
            .with_context(|| format!("failed to parse {}", manifest.display()))?;
        let descriptions = edit_document(&mut doc, &manifest, ws, &packages, domain, &new);
        for description in descriptions {
            edits.push(Edit {
                path: manifest.clone(),
                description,
            });
        }
    }

    Ok(VersionPlan {
        domain,
        old,
        new,
        packages,
        edits,
    })
}

/// Apply a planned bump, then refresh the lockfile.
pub fn apply(ws: &Workspace, config: &Config, plan: &VersionPlan) -> Result<()> {
    let packages: BTreeSet<String> = config
        .packages
        .iter()
        .filter(|p| p.version_source == Some(plan.domain))
        .map(|p| p.name.clone())
        .collect();

    for manifest in manifests(ws) {
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("failed to read {}", manifest.display()))?;
        let mut doc: DocumentMut = text.parse()?;
        let changed = edit_document(&mut doc, &manifest, ws, &packages, plan.domain, &plan.new);
        if !changed.is_empty() {
            std::fs::write(&manifest, doc.to_string())
                .with_context(|| format!("failed to write {}", manifest.display()))?;
        }
    }

    // Templates hardcode an SDK requirement, so an SDK bump has to carry them
    // along or generated projects stay pinned to the previous SDK. This is
    // silent when missed: the templates still render and still build.
    if plan.domain == VersionSource::Sdk {
        let templates = ws.root.join("extra/templates");
        if templates.join("bundle.toml").exists() {
            let requirement = crate::bundle::requirement_for(&plan.new);
            let changed = crate::bundle::set_sdk_requirement(&templates, &requirement)?;
            if !changed.is_empty() {
                println!(
                    "updated the template SDK requirement to \"{requirement}\" in {} file(s)",
                    changed.len()
                );
            }
        }
    }

    refresh_lockfile(&ws.root)
}

/// Root manifest first, then every member manifest.
fn manifests(ws: &Workspace) -> Vec<PathBuf> {
    let mut paths = vec![ws.root.join("Cargo.toml")];
    paths.extend(ws.packages.values().filter_map(|p| {
        let path = p.manifest_path.clone();
        (path != ws.root.join("Cargo.toml")).then_some(path)
    }));
    paths.sort();
    paths.dedup();
    paths
}

/// Edit one manifest in place, returning a description of each change.
fn edit_document(
    doc: &mut DocumentMut,
    manifest: &Path,
    ws: &Workspace,
    packages: &BTreeSet<String>,
    domain: VersionSource,
    new: &Version,
) -> Vec<String> {
    let mut changes = Vec::new();
    let is_root = manifest == ws.root.join("Cargo.toml");

    if is_root {
        // Compiler crates inherit this; SDK crates do not.
        if domain == VersionSource::Workspace
            && let Some(version) = doc
                .get_mut("workspace")
                .and_then(|w| w.get_mut("package"))
                .and_then(|p| p.get_mut("version"))
            && set_string(version, new)
        {
            changes.push(format!("workspace.package.version = \"{new}\""));
        }

        if let Some(deps) = doc
            .get_mut("workspace")
            .and_then(|w| w.get_mut("dependencies"))
            .and_then(Item::as_table_like_mut)
        {
            changes.extend(update_requirements(deps, packages, new, "workspace.dependencies"));
        }
    } else {
        // A member that carries its own version rather than inheriting one.
        if let Some(package) = doc.get_mut("package")
            && let Some(name) = package.get("name").and_then(|n| n.as_str()).map(str::to_string)
            && packages.contains(&name)
            && let Some(version) = package.get_mut("version")
            && set_string(version, new)
        {
            changes.push(format!("package.version = \"{new}\""));
        }
    }

    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps) = doc.get_mut(table).and_then(Item::as_table_like_mut) {
            changes.extend(update_requirements(deps, packages, new, table));
        }
    }

    changes
}

/// Rewrite every requirement naming a package in the domain.
fn update_requirements(
    deps: &mut dyn toml_edit::TableLike,
    packages: &BTreeSet<String>,
    new: &Version,
    context: &str,
) -> Vec<String> {
    let names: Vec<String> = deps
        .iter()
        .map(|(name, _)| name.to_string())
        .filter(|name| packages.contains(name))
        .collect();

    let mut changes = Vec::new();
    for name in names {
        let Some(item) = deps.get_mut(&name) else {
            continue;
        };
        // `foo = "1.2.3"`
        if item.as_str().is_some() {
            if set_string(item, new) {
                changes.push(format!("{context}.{name} = \"{new}\""));
            }
            continue;
        }
        // `foo = { version = "1.2.3", path = "..." }`. An entry inheriting from
        // the workspace has no version of its own and needs no edit.
        if let Some(table) = item.as_table_like_mut()
            && let Some(version) = table.get_mut("version")
            && set_string(version, new)
        {
            changes.push(format!("{context}.{name}.version = \"{new}\""));
        }
    }
    changes
}

/// Set a TOML string, preserving its decor. Returns false if it already matches.
fn set_string(item: &mut Item, new: &Version) -> bool {
    let new = new.to_string();
    match item.as_str() {
        Some(current) if current == new => false,
        Some(_) => {
            let decor = item.as_value().map(|v| v.decor().clone());
            let mut value = Value::from(new);
            if let Some(decor) = decor {
                *value.decor_mut() = decor;
            }
            *item = Item::Value(value);
            true
        }
        None => false,
    }
}

fn refresh_lockfile(root: &Path) -> Result<()> {
    let status = std::process::Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
        .args(["update", "--workspace"])
        .current_dir(root)
        .status()
        .context("failed to run `cargo update --workspace`")?;
    if !status.success() {
        bail!("`cargo update --workspace` failed; the lockfile may be inconsistent");
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    /// Correcting a candidate in progress is a real situation: an already
    /// bumped `0.32.0` that should ship as `0.32.0-rc.1` first. Without an
    /// escape hatch the alternatives are discarding the branch or hand-editing
    /// manifests, and hand-editing is what skips the template rewrite and
    /// leaves a candidate whose templates cannot resolve their own SDK.
    mod direction {
        use super::*;

        fn v(s: &str) -> Version {
            Version::parse(s).unwrap()
        }

        #[test]
        fn moving_forward_needs_no_force() {
            check_direction(&v("0.9.2"), &v("0.10.0"), Force::No).unwrap();
        }

        #[test]
        fn moving_backwards_is_refused_by_default() {
            let err = check_direction(&v("0.32.0"), &v("0.32.0-rc.1"), Force::No)
                .unwrap_err()
                .to_string();
            assert!(err.contains("versions must increase"), "{err}");
            assert!(err.contains("--force"), "the message must name the way out: {err}");
        }

        #[test]
        fn a_prerelease_of_the_current_version_is_backwards() {
            // SemVer orders a prerelease *below* its release, which is exactly
            // why this needs forcing and why it is nonetheless legitimate.
            assert!(v("0.32.0-rc.1") < v("0.32.0"));
        }

        #[test]
        fn force_permits_a_backwards_move() {
            check_direction(&v("0.32.0"), &v("0.32.0-rc.1"), Force::Yes).unwrap();
        }

        #[test]
        fn force_permits_reapplying_the_same_version() {
            // Re-running to repair a hand-edited candidate: the version does not
            // move, but the template requirements it drives still need writing.
            check_direction(&v("0.14.0-rc.1"), &v("0.14.0-rc.1"), Force::No).unwrap_err();
            check_direction(&v("0.14.0-rc.1"), &v("0.14.0-rc.1"), Force::Yes).unwrap();
        }
    }
    use super::*;

    #[test]
    fn default_bump_is_the_next_minor() {
        assert_eq!(
            next_minor(&Version::parse("0.9.2").unwrap()),
            Version::parse("0.10.0").unwrap()
        );
        assert_eq!(next_minor(&Version::parse("1.0.0").unwrap()), Version::parse("1.1.0").unwrap());
    }

    fn doc(text: &str) -> DocumentMut {
        text.parse().unwrap()
    }

    fn packages(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn requirements_are_rewritten_in_every_form() {
        let mut document = doc(r#"
[dependencies]
plain = "1.0.0"
detailed = { version = "1.0.0", path = "../detailed" }
inherited = { workspace = true }
unrelated = "1.0.0"
"#);
        let deps = document.get_mut("dependencies").unwrap().as_table_like_mut().unwrap();
        let new = Version::parse("2.0.0").unwrap();
        let changes =
            update_requirements(deps, &packages(&["plain", "detailed", "inherited"]), &new, "deps");

        assert_eq!(changes.len(), 2, "inherited entries carry no version to rewrite");
        let text = document.to_string();
        assert!(text.contains(r#"plain = "2.0.0""#), "{text}");
        assert!(text.contains(r#"version = "2.0.0""#), "{text}");
        assert!(text.contains(r#"inherited = { workspace = true }"#), "{text}");
        assert!(
            text.contains(r#"unrelated = "1.0.0""#),
            "unrelated crates must not move: {text}"
        );
    }

    #[test]
    fn rewriting_preserves_surrounding_formatting() {
        let mut document = doc("[dependencies]\n# keep me\nfoo = { version = \"1.0.0\", path = \
                                \"../foo\" }  # trailing\n");
        let deps = document.get_mut("dependencies").unwrap().as_table_like_mut().unwrap();
        update_requirements(deps, &packages(&["foo"]), &Version::parse("1.1.0").unwrap(), "deps");

        let text = document.to_string();
        assert!(text.contains("# keep me"), "{text}");
        assert!(text.contains("# trailing"), "{text}");
        assert!(text.contains(r#"path = "../foo""#), "{text}");
    }

    #[test]
    fn setting_an_unchanged_version_reports_no_change() {
        let mut item = Item::Value(Value::from("1.0.0"));
        assert!(!set_string(&mut item, &Version::parse("1.0.0").unwrap()));
        assert!(set_string(&mut item, &Version::parse("1.0.1").unwrap()));
    }
}
