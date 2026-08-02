//! Creating and populating draft releases.
//!
//! This is Phase C, and it is the last point at which a release can be undone.
//! Drafts are created, filled, and read back while they can still be deleted;
//! the tag does not exist yet and no crate has been published. Everything after
//! the approval gate is permanent.
//!
//! Each draft carries its unit's payload plus the sealed plan, so a release is
//! self-describing after the fact: the plan records what was intended and the
//! assets are what was produced.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::{
    github::{self, GitHub},
    plan::Plan,
};

/// The checksum file staged beside a unit's payload.
pub const SHA256SUMS: &str = "SHA256SUMS";

/// Prefix of the asset carrying the sealed plan. The plan's digest is appended,
/// so a draft names the run it belongs to.
pub const PLAN_ASSET_PREFIX: &str = "release-plan-";

/// What to attach to a unit's draft.
#[derive(Debug, Default)]
pub struct Payload {
    /// Files to upload, by asset name.
    pub assets: BTreeMap<String, PathBuf>,
}

impl Payload {
    pub fn add(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) {
        self.assets.insert(name.into(), path.into());
    }
}

#[derive(Debug)]
pub struct Staged {
    pub unit: String,
    pub tag: String,
    pub release_id: u64,
    pub assets: Vec<github::Asset>,
}

/// Create a draft for every unit in the plan and populate it.
///
/// A draft that already exists is reused rather than duplicated: this runs
/// again on a resume, and creating a second draft for the same tag would leave
/// two candidates for publication.
pub fn stage(
    github: &dyn GitHub,
    plan: &Plan,
    payloads: &BTreeMap<String, Payload>,
) -> Result<Vec<Staged>> {
    let mut staged = Vec::new();

    for tag in &plan.intent.tags {
        let stage_info = plan
            .intent
            .stages
            .iter()
            .find(|stage| stage.unit == tag.unit)
            .with_context(|| format!("unit '{}' has a tag but no stage", tag.unit))?;

        let release = match github.release_by_tag(&tag.name)? {
            Some(existing) if !existing.draft => bail!(
                "a published release already exists for '{}'; it cannot be modified, so this \
                 version must be abandoned and a new one released",
                tag.name
            ),
            Some(existing) => existing,
            None => github
                .create_draft(&tag.name, &plan.intent.subject, stage_info.prerelease)
                .with_context(|| format!("creating a draft for '{}'", tag.name))?,
        };

        let mut to_upload: Vec<(String, Vec<u8>)> = Vec::new();
        if let Some(payload) = payloads.get(&tag.unit) {
            for (name, path) in &payload.assets {
                let bytes = std::fs::read(path)
                    .with_context(|| format!("reading asset '{name}' from {}", path.display()))?;
                to_upload.push((name.clone(), bytes));
            }
        }

        // A checksum file over the payload, so a consumer can verify a download
        // without consulting the release API.
        if !to_upload.is_empty() {
            to_upload.push((SHA256SUMS.to_string(), sha256sums(&to_upload).into_bytes()));
        }

        // The sealed plan travels with every draft: it is the audit record and
        // the input a resume reads.
        to_upload.push((
            format!("{PLAN_ASSET_PREFIX}{}.json", &plan.digest()[..16]),
            plan.to_canonical_json().into_bytes(),
        ));

        // Uploading and reading back happens while the release is still mutable,
        // which is the only time a mismatch can be corrected.
        let assets = github::upload_and_verify(github, release.id, &to_upload)
            .with_context(|| format!("populating the draft for '{}'", tag.name))?;

        staged.push(Staged {
            unit: tag.unit.clone(),
            tag: tag.name.clone(),
            release_id: release.id,
            assets,
        });
    }

    Ok(staged)
}

/// Delete every still-draft release belonging to a plan.
///
/// This is what makes Phase C reversible. A published release is left alone;
/// it cannot be removed, and pretending otherwise would hide the problem.
pub fn discard(github: &dyn GitHub, plan: &Plan) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for tag in &plan.intent.tags {
        let Some(release) = github.release_by_tag(&tag.name)? else {
            continue;
        };
        if !release.draft {
            continue;
        }
        github.delete_release(release.id)?;
        removed.push(tag.name.clone());
    }
    Ok(removed)
}

fn sha256sums(assets: &[(String, Vec<u8>)]) -> String {
    let mut lines: Vec<String> = assets
        .iter()
        .map(|(name, bytes)| format!("{}  {name}", crate::registry::sha256_hex(bytes)))
        .collect();
    lines.sort();
    format!("{}\n", lines.join("\n"))
}

