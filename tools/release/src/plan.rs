//! The sealed release plan.
//!
//! An intent says what a release *would* do; a plan records what was actually
//! built. Sealing happens once, against prepared artifacts, and binds the
//! reviewed scope to the exact bytes that will be published.
//!
//! The digests are the point. Until a plan is sealed, reconciliation can only
//! ask "does this version exist?", which cannot distinguish a version this
//! release published from one somebody else published with different content.
//! With digests it can, and a mismatch becomes a conflict that stops the
//! release instead of a silent skip.
//!
//! A sealed plan is never edited. Anything that would change it -- a different
//! subject, a different scope, different bytes -- requires a new intent and a
//! new plan.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::{closure::Closure, intent::Intent, reconcile::Planned};

pub const SCHEMA_VERSION: u32 = 1;

/// One package as it was actually built.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct SealedPackage {
    pub name: String,
    pub version: String,
    /// SHA-256 of the `.crate` archive, which is what crates.io records as the
    /// version's checksum.
    pub digest: String,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Plan {
    pub schema_version: u32,
    /// The intent this plan was sealed from, carried whole so the plan is
    /// self-contained: scope, subject, stages, and tags travel with the digests.
    pub intent: Intent,
    /// Built packages, ordered as the intent orders them.
    pub packages: Vec<SealedPackage>,
}

impl Plan {
    pub fn to_canonical_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("plan is serializable")
    }

    pub fn digest(&self) -> String {
        crate::registry::sha256_hex(self.to_canonical_json().as_bytes())
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let plan: Self = serde_json::from_str(&text)?;
        if plan.schema_version != SCHEMA_VERSION {
            bail!(
                "release plan schema version {} is not supported (expected {})",
                plan.schema_version,
                SCHEMA_VERSION
            );
        }
        Ok(plan)
    }

    /// The publication set for one unit, with sealed digests attached.
    ///
    /// This is what makes reconciliation able to tell "already published by this
    /// release" apart from "published by someone else".
    pub fn planned_for(&self, unit: &str) -> Vec<Planned> {
        let digests: BTreeMap<&str, &SealedPackage> =
            self.packages.iter().map(|p| (p.name.as_str(), p)).collect();

        self.intent
            .stages
            .iter()
            .filter(|stage| stage.unit == unit)
            .flat_map(|stage| stage.packages.iter())
            .filter_map(|name| {
                digests.get(name.as_str()).map(|sealed| Planned {
                    name: sealed.name.clone(),
                    version: sealed.version.clone(),
                    expected_cksum: Some(sealed.digest.clone()),
                })
            })
            .collect()
    }

    /// Every planned package across all units, in stage order.
    pub fn planned(&self) -> Vec<Planned> {
        self.intent
            .stages
            .iter()
            .flat_map(|stage| self.planned_for(&stage.unit))
            .collect()
    }
}

