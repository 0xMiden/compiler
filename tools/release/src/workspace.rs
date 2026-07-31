//! A read-only view of the Cargo workspace.
//!
//! Cargo is invoked as a subprocess rather than linked as a library. That keeps
//! the boundary between "what the release tool decides" and "what Cargo does"
//! explicit, and it is the same boundary the executor will need for packaging
//! and publication.

use std::{collections::BTreeMap, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<MetadataPackage>,
    workspace_root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    name: String,
    version: String,
    dependencies: Vec<MetadataDependency>,
    /// `Some([])` means `publish = false`; `None` means publishable anywhere.
    publish: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct MetadataDependency {
    name: String,
    kind: Option<String>,
    req: String,
}

/// How a dependency edge is treated when ordering packages for publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// Normal or build dependency: always retained in the published manifest.
    Required,
    /// Dev dependency carrying a version requirement: also retained, so it must
    /// be published first. A dev dependency without a version is stripped by
    /// Cargo when packaging and is ignored here.
    VersionedDev,
}

#[derive(Debug, Clone)]
pub struct Package {
    /// The version this package would be published at.
    pub version: String,
    /// Edges to other workspace members only.
    pub local_deps: Vec<(String, EdgeKind)>,
    /// Whether Cargo considers this package publishable.
    pub publishable: bool,
}

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub packages: BTreeMap<String, Package>,
}

impl Workspace {
    /// Load workspace metadata by shelling out to `cargo metadata`.
    pub fn load(manifest_dir: &std::path::Path) -> Result<Self> {
        let output = Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(manifest_dir)
            .output()
            .context("failed to run `cargo metadata`")?;

        if !output.status.success() {
            bail!("`cargo metadata` failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        }

        let metadata: Metadata =
            serde_json::from_slice(&output.stdout).context("failed to parse `cargo metadata`")?;

        let members: BTreeMap<&str, ()> =
            metadata.packages.iter().map(|p| (p.name.as_str(), ())).collect();

        let mut packages = BTreeMap::new();
        for package in &metadata.packages {
            let mut local_deps = Vec::new();
            for dep in &package.dependencies {
                if !members.contains_key(dep.name.as_str()) {
                    continue;
                }
                let kind = match dep.kind.as_deref() {
                    None | Some("build") => EdgeKind::Required,
                    // A dev dependency with no version requirement is stripped
                    // when packaging, so it imposes no publication ordering.
                    Some("dev") if dep.req == "*" => continue,
                    Some("dev") => EdgeKind::VersionedDev,
                    Some(_) => continue,
                };
                local_deps.push((dep.name.clone(), kind));
            }
            local_deps.sort();
            local_deps.dedup();

            packages.insert(
                package.name.clone(),
                Package {
                    version: package.version.clone(),
                    local_deps,
                    publishable: package.publish.as_deref() != Some(&[]),
                },
            );
        }

        Ok(Self {
            root: metadata.workspace_root,
            packages,
        })
    }
}