/// Package a built executable into a deterministic archive.
///
/// The archive contains exactly one entry, the executable itself, at the root
/// with no enclosing directory: extracting it yields something ready to put on
/// `PATH`.
pub fn archive_binary(binary: &Path, name: &str) -> Result<Vec<u8>> {
    let bytes = std::fs::read(binary)
        .with_context(|| format!("reading the executable at {}", binary.display()))?;
    crate::archive::tar_gz(vec![crate::archive::Entry {
        path: name.to_string(),
        bytes,
        executable: true,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        github::StubGitHub,
        intent::{Intent, Stage, Tag},
        plan::{Plan, SealedPackage},
    };

    fn plan(units: &[&str]) -> Plan {
        Plan {
            schema_version: 1,
            intent: Intent {
                schema_version: 1,
                subject: "abc123".into(),
                candidate_digest: "cand".into(),
                stages: units
                    .iter()
                    .map(|unit| Stage {
                        unit: unit.to_string(),
                        version: "1.0.0".into(),
                        prerelease: false,
                        packages: vec![],
                    })
                    .collect(),
                tags: units
                    .iter()
                    .map(|unit| Tag {
                        unit: unit.to_string(),
                        name: format!("{unit}/v1.0.0"),
                    })
                    .collect(),
            },
            packages: vec![SealedPackage {
                name: "a".into(),
                version: "1.0.0".into(),
                digest: "d".into(),
                size: 1,
            }],
        }
    }

    fn payload_with(dir: &Path, name: &str, bytes: &[u8]) -> BTreeMap<String, Payload> {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut payload = Payload::default();
        payload.add(name, path);
        [("templates".to_string(), payload)].into_iter().collect()
    }

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("staging-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn every_unit_gets_a_draft_carrying_the_sealed_plan() {
        let github = StubGitHub::new();
        let plan = plan(&["sdk", "compiler"]);

        let staged = stage(&github, &plan, &BTreeMap::new()).unwrap();
        assert_eq!(staged.len(), 2);
        for entry in &staged {
            assert!(
                entry.assets.iter().any(|a| a.name.starts_with("release-plan-")),
                "every draft carries the plan: {:?}",
                entry.assets
            );
        }
    }

    #[test]
    fn payload_assets_are_uploaded_with_a_checksum_file() {
        let dir = temp("payload");
        let github = StubGitHub::new();
        let plan = plan(&["templates"]);
        let payloads = payload_with(&dir, "templates.tar.gz", b"bundle-bytes");

        let staged = stage(&github, &plan, &payloads).unwrap();
        let names: Vec<&str> = staged[0].assets.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"templates.tar.gz"), "{names:?}");
        assert!(names.contains(&"SHA256SUMS"), "{names:?}");

        let sums = github.download_asset(staged[0].release_id, "SHA256SUMS").unwrap();
        let sums = String::from_utf8(sums).unwrap();
        assert!(sums.contains(&crate::registry::sha256_hex(b"bundle-bytes")), "{sums}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging_again_reuses_the_draft_rather_than_duplicating_it() {
        let github = StubGitHub::new();
        let plan = plan(&["sdk"]);

        let first = stage(&github, &plan, &BTreeMap::new()).unwrap();
        let second = stage(&github, &plan, &BTreeMap::new()).unwrap();
        assert_eq!(
            first[0].release_id, second[0].release_id,
            "a resume must not leave two candidates for publication"
        );
    }

    #[test]
    fn a_published_release_stops_staging() {
        let github = StubGitHub::new();
        let plan = plan(&["sdk"]);
        let release = github.create_draft("sdk/v1.0.0", "abc123", false).unwrap();
        github.publish_release(release.id, false).unwrap();

        let err = stage(&github, &plan, &BTreeMap::new()).unwrap_err().to_string();
        assert!(err.contains("cannot be modified"), "{err}");
        assert!(err.contains("abandoned"), "{err}");
    }

    #[test]
    fn discarding_removes_drafts_and_leaves_published_releases() {
        let github = StubGitHub::new();
        let plan = plan(&["sdk", "compiler"]);
        stage(&github, &plan, &BTreeMap::new()).unwrap();

        // Publish one of them; only the other should be discardable.
        let published = github.release_by_tag("compiler/v1.0.0").unwrap().unwrap();
        github.publish_release(published.id, false).unwrap();

        let removed = discard(&github, &plan).unwrap();
        assert_eq!(removed, ["sdk/v1.0.0"]);
        assert!(github.release_by_tag("compiler/v1.0.0").unwrap().is_some());
    }

    #[test]
    fn a_binary_archive_holds_one_executable_at_the_root() {
        let dir = temp("binary");
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("midenc");
        std::fs::write(&binary, b"#!/bin/sh\necho hi\n").unwrap();

        let archive = archive_binary(&binary, "midenc").unwrap();
        assert_eq!(&archive[..2], &[0x1f, 0x8b]);

        // Deterministic, like every other release artifact.
        assert_eq!(archive, archive_binary(&binary, "midenc").unwrap());

        let extract = dir.join("out");
        std::fs::create_dir_all(&extract).unwrap();
        std::fs::write(dir.join("a.tar.gz"), &archive).unwrap();
        let status = std::process::Command::new("tar")
            .args(["-xzf", dir.join("a.tar.gz").to_str().unwrap()])
            .current_dir(&extract)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(extract.join("midenc").is_file(), "no enclosing directory");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
