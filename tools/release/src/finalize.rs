//! Phase E: turning staged drafts into published releases.
//!
//! This is the last phase, and the only one whose effects cannot be undone even
//! in principle: a published release is immutable, so everything checkable is
//! checked before the first draft is published rather than after.
//!
//! What it verifies, and why each one is here rather than assumed:
//!
//! - **The tag points at the subject.** Read from the tag *ref*, not from the
//!   release's `target_commitish`: GitHub ignores that field once the tag
//!   exists, so it reports what was requested rather than what is true.
//! - **The draft carries this plan.** The sealed plan travels with every draft,
//!   so finalizing the wrong run's draft is detectable rather than silent.
//! - **Every asset still hashes to what was uploaded.** Assets were verified at
//!   staging time, but staging and finalization are different jobs, minutes and
//!   an approval gate apart.
//! - **Nothing extra is attached.** An asset nobody planned is either a mistake
//!   or an intrusion, and after publication it can never be removed.
//!
//! Units are published in dependency order — SDK, then templates, then compiler
//! — so that a consumer who sees a compiler release can already resolve
//! everything it refers to.

use anyhow::{Context, Result, bail};

use crate::{
    github::GitHub,
    plan::Plan,
    registry::sha256_hex,
    staging::{PLAN_ASSET_PREFIX, SHA256SUMS},
};

/// What finalization did for one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finalized {
    pub unit: String,
    pub tag: String,
    pub release_id: u64,
    /// Whether this release claimed the repository's "latest" slot.
    pub latest: bool,
    /// True when the release was already published before this run, which is
    /// what a resume after a partial finalization looks like.
    pub already_published: bool,
}

/// Publication order. A unit absent from the plan is simply skipped.
///
/// The compiler goes last because it depends on the SDK, and templates go
/// before it because a compiler release announces template compatibility.
const ORDER: [&str; 3] = ["sdk", "templates", "compiler"];

pub fn order_units(plan: &Plan) -> Vec<String> {
    let mut ordered: Vec<String> = ORDER
        .iter()
        .filter(|unit| plan.intent.tags.iter().any(|tag| tag.unit == **unit))
        .map(|unit| unit.to_string())
        .collect();

    // Anything the plan names that this list does not know about still has to
    // be released; dropping it silently would publish a partial release and
    // report success.
    for tag in &plan.intent.tags {
        if !ordered.contains(&tag.unit) {
            ordered.push(tag.unit.clone());
        }
    }
    ordered
}

/// Whether a unit may claim the repository's "latest release" slot.
///
/// Only a stable compiler release. The SDK and the template bundle are not what
/// someone means by "the latest release" of this repository, and a prerelease
/// claiming the slot would hand it to something explicitly not recommended.
fn claims_latest(plan: &Plan, unit: &str) -> bool {
    if unit != "compiler" {
        return false;
    }
    plan.intent
        .stages
        .iter()
        .find(|stage| stage.unit == unit)
        .map(|stage| !stage.prerelease)
        .unwrap_or(false)
}

/// Verify and publish every draft in the plan.
///
/// Verification for *all* units happens before *any* unit is published: a
/// failure discovered halfway through leaves published releases that cannot be
/// withdrawn, so the cheap checks are exhausted first.
pub fn finalize(github: &dyn GitHub, plan: &Plan) -> Result<Vec<Finalized>> {
    let units = order_units(plan);

    let mut verified = Vec::new();
    for unit in &units {
        verified.push(verify_unit(github, plan, unit)?);
    }

    let mut finalized = Vec::new();
    for pending in verified {
        let latest = claims_latest(plan, &pending.unit);

        if pending.already_published {
            finalized.push(Finalized {
                unit: pending.unit,
                tag: pending.tag,
                release_id: pending.release_id,
                latest,
                already_published: true,
            });
            continue;
        }

        github
            .publish_release(pending.release_id, latest)
            .with_context(|| format!("publishing the release for '{}'", pending.tag))?;

        finalized.push(Finalized {
            unit: pending.unit,
            tag: pending.tag,
            release_id: pending.release_id,
            latest,
            already_published: false,
        });
    }

    Ok(finalized)
}

struct Verified {
    unit: String,
    tag: String,
    release_id: u64,
    already_published: bool,
}

