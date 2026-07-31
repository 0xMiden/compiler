//! Package-closure verification.
//!
//! This is the gate that justifies publishing with `--no-verify`. Production
//! skips Cargo's own verification step so that no package build script runs
//! while a crates.io token is present, which means *this* is the only thing
//! proving the packaged crates actually build from their published form.
//! It is therefore required, not optional.
//!
//! The method is to publish the selected set to a throwaway registry and then
//! build a consumer that resolves exclusively through it. Nothing reaches the
//! consumer by workspace path, so anything the packaged crates forgot to
//! include -- a file excluded from the archive, a dependency that only resolves
//! locally, an active `[patch]` -- fails here rather than after publication.
//!
//! §15.2's "temporary static registry" and §15.4's rehearsal registry are the
//! same thing, so this reuses the latter rather than building a second one.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use anyhow::{Context, Result, bail};

use crate::{
    registry::{CurlUpstream, Faults, NoUpstream, Registry, Upstream, client::IndexClient},
    workspace::Workspace,
};

/// One packaged crate, as the registry received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedCrate {
    pub name: String,
    pub version: String,
    pub digest: String,
    pub size: usize,
}

#[derive(Debug)]
pub struct Closure {
    pub crates: Vec<PackagedCrate>,
}

impl Closure {
    pub fn digests(&self) -> BTreeMap<String, String> {
        self.crates
            .iter()
            .map(|c| (format!("{}-{}", c.name, c.version), c.digest.clone()))
            .collect()
    }
}

pub struct Options {
    /// Packages to verify, in dependency order. Cargo's own ordering is not
    /// reliable at this workspace's scale, so the caller supplies it.
    pub packages: Vec<String>,
    /// Build a consumer against the registry. Skipping this makes the check
    /// much faster and much weaker: resolution alone cannot prove the archives
    /// contain every file they need.
    pub build_consumer: bool,
    /// Resolve third-party dependencies from crates.io. Disable only for
    /// fixtures that have none.
    pub allow_upstream: bool,
    /// Where to cache upstream index responses between runs.
    pub cache_dir: Option<PathBuf>,
}

/// Check that nothing in the selection needs a version that exists neither in
/// the selection nor on the registry.
///
/// The case this exists for is the contract crate: `midenc-frontend-wasm-metadata`
/// lives in the SDK version domain but is depended on from the compiler unit,
/// so verifying or releasing the compiler alone works only while the SDK's
/// current version is already published. Without this check that surfaces as a
/// Cargo resolution error deep inside packaging, which says nothing about
/// release units.
pub fn check_external_dependencies(
    ws: &Workspace,
    index: &dyn IndexClient,
    selected: &[String],
) -> Result<Vec<String>> {
    let selection: std::collections::BTreeSet<&str> = selected.iter().map(String::as_str).collect();
    let mut problems = Vec::new();

    for name in selected {
        let Some(package) = ws.packages.get(name) else {
            continue;
        };
        for (dep, _) in &package.local_deps {
            if selection.contains(dep.as_str()) {
                continue;
            }
            let Some(dep_package) = ws.packages.get(dep) else {
                continue;
            };
            if !dep_package.publishable {
                continue;
            }

            let published = index.versions(dep)?;
            if !published.iter().any(|entry| entry.vers == dep_package.version) {
                problems.push(format!(
                    "'{name}' needs '{dep}' {}, which is outside this selection and not \
                     published; release the unit that owns '{dep}' in the same release, or select \
                     it here",
                    dep_package.version
                ));
            }
        }
    }

    problems.sort();
    problems.dedup();
    Ok(problems)
}

/// Package the selected crates, publish them to a throwaway registry, and
/// verify a consumer resolves and builds through it.
pub fn verify(workspace_root: &Path, options: &Options) -> Result<Closure> {
    if options.packages.is_empty() {
        bail!("no packages selected for closure verification");
    }

    let upstream: Arc<dyn Upstream> = if options.allow_upstream {
        Arc::new(CurlUpstream::new(options.cache_dir.clone()))
    } else {
        Arc::new(NoUpstream)
    };
    let registry = Registry::start(0, Faults::default(), upstream)?;

    // An isolated CARGO_HOME with source replacement. Replacement redirects
    // dependency *resolution* so interdependent unpublished crates resolve
    // against each other; `--index` redirects the *upload target*. Neither
    // alone is sufficient.
    let cargo_home = workspace_root.join("target/release-closure/cargo-home");
    write_cargo_home(&cargo_home, &registry.index_url())?;

    publish_closure(workspace_root, &cargo_home, &registry.index_url(), &options.packages)?;

    let mut crates = Vec::with_capacity(options.packages.len());
    for name in &options.packages {
        let versions = registry.published_versions(name);
        let Some(version) = versions.last() else {
            bail!("'{name}' was selected but never reached the registry");
        };
        let archive = registry
            .archive(name, version)
            .with_context(|| format!("'{name}' has an index entry but no archive"))?;
        crates.push(PackagedCrate {
            name: name.clone(),
            version: version.clone(),
            digest: crate::registry::sha256_hex(&archive),
            size: archive.len(),
        });
    }

    if options.build_consumer {
        let consumer = workspace_root.join("target/release-closure/consumer");
        build_consumer(&consumer, &cargo_home, &crates)?;
    }

    Ok(Closure { crates })
}