/// Bind an intent to the artifacts built from it.
///
/// The cross-checks are what stop a plan from being sealed against the wrong
/// build: every package the intent names must have been built, at the version
/// the intent declared, and nothing else may have been.
pub fn seal(intent: &Intent, closure: &Closure) -> Result<Plan> {
    let built: BTreeMap<&str, &crate::closure::PackagedCrate> =
        closure.crates.iter().map(|c| (c.name.as_str(), c)).collect();

    let mut packages = Vec::new();
    let mut problems = Vec::new();

    for stage in &intent.stages {
        for name in &stage.packages {
            let Some(crate_) = built.get(name.as_str()) else {
                problems.push(format!("'{name}' is in the intent but was not built"));
                continue;
            };
            if crate_.version != stage.version {
                problems.push(format!(
                    "'{name}' was built at {} but the intent declares {} for unit '{}'",
                    crate_.version, stage.version, stage.unit
                ));
                continue;
            }
            packages.push(SealedPackage {
                name: crate_.name.clone(),
                version: crate_.version.clone(),
                digest: crate_.digest.clone(),
                size: crate_.size,
            });
        }
    }

    let planned: std::collections::BTreeSet<&str> = intent
        .stages
        .iter()
        .flat_map(|s| s.packages.iter())
        .map(String::as_str)
        .collect();
    for crate_ in &closure.crates {
        if !planned.contains(crate_.name.as_str()) {
            problems.push(format!(
                "'{}' was built but is not in the intent; the plan would seal bytes nobody \
                 reviewed",
                crate_.name
            ));
        }
    }

    if !problems.is_empty() {
        bail!(
            "cannot seal the plan:\n{}",
            problems.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
        );
    }

    Ok(Plan {
        schema_version: SCHEMA_VERSION,
        intent: intent.clone(),
        packages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        closure::PackagedCrate,
        intent::{Stage, Tag},
    };

    fn intent(packages: &[&str], version: &str) -> Intent {
        Intent {
            schema_version: 1,
            subject: "abc123".into(),
            candidate_digest: "cand".into(),
            stages: vec![Stage {
                unit: "sdk".into(),
                version: version.into(),
                prerelease: false,
                packages: packages.iter().map(|p| p.to_string()).collect(),
            }],
            tags: vec![Tag {
                unit: "sdk".into(),
                name: format!("sdk/v{version}"),
            }],
        }
    }

    fn closure(entries: &[(&str, &str, &str)]) -> Closure {
        Closure {
            crates: entries
                .iter()
                .map(|(name, version, digest)| PackagedCrate {
                    name: name.to_string(),
                    version: version.to_string(),
                    digest: digest.to_string(),
                    size: 100,
                })
                .collect(),
        }
    }

    #[test]
    fn sealing_attaches_digests_in_stage_order() {
        let plan = seal(
            &intent(&["leaf", "root"], "1.0.0"),
            &closure(&[("root", "1.0.0", "d-root"), ("leaf", "1.0.0", "d-leaf")]),
        )
        .unwrap();

        assert_eq!(plan.packages.len(), 2);
        assert_eq!(plan.packages[0].name, "leaf", "stage order wins, not build order");
        assert_eq!(plan.packages[0].digest, "d-leaf");
    }

    #[test]
    fn a_package_the_intent_names_but_nobody_built_blocks_sealing() {
        let err = seal(&intent(&["leaf", "root"], "1.0.0"), &closure(&[("leaf", "1.0.0", "d")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("'root' is in the intent but was not built"), "{err}");
    }

    #[test]
    fn a_package_built_but_not_planned_blocks_sealing() {
        let err = seal(
            &intent(&["leaf"], "1.0.0"),
            &closure(&[("leaf", "1.0.0", "d"), ("stowaway", "1.0.0", "d")]),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("stowaway"), "{err}");
        assert!(err.contains("nobody reviewed"), "{err}");
    }

    #[test]
    fn a_version_mismatch_between_intent_and_build_blocks_sealing() {
        let err = seal(&intent(&["leaf"], "1.0.0"), &closure(&[("leaf", "9.9.9", "d")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("built at 9.9.9"), "{err}");
    }

    #[test]
    fn planned_packages_carry_their_sealed_digest() {
        let plan =
            seal(&intent(&["leaf"], "1.0.0"), &closure(&[("leaf", "1.0.0", "sealed")])).unwrap();
        let planned = plan.planned();

        assert_eq!(planned.len(), 1);
        assert_eq!(
            planned[0].expected_cksum.as_deref(),
            Some("sealed"),
            "without this, reconciliation cannot tell our version from someone else's"
        );
    }

    #[test]
    fn plans_are_deterministic() {
        let (intent, closure) = (intent(&["leaf"], "1.0.0"), closure(&[("leaf", "1.0.0", "d")]));
        let first = seal(&intent, &closure).unwrap();
        for _ in 0..8 {
            assert_eq!(seal(&intent, &closure).unwrap().digest(), first.digest());
        }
    }

    #[test]
    fn plans_round_trip_through_json() {
        let plan = seal(&intent(&["leaf"], "1.0.0"), &closure(&[("leaf", "1.0.0", "d")])).unwrap();
        let restored: Plan = serde_json::from_str(&plan.to_canonical_json()).unwrap();
        assert_eq!(restored, plan);
        assert_eq!(restored.digest(), plan.digest());
    }
}
