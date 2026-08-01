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

    Ok(findings)
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
        findings.error(format!(
            "the embedded template bundle is stale: {} has sha256 {}, but the sources produce {}. \
             Regenerate it with `release-tool bundle --output tools/cargo-miden/templates.tar.gz`",
            embedded.display(),
            &actual[..16],
            &expected[..16]
        ));
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