fn verify_unit(github: &dyn GitHub, plan: &Plan, unit: &str) -> Result<Verified> {
    let tag = plan
        .intent
        .tags
        .iter()
        .find(|tag| tag.unit == unit)
        .with_context(|| format!("the plan declares no tag for unit '{unit}'"))?;

    let release = github.release_by_tag(&tag.name)?.with_context(|| {
        format!("no release exists for '{}'; staging did not complete", tag.name)
    })?;

    // The tag ref, not the release object. GitHub reports `target_commitish` as
    // whatever was requested at creation and stops honouring it once the tag
    // exists, so trusting it here would verify an intention rather than a fact.
    match github.tag_commit(&tag.name)? {
        None => bail!(
            "the tag '{}' does not exist, but its release does; the tag is created in Phase D and \
             must be present before finalization",
            tag.name
        ),
        Some(commit) if commit != plan.intent.subject => bail!(
            "the tag '{}' points at {commit}, not at the subject {}; something moved it, and \
             publishing would finalize a release naming the wrong commit",
            tag.name,
            plan.intent.subject
        ),
        Some(_) => {}
    }

    if !release.draft {
        // Already published by an earlier attempt. Its assets are immutable, so
        // there is nothing left to verify and nothing that could be fixed.
        return Ok(Verified {
            unit: unit.to_string(),
            tag: tag.name.clone(),
            release_id: release.id,
            already_published: true,
        });
    }

    verify_assets(github, plan, release.id, &tag.name)?;

    Ok(Verified {
        unit: unit.to_string(),
        tag: tag.name.clone(),
        release_id: release.id,
        already_published: false,
    })
}

