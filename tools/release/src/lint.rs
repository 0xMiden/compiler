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
    config::{Config, UnitKind},
    workspace::Workspace,
};

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

pub fn run(ws: &Workspace, config: &Config, candidate_path: &Path) -> Result<Findings> {
    let mut findings = Findings::default();

    check_classification(ws, config, &mut findings);
    check_private_versions(ws, config, &mut findings);
    check_private_dependencies(ws, config, &mut findings);
    check_active_patches(&ws.root, &mut findings)?;
    check_embedded_bundle(&ws.root, config, &mut findings)?;
    check_tracked_requirements(ws, config, candidate_path, &mut findings)?;

    Ok(findings)
}

/// A unit's requirement on a tracked unit must be able to resolve the version
/// that tracked unit actually releases as part of this candidate.
///
/// `bundle::check_requirements` asks whether the sources agree with the
/// manifest's declared requirement. This asks the question that actually
/// matters: whether what they agree on can resolve anything. The two come
/// apart on a prerelease — a caret requirement never matches one, so an SDK at
/// `0.14.0-rc.1` beneath templates requiring `"0.14"` leaves every generated
/// project unable to resolve the very SDK it was released beside, while both
/// files look perfectly consistent.
fn check_tracked_requirements(
    ws: &Workspace,
    config: &Config,
    candidate_path: &Path,
    findings: &mut Findings,
) -> Result<()> {
    if !candidate_path.exists() {
        return Ok(());
    }
    let candidate = crate::candidate::Candidate::load(candidate_path)?;

    for (name, unit) in config.releasable() {
        let Some(source) = &unit.source else {
            continue;
        };
        let (Some(directory), Some(manifest)) = (&source.directory, &source.manifest) else {
            continue;
        };
        let manifest_path = ws.root.join(directory).join(manifest);
        if !manifest_path.exists() {
            continue;
        }

        for tracked in unit.tracks.keys() {
            let Some(declaration) = candidate
                .declarations
                .get(tracked)
                .filter(|_| candidate.units.iter().any(|selected| selected == tracked))
            else {
                // The tracked unit is not being released, so this unit's
                // requirement refers to a version that is already published
                // and outside this candidate.
                continue;
            };

            let bundle = crate::bundle::Bundle::load(&manifest_path)?;
            let key = unit.requirement_key(tracked);
            let expected = crate::bundle::requirement_for(&declaration.version);

            let declared = match bundle.requirement(&key) {
                Ok(declared) => declared,
                Err(problem) => {
                    findings.error(format!(
                        "unit '{name}' tracks '{tracked}' but its manifest declares no \
                         requirement under '{key}': {problem:#}"
                    ));
                    continue;
                }
            };

            if declared != expected {
                findings.error(format!(
                    "unit '{name}' requires `{key} = \"{declared}\"`, which cannot resolve the \
                     version this release publishes for '{tracked}' ({}). It must be \
                     \"{expected}\". Fix it with `release-tool set-version --unit {tracked} {}` \
                     rather than by hand, which rewrites the manifest and every tracked source \
                     together",
                    declaration.version, declaration.version
                ));
            }
        }
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

        if actual.publishable != config.is_publishable(package) {
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

/// Private packages are pinned at the configured frozen version and never move.
///
/// A private crate carrying a plausible-looking version invites the reader to
/// assume it ships with the release. Freezing them all at one obviously inert
/// version says the opposite, and keeps release churn off manifests that are
/// never published.
fn check_private_versions(ws: &Workspace, config: &Config, findings: &mut Findings) {
    let Some(frozen) = config.private_version.as_deref() else {
        return;
    };
    for (name, _) in config.units_of_kind(UnitKind::Private) {
        for package in config.packages_in(name) {
            let Some(actual) = ws.packages.get(&package.name) else {
                continue;
            };
            if actual.version != frozen {
                findings.error(format!(
                    "private package '{}' is at {} but private packages are frozen at {frozen}; \
                     they are never published, so a version that tracks a release domain is \
                     misleading",
                    package.name, actual.version
                ));
            }
        }
    }
}

/// A publishable crate cannot depend on a private one: the dependency would be
/// unresolvable for anyone consuming the published crate.
///
/// Dev dependencies without a version requirement are exempt, because Cargo
/// strips them when packaging.
fn check_private_dependencies(ws: &Workspace, config: &Config, findings: &mut Findings) {
    let private: BTreeSet<&str> = config
        .units_of_kind(UnitKind::Private)
        .flat_map(|(name, _)| config.packages_in(name))
        .map(|p| p.name.as_str())
        .collect();

    for package in config.publishable() {
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

/// A committed copy of an artifact unit's archive must match its sources.
///
/// The archive is committed rather than generated at build time, because the
/// crate that embeds it is published and a `.crate` cannot contain files from
/// outside its own directory. A committed artifact drifts unless something
/// checks it, and drift here is invisible: the tool keeps working, from stale
/// sources.
fn check_embedded_bundle(root: &Path, config: &Config, findings: &mut Findings) -> Result<()> {
    for (name, unit) in config.releasable() {
        let Some(source) = &unit.source else {
            continue;
        };
        let (Some(directory), Some(embedded)) = (&source.directory, &source.embedded_copy) else {
            continue;
        };

        let sources = root.join(directory);
        let embedded_path = root.join(embedded);
        // An inline-include unit has no manifest, and seeds the archive with
        // nothing. Its embedded copy is checked all the same.
        let manifest_path = source.manifest.as_ref().map(|manifest| sources.join(manifest));
        if manifest_path.as_ref().is_some_and(|path| !path.exists()) || !embedded_path.exists() {
            continue;
        }

        let bundle = manifest_path.map(|path| crate::bundle::Bundle::load(&path)).transpose()?;
        let include = crate::bundle::include_paths(root, unit)?;
        let seed = bundle.as_ref().map(crate::bundle::Bundle::manifest_name);
        let (_, expected) = crate::bundle::archive(&sources, seed, &include)?;
        let actual = crate::registry::sha256_hex(&std::fs::read(&embedded_path)?);

        if actual != expected {
            let mut message = format!(
                "the embedded archive for unit '{name}' is stale: {} has sha256 {}, but the \
                 sources produce {}. Regenerate it with `release-tool bundle --unit {name} \
                 --output {}`",
                embedded_path.display(),
                &actual[..16],
                &expected[..16],
                embedded_path.strip_prefix(root).unwrap_or(&embedded_path).display()
            );

            // The bundle is built from tracked files, so an untracked one under
            // an included path is the likeliest reason two checkouts of the
            // same commit disagree -- and the reason is invisible from the
            // digests alone. This turns "it differs in CI" into an answer.
            let strays = crate::bundle::untracked(&sources, &include)?;
            if !strays.is_empty() {
                let list: Vec<String> =
                    strays.iter().map(|path| directory.join(path).display().to_string()).collect();
                message.push_str(&format!(
                    ".\nNote: these files sit under the sources for unit '{name}' but are not \
                     tracked by git, so they are not in the bundle and never reach a generated \
                     project: {}. Commit them (`git add -f` if something is ignoring them) or \
                     remove them",
                    list.join(", ")
                ));
            }

            findings.error(message);
        }
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

    /// Like `fixture`, but the generated bundle declares no requirement at all
    /// under the tracked key -- the case `check_tracked_requirements` handles
    /// by recording a finding and continuing, not by propagating an error.
    fn fixture_without_requirement(label: &str, sdk_version: &str) -> std::path::PathBuf {
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
            "schema-version = 1\nversion = \"1.0.0\"\n\n[templates]\naccount = { path = \
             \"rust/account\" }\n",
        )
        .unwrap();
        root
    }

    fn check(root: &std::path::Path) -> Findings {
        let ws = Workspace {
            root: root.to_path_buf(),
            packages: Default::default(),
        };
        let config = crate::config::testing::config(crate::config::testing::THREE_UNITS);
        let mut findings = Findings::default();
        check_tracked_requirements(
            &ws,
            &config,
            &root.join(".release/release.toml"),
            &mut findings,
        )
        .unwrap();
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

    /// Task 12's behavior change: a unit whose manifest declares no
    /// requirement under its tracked key is a finding, not an error that
    /// propagates and aborts the whole lint run.
    #[test]
    fn a_manifest_missing_the_declared_requirement_key_is_a_finding_not_an_error() {
        let root = fixture_without_requirement("missing-key", "0.14.0");
        let findings = check(&root);

        assert_eq!(findings.errors.len(), 1, "{:?}", findings.errors);
        assert!(findings.errors[0].contains("sdk-requirement"), "{}", findings.errors[0]);
        assert!(findings.errors[0].contains("templates"), "{}", findings.errors[0]);
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

    #[test]
    fn a_repository_with_no_artifact_units_is_not_checked() {
        let config = crate::config::testing::config(crate::config::testing::SINGLE_UNIT);
        let mut findings = Findings::default();
        check_embedded_bundle(std::path::Path::new("/nonexistent"), &config, &mut findings)
            .unwrap();
        assert!(findings.is_empty(), "{:?}", findings.errors);
    }

    #[test]
    fn an_absent_embedded_copy_is_skipped_not_reported() {
        // Absence is not drift. Reporting it would make the check fire in
        // every repository that has not built the archive yet.
        let config = crate::config::testing::config(crate::config::testing::THREE_UNITS);
        let mut findings = Findings::default();
        check_embedded_bundle(std::path::Path::new("/nonexistent"), &config, &mut findings)
            .unwrap();
        assert!(findings.is_empty(), "{:?}", findings.errors);
    }

    /// A minimal config with one private package, and no `private-version` --
    /// the check must not run.
    const PRIVATE_UNIT_CONFIG: &str = r#"
schema-version = 2

[units.private]
kind = "private"

[[packages]]
name = "internal"
unit = "private"
"#;

    /// The same shape, frozen at `"0.1.0"` -- the check must run.
    const PRIVATE_UNIT_CONFIG_FROZEN: &str = r#"
schema-version = 2
private-version = "0.1.0"

[units.private]
kind = "private"

[[packages]]
name = "internal"
unit = "private"
"#;

    /// A workspace with one private package, `internal`, at `version`.
    fn workspace_with_private_package(version: &str) -> Workspace {
        let mut packages = std::collections::BTreeMap::new();
        packages.insert(
            "internal".to_string(),
            crate::workspace::Package {
                version: version.to_string(),
                manifest_path: std::path::PathBuf::from("internal/Cargo.toml"),
                local_deps: Vec::new(),
                publishable: false,
            },
        );
        Workspace {
            root: std::path::PathBuf::from("/nonexistent"),
            packages,
        }
    }

    #[test]
    fn private_versions_are_not_checked_when_unset() {
        let config = crate::config::testing::config(PRIVATE_UNIT_CONFIG);
        assert!(config.private_version.is_none());

        // Without a frozen version, a private package at any version -- even
        // one that would clearly drift once a version is configured -- must
        // produce no finding.
        let ws = workspace_with_private_package("9.9.9");
        let mut findings = Findings::default();
        check_private_versions(&ws, &config, &mut findings);

        assert!(findings.is_empty(), "{:?}", findings.errors);
    }

    /// The positive case that makes the negative one mean something: the same
    /// workspace, with `private-version` set, does produce a finding.
    #[test]
    fn a_private_package_that_drifts_from_the_frozen_version_is_reported() {
        let config = crate::config::testing::config(PRIVATE_UNIT_CONFIG_FROZEN);
        assert_eq!(config.private_version.as_deref(), Some("0.1.0"));

        let ws = workspace_with_private_package("9.9.9");
        let mut findings = Findings::default();
        check_private_versions(&ws, &config, &mut findings);

        assert_eq!(findings.errors.len(), 1, "{:?}", findings.errors);
        assert!(findings.errors[0].contains("internal"), "{}", findings.errors[0]);
        assert!(findings.errors[0].contains("9.9.9"), "{}", findings.errors[0]);
    }
}