fn write_cargo_home(cargo_home: &Path, index_url: &str) -> Result<()> {
    fs::create_dir_all(cargo_home)?;
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with = \"closure\"\n\n[source.closure]\nregistry = \
             \"{index_url}\"\n"
        ),
    )
    .context("failed to configure the closure CARGO_HOME")?;
    Ok(())
}

fn publish_closure(
    workspace_root: &Path,
    cargo_home: &Path,
    index_url: &str,
    packages: &[String],
) -> Result<()> {
    let mut command = cargo();
    command
        .current_dir(workspace_root)
        .env("CARGO_HOME", cargo_home)
        .args(["publish", "--no-verify", "--locked", "--allow-dirty"])
        .args(["--index", index_url])
        .args(["--token", "closure-verification"]);
    for package in packages {
        command.args(["-p", package]);
    }

    let output = command.output().context("failed to run `cargo publish`")?;
    if !output.status.success() {
        bail!(
            "packaging the closure failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Build a crate that depends on the closure and resolves only through the
/// registry. This is what proves the published archives are usable.
fn build_consumer(dir: &Path, cargo_home: &Path, crates: &[PackagedCrate]) -> Result<()> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir.join("src"))?;

    let dependencies: String =
        crates.iter().map(|c| format!("{} = \"={}\"\n", c.name, c.version)).collect();

    fs::write(
        dir.join("Cargo.toml"),
        format!(
            // The empty `[workspace]` table matters: the consumer is written
            // under the workspace's target directory, and without it Cargo
            // treats it as a stray member and refuses to build.
            "[workspace]\n\n[package]\nname = \"release-closure-consumer\"\nversion = \
             \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\n{dependencies}"
        ),
    )?;
    fs::write(dir.join("src/lib.rs"), "")?;

    let output = cargo()
        .current_dir(dir)
        .env("CARGO_HOME", cargo_home)
        .args(["build"])
        .output()
        .context("failed to run `cargo build` for the closure consumer")?;

    if !output.status.success() {
        bail!(
            "the packaged crates do not build when resolved from a registry:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_selection_is_rejected() {
        let options = Options {
            packages: Vec::new(),
            build_consumer: false,
            allow_upstream: false,
            cache_dir: None,
        };
        let err = verify(Path::new("/tmp"), &options).unwrap_err().to_string();
        assert!(err.contains("no packages selected"), "{err}");
    }

    fn workspace(packages: &[(&str, &str, &[&str], bool)]) -> Workspace {
        use crate::workspace::{EdgeKind, Package};
        Workspace {
            root: PathBuf::from("/tmp"),
            packages: packages
                .iter()
                .map(|(name, version, deps, publishable)| {
                    (
                        name.to_string(),
                        Package {
                            version: version.to_string(),
                            manifest_path: PathBuf::from("/tmp/Cargo.toml"),
                            local_deps: deps
                                .iter()
                                .map(|d| (d.to_string(), EdgeKind::Required))
                                .collect(),
                            publishable: *publishable,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn a_cross_unit_dependency_that_is_published_is_fine() {
        use crate::registry::client::StubIndex;
        let ws = workspace(&[
            ("comp", "0.10.0", &["contract"][..], true),
            ("contract", "0.13.1", &[][..], true),
        ]);
        let index = StubIndex::new().publish("contract", "0.13.1", "abc", false);

        let problems = check_external_dependencies(&ws, &index, &["comp".to_string()]).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_cross_unit_dependency_that_is_unpublished_is_reported() {
        use crate::registry::client::StubIndex;
        let ws = workspace(&[
            ("comp", "0.10.0", &["contract"][..], true),
            ("contract", "0.14.0", &[][..], true),
        ]);
        // Only the old version is on the registry, which is the real situation
        // when the SDK has been bumped but not yet released.
        let index = StubIndex::new().publish("contract", "0.13.1", "abc", false);

        let problems = check_external_dependencies(&ws, &index, &["comp".to_string()]).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("contract"), "{}", problems[0]);
        assert!(problems[0].contains("0.14.0"), "{}", problems[0]);
    }

    #[test]
    fn selecting_the_dependency_resolves_the_problem() {
        use crate::registry::client::StubIndex;
        let ws = workspace(&[
            ("comp", "0.10.0", &["contract"][..], true),
            ("contract", "0.14.0", &[][..], true),
        ]);
        let index = StubIndex::new();

        let problems =
            check_external_dependencies(&ws, &index, &["comp".to_string(), "contract".to_string()])
                .unwrap();
        assert!(
            problems.is_empty(),
            "in-selection dependencies are satisfied by the release itself"
        );
    }

    #[test]
    fn private_dependencies_are_not_expected_on_the_registry() {
        use crate::registry::client::StubIndex;
        let ws = workspace(&[
            ("comp", "0.10.0", &["helper"][..], true),
            ("helper", "0.1.0", &[][..], false),
        ]);
        let problems =
            check_external_dependencies(&ws, &StubIndex::new(), &["comp".to_string()]).unwrap();
        assert!(problems.is_empty(), "private crates are stripped when packaging: {problems:?}");
    }

    #[test]
    fn digests_are_keyed_by_name_and_version() {
        let closure = Closure {
            crates: vec![PackagedCrate {
                name: "a".into(),
                version: "1.0.0".into(),
                digest: "abc".into(),
                size: 10,
            }],
        };
        assert_eq!(closure.digests().get("a-1.0.0").map(String::as_str), Some("abc"));
    }
}