/// Confirm a draft holds exactly the planned assets, each with the bytes that
/// were staged, plus this plan.
fn verify_assets(github: &dyn GitHub, plan: &Plan, release_id: u64, tag: &str) -> Result<()> {
    let attached = github.assets(release_id)?;
    let plan_asset = format!("{PLAN_ASSET_PREFIX}{}.json", &plan.digest()[..16]);

    if !attached.iter().any(|asset| asset.name == plan_asset) {
        bail!(
            "the draft for '{tag}' does not carry this plan ({plan_asset}); it belongs to a \
             different release run"
        );
    }

    // The plan travels as bytes, so compare bytes: a draft carrying a plan with
    // the same digest in its *name* but different content is the case a name
    // check alone would wave through.
    let carried = github.download_asset(release_id, &plan_asset)?;
    if carried != plan.to_canonical_json().into_bytes() {
        bail!("the plan attached to '{tag}' differs from the plan being finalized");
    }

    let mut expected: Vec<String> = vec![plan_asset.clone()];

    if attached.iter().any(|asset| asset.name == SHA256SUMS) {
        expected.push(SHA256SUMS.to_string());
        let sums = github.download_asset(release_id, SHA256SUMS)?;
        let sums = String::from_utf8(sums)
            .with_context(|| format!("{SHA256SUMS} on '{tag}' is not valid UTF-8"))?;

        for line in sums.lines().filter(|line| !line.trim().is_empty()) {
            let (digest, name) = line
                .split_once("  ")
                .with_context(|| format!("malformed {SHA256SUMS} line on '{tag}': {line}"))?;
            expected.push(name.to_string());

            let bytes = github
                .download_asset(release_id, name)
                .with_context(|| format!("reading back '{name}' from '{tag}'"))?;
            let actual = sha256_hex(&bytes);
            if actual != digest {
                bail!(
                    "asset '{name}' on '{tag}' hashes to {} but was staged as {digest}; the bytes \
                     changed between staging and finalization",
                    &actual[..16]
                );
            }
        }
    }

    // Anything else was never planned. After publication it is permanent, so an
    // unexplained asset stops the release rather than shipping.
    let unexpected: Vec<&str> = attached
        .iter()
        .map(|asset| asset.name.as_str())
        .filter(|name| !expected.iter().any(|e| e == name))
        .collect();
    if !unexpected.is_empty() {
        bail!(
            "the draft for '{tag}' carries {} asset(s) the plan does not describe: {}. \
             Publication is permanent, so this must be explained before finalizing",
            unexpected.len(),
            unexpected.join(", ")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        github::StubGitHub,
        intent::{Intent, Stage, Tag},
        plan::SealedPackage,
        staging::{self, Payload},
    };

    const SUBJECT: &str = "abc123";

    fn plan(units: &[(&str, bool)]) -> Plan {
        Plan {
            schema_version: 1,
            intent: Intent {
                schema_version: 1,
                subject: SUBJECT.into(),
                candidate_digest: "cand".into(),
                stages: units
                    .iter()
                    .map(|(unit, prerelease)| Stage {
                        unit: unit.to_string(),
                        version: "1.0.0".into(),
                        prerelease: *prerelease,
                        packages: vec![],
                    })
                    .collect(),
                tags: units
                    .iter()
                    .map(|(unit, _)| Tag {
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

    /// Stage the plan's drafts and create the tags Phase D would have created.
    fn staged(plan: &Plan) -> StubGitHub {
        let github = StubGitHub::new();
        staging::stage(&github, plan, &BTreeMap::new()).unwrap();
        for tag in &plan.intent.tags {
            github.create_tag(&tag.name, SUBJECT).unwrap();
        }
        github
    }

    #[test]
    fn units_publish_sdk_then_templates_then_compiler() {
        let plan = plan(&[("compiler", false), ("templates", false), ("sdk", false)]);
        let github = staged(&plan);

        let finalized = finalize(&github, &plan).unwrap();
        let order: Vec<&str> = finalized.iter().map(|f| f.unit.as_str()).collect();
        assert_eq!(order, ["sdk", "templates", "compiler"]);
        assert!(finalized.iter().all(|f| !f.already_published));
    }

    #[test]
    fn only_a_stable_compiler_release_becomes_latest() {
        let plan = plan(&[("sdk", false), ("templates", false), ("compiler", false)]);
        let github = staged(&plan);
        finalize(&github, &plan).unwrap();

        assert_eq!(github.is_latest("compiler/v1.0.0"), Some(true));
        assert_eq!(github.is_latest("sdk/v1.0.0"), Some(false));
        assert_eq!(github.is_latest("templates/v1.0.0"), Some(false));
    }

    #[test]
    fn a_compiler_prerelease_does_not_become_latest() {
        let plan = plan(&[("compiler", true)]);
        let github = staged(&plan);
        finalize(&github, &plan).unwrap();

        assert_eq!(
            github.is_latest("compiler/v1.0.0"),
            Some(false),
            "a prerelease must never take the latest slot"
        );
    }

    #[test]
    fn a_tag_pointing_somewhere_else_stops_the_release() {
        let plan = plan(&[("sdk", false)]);
        let github = StubGitHub::new();
        staging::stage(&github, &plan, &BTreeMap::new()).unwrap();
        github.create_tag("sdk/v1.0.0", "a-different-commit").unwrap();

        let err = finalize(&github, &plan).unwrap_err().to_string();
        assert!(err.contains("not at the subject"), "{err}");
        assert!(!github.is_published("sdk/v1.0.0"), "nothing may be published after a failure");
    }

    #[test]
    fn a_missing_tag_stops_the_release() {
        let plan = plan(&[("sdk", false)]);
        let github = StubGitHub::new();
        staging::stage(&github, &plan, &BTreeMap::new()).unwrap();

        let err = finalize(&github, &plan).unwrap_err().to_string();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn an_asset_whose_bytes_changed_stops_the_release() {
        let plan = plan(&[("templates", false)]);
        let dir = std::env::temp_dir().join(format!("finalize-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let asset = dir.join("templates.tar.gz");
        std::fs::write(&asset, b"bundle-bytes").unwrap();

        let mut payload = Payload::default();
        payload.add("templates.tar.gz", &asset);
        let payloads: BTreeMap<String, Payload> =
            [("templates".to_string(), payload)].into_iter().collect();

        // Staging verifies on upload, so the corruption has to happen after it:
        // the point of the check is that staging and finalization are different
        // jobs, an approval apart.
        let github = StubGitHub::new();
        staging::stage(&github, &plan, &payloads).unwrap();
        github.create_tag("templates/v1.0.0", SUBJECT).unwrap();
        github.replace_asset_bytes("templates.tar.gz", b"tampered");

        let err = finalize(&github, &plan).unwrap_err().to_string();
        assert!(err.contains("bytes changed"), "{err}");
        assert!(!github.is_published("templates/v1.0.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unplanned_asset_stops_the_release() {
        let plan = plan(&[("sdk", false)]);
        let github = staged(&plan);
        let release = github.release_by_tag("sdk/v1.0.0").unwrap().unwrap();
        github.upload_asset(release.id, "surprise.bin", b"?").unwrap();

        let err = finalize(&github, &plan).unwrap_err().to_string();
        assert!(err.contains("does not describe"), "{err}");
        assert!(err.contains("surprise.bin"), "{err}");
    }

    #[test]
    fn a_draft_carrying_a_different_plan_stops_the_release() {
        let plan = plan(&[("sdk", false)]);
        let github = staged(&plan);

        // A plan with the same units but a different subject: a different run.
        let mut other = plan.clone();
        other.intent.subject = "def456".into();

        let err = finalize(&github, &other).unwrap_err().to_string();
        assert!(
            err.contains("different release run") || err.contains("not at the subject"),
            "{err}"
        );
    }

    #[test]
    fn finalizing_again_is_a_resume_rather_than_an_error() {
        let plan = plan(&[("sdk", false), ("compiler", false)]);
        let github = staged(&plan);

        finalize(&github, &plan).unwrap();
        let second = finalize(&github, &plan).unwrap();

        assert!(
            second.iter().all(|f| f.already_published),
            "a second finalization must recognise its own work: {second:?}"
        );
        assert!(github.is_published("sdk/v1.0.0"));
        assert!(github.is_published("compiler/v1.0.0"));
    }

    #[test]
    fn a_unit_the_order_does_not_know_is_still_released() {
        let plan = plan(&[("sdk", false), ("docs", false)]);
        let ordered = order_units(&plan);
        assert_eq!(ordered, ["sdk", "docs"], "an unknown unit must not be silently dropped");
    }
}
