//! Release-candidate preconditions.
//!
//! These checks run on every pull request. They are cheap, and each one
//! corresponds to a way a release has broken or could break: an unclassified
//! package silently joining the release surface, a publishable crate depending
//! on a private one, or an active `[patch]` entry that makes the workspace
//! build correctly while the published crate does not.

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result};

use crate::{
    config::{Config, Unit},
    workspace::Workspace,
};

/// The frozen version every private package carries.
pub const PRIVATE_VERSION: &str = "0.1.0";

#[derive(Debug, Default)]
pub struct Findings {
    pub errors: Vec<String>,
}

impl Findings {
    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn run(ws: &Workspace, config: &Config) -> Result<Findings> {
    let mut findings = Findings::default();

    check_classification(ws, config, &mut findings);
    check_private_versions(ws, config, &mut findings);
    check_private_dependencies(ws, config, &mut findings);
    check_active_patches(&ws.root, &mut findings)?;
    check_embedded_bundle(&ws.root, &mut findings)?;
    check_sdk_requirement_matches_release(ws, &mut findings)?;

    Ok(findings)
}

/// The templates' `miden` requirement must be able to resolve the SDK version
/// this candidate releases.
///
/// `check_sdk_requirements` asks whether the templates agree with `bundle.toml`.
/// This asks the question that actually matters: whether what they agree on can
/// resolve anything. The two come apart on a prerelease — a caret requirement
/// never matches one, so an SDK at `0.14.0-rc.1` beneath templates requiring
/// `"0.14"` leaves every generated project unable to resolve the very SDK it
/// was released beside, while both files look perfectly consistent.
fn check_sdk_requirement_matches_release(ws: &Workspace, findings: &mut Findings) -> Result<()> {
    let candidate_path = ws.root.join(".release/release.toml");
    let templates = ws.root.join("extra/templates");
    if !candidate_path.exists() || !templates.join("bundle.toml").exists() {
        return Ok(());
    }

    let candidate = crate::candidate::Candidate::load(&candidate_path)?;
    let Some(sdk) = candidate
        .declarations
        .get("sdk")
        .filter(|_| candidate.units.iter().any(|unit| unit == "sdk"))
    else {
        // The SDK is not being released, so the templates' requirement refers to
        // a version that is already published and outside this candidate.
        return Ok(());
    };

    let bundle = crate::bundle::Bundle::load(&templates.join("bundle.toml"))?;
    let expected = crate::bundle::requirement_for(&sdk.version);

    if bundle.sdk_requirement != expected {
        findings.error(format!(
            "the templates require `miden = \"{}\"`, which cannot resolve the SDK version this \
             release publishes ({}). It must be \"{expected}\". Fix it with `cargo make release \
             set-version --unit sdk {}` rather than by hand, which rewrites the bundle manifest \
             and every template together",
            bundle.sdk_requirement, sdk.version, sdk.version
        ));
    }
    Ok(())
}

/// Every workspace member is classified exactly once, and the classification
/// agrees with the manifest's own `publish` field.
fn check_classification(ws: &Workspace, config: &Config, findings: &mut Findings) {
    let classified: BTreeSet<&str> = config.packages.iter().map(|p| p.name.as_str()).collect();

    for name in ws.packages.keys() {
        if !classified.contains(name.as_str()) {
            findings.error(format!(
                "package '{name}' is not classified in .release/config.toml; add it with an \
                 explicit unit and publish setting"
            ));
        }
    }

    for package in &config.packages {
        let Some(actual) = ws.packages.get(&package.name) else {
            findings.error(format!(
                "package '{}' is classified in .release/config.toml but is not a workspace member",
                package.name
            ));
            continue;
        };

        if actual.publishable != package.publish {
            let (manifest, config_says) = if actual.publishable {
                ("publishable", "private")
            } else {
                ("publish = false", "publishable")
            };
            findings.error(format!(
                "package '{}' is {manifest} in its manifest but classified as {config_says} in \
                 .release/config.toml",
                package.name
            ));
        }
    }
}

/// Private packages are pinned at [`PRIVATE_VERSION`] and never move.
///
/// A private crate carrying a plausible-looking version invites the reader to
/// assume it ships with the release. Freezing them all at one obviously inert
/// version says the opposite, and keeps release churn off manifests that are
/// never published.
fn check_private_versions(ws: &Workspace, config: &Config, findings: &mut Findings) {
    for package in config.packages_in(Unit::Private) {
        let Some(actual) = ws.packages.get(&package.name) else {
            continue;
        };
        if actual.version != PRIVATE_VERSION {
            findings.error(format!(
                "private package '{}' is at {} but private packages are frozen at {}; they are \
                 never published, so a version that tracks a release domain is misleading",
                package.name, actual.version, PRIVATE_VERSION
            ));
        }
    }
}

/// A publishable crate cannot depend on a private one: the dependency would be
/// unresolvable for anyone consuming the published crate.
///
/// Dev dependencies without a version requirement are exempt, because Cargo
/// strips them when packaging.
fn check_private_dependencies(ws: &Workspace, config: &Config, findings: &mut Findings) {
    let private: BTreeSet<&str> =
        config.packages_in(Unit::Private).map(|p| p.name.as_str()).collect();

    for package in config.packages.iter().filter(|p| p.publish) {
        let Some(actual) = ws.packages.get(&package.name) else {
            continue;
        };
        for (dep, _) in &actual.local_deps {
            if private.contains(dep.as_str()) {
                findings.error(format!(
                    "publishable package '{}' depends on private package '{dep}'",
                    package.name
                ));
            }
        }
    }
}

/// The archive embedded in `cargo-miden` must match the template sources.
///
/// The archive is committed rather than generated at build time, because
/// `cargo-miden` is published and a `.crate` cannot contain files from outside
/// its own directory. A committed artifact drifts unless something checks it,
/// and drift here is invisible: `cargo miden new` keeps working, from stale
/// templates.
fn check_embedded_bundle(root: &Path, findings: &mut Findings) -> Result<()> {
    let templates = root.join("extra/templates");
    let embedded = root.join("tools/cargo-miden/templates.tar.gz");
    if !templates.join("bundle.toml").exists() || !embedded.exists() {
        return Ok(());
    }

    let bundle = crate::bundle::Bundle::load(&templates.join("bundle.toml"))?;
    let (_, expected) = crate::bundle::archive(&templates, &bundle)?;
    let actual = crate::registry::sha256_hex(&std::fs::read(&embedded)?);

    if actual != expected {
        let mut message = format!(
            "the embedded template bundle is stale: {} has sha256 {}, but the sources produce {}. \
             Regenerate it with `release-tool bundle --output tools/cargo-miden/templates.tar.gz`",
            embedded.display(),
            &actual[..16],
            &expected[..16]
        );

        // The bundle is built from tracked files, so an untracked one in a
        // template directory is the likeliest reason two checkouts of the same
        // commit disagree -- and the reason is invisible from the digests
        // alone. This turns "it differs in CI" into an answer.
        let strays = crate::bundle::untracked(&bundle, &templates)?;
        if !strays.is_empty() {
            let list: Vec<String> = strays
                .iter()
                .map(|path| format!("extra/templates/{}", path.display()))
                .collect();
            message.push_str(&format!(
                ".\nNote: these files sit in a template directory but are not tracked by git, so \
                 they are not in the bundle and never reach a generated project: {}. Commit them \
                 (`git add -f` if something is ignoring them) or remove them",
                list.join(", ")
            ));
        }

        findings.error(message);
    }
    Ok(())
}

/// An active `[patch]` entry is the most likely way to publish a broken crate:
/// the workspace builds, the normalized manifest looks correct, and the
/// published crate resolves to a registry version that does not have the patched
/// behavior.
fn check_active_patches(root: &Path, findings: &mut Findings) -> Result<()> {
    let manifest_path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;

    let mut in_patch_section = false;
    for (number, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_patch_section = trimmed.starts_with("[patch");
            continue;
        }
        if !in_patch_section || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        findings.error(format!(
            "Cargo.toml:{}: active [patch] entry `{trimmed}`; a release candidate must resolve \
             every dependency from the registry",
            number + 1
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A candidate on disk: the release declaration and a template bundle.
    fn fixture(label: &str, sdk_version: &str, sdk_requirement: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("lint-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".release")).unwrap();
        std::fs::create_dir_all(root.join("extra/templates")).unwrap();

        std::fs::write(
            root.join(".release/release.toml"),
            format!(
                "schema-version = 1\nunits = [\"sdk\"]\n\n[sdk]\nversion = \"{sdk_version}\"\ntag \
                 = \"sdk/v{sdk_version}\"\nprerelease = {}\n",
                sdk_version.contains('-')
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("extra/templates/bundle.toml"),
            format!(
                "schema-version = 1\nversion = \"1.0.0\"\nsdk-requirement = \
                 \"{sdk_requirement}\"\n\n[templates]\naccount = {{ path = \"rust/account\" }}\n"
            ),
        )
        .unwrap();
        root
    }

    fn check(root: &std::path::Path) -> Findings {
        let ws = Workspace {
            root: root.to_path_buf(),
            packages: Default::default(),
        };
        let mut findings = Findings::default();
        check_sdk_requirement_matches_release(&ws, &mut findings).unwrap();
        findings
    }

    /// The defect this exists for: both files agree on `"0.14"`, so the drift
    /// check passes, but a caret requirement never matches a prerelease and
    /// every generated project would fail to resolve the SDK it shipped with.
    #[test]
    fn a_caret_requirement_cannot_resolve_a_prerelease_sdk() {
        let root = fixture("prerelease", "0.14.0-rc.1", "0.14");
        let findings = check(&root);

        assert_eq!(findings.errors.len(), 1, "{:?}", findings.errors);
        assert!(findings.errors[0].contains("cannot resolve"), "{}", findings.errors[0]);
        assert!(findings.errors[0].contains("0.14.0-rc.1"), "{}", findings.errors[0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_exact_requirement_matches_a_prerelease_sdk() {
        let root = fixture("exact", "0.14.0-rc.1", "0.14.0-rc.1");
        assert!(check(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A stable SDK keeps the minor-level requirement, so a later patch needs no
    /// template change.
    #[test]
    fn a_stable_sdk_wants_the_minor_requirement() {
        let root = fixture("stable", "0.14.0", "0.14");
        assert!(check(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stable_sdk_pinned_exactly_is_reported() {
        let root = fixture("overpinned", "0.14.0", "0.14.0");
        let findings = check(&root);
        assert_eq!(findings.errors.len(), 1, "{:?}", findings.errors);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// When the SDK is not part of the release, its requirement names an
    /// already-published version and is none of this check's business.
    #[test]
    fn a_release_without_the_sdk_is_not_checked() {
        let root = fixture("nosdk", "0.14.0-rc.1", "0.13");
        std::fs::write(
            root.join(".release/release.toml"),
            "schema-version = 1\nunits = [\"compiler\"]\n\n[compiler]\nversion = \"0.10.0\"\ntag \
             = \"v0.10.0\"\nprerelease = false\n",
        )
        .unwrap();
        assert!(check(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
