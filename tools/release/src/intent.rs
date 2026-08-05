//! The release intent: a canonical, deterministic description of what a
//! release would do, before anything is built.
//!
//! The intent is a pure function of the subject commit, the committed
//! candidate declaration, workspace metadata, and release policy. It contains
//! no digests, because nothing has been built yet -- sealing those into a plan
//! is a separate step against prepared artifacts.
//!
//! Determinism is the property that matters. The same inputs must produce
//! byte-identical output, so that a maintainer reviewing an intent and the
//! executor acting on one are demonstrably looking at the same release. That
//! rules out timestamps, absolute paths, and map iteration order, and it is why
//! serialization goes through ordered collections.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    candidate::{Candidate, UnitDeclaration},
    config::{Config, Unit},
    order,
    workspace::Workspace,
};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Intent {
    pub schema_version: u32,
    /// The commit whose source would be packaged.
    pub subject: String,
    /// Digest of the candidate declaration this intent was derived from, so a
    /// later step can prove it is acting on the reviewed scope.
    pub candidate_digest: String,
    /// Publication stages, in the order they must run.
    pub stages: Vec<Stage>,
    /// Tags this release would create, in creation order.
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Stage {
    pub unit: String,
    pub version: String,
    pub prerelease: bool,
    /// Crates to publish, in dependency order. This is the `-p` list.
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Tag {
    pub unit: String,
    pub name: String,
}

impl Intent {
    /// Canonical JSON. Identical inputs produce identical bytes.
    pub fn to_canonical_json(&self) -> String {
        // `serde_json` preserves struct field order and `BTreeMap` ordering, and
        // the intent contains no maps with unstable iteration, so pretty-printing
        // is already canonical.
        serde_json::to_string_pretty(self).expect("intent is serializable")
    }

    pub fn digest(&self) -> String {
        crate::registry::sha256_hex(self.to_canonical_json().as_bytes())
    }
}

/// Units publish in this order when several are selected. SDK crates are
/// depended on by compiler crates, so the SDK must be resolvable first.
const UNIT_ORDER: [&str; 3] = ["sdk", "templates", "compiler"];

/// Build an intent from the reviewed candidate.
pub fn generate(
    ws: &Workspace,
    config: &Config,
    candidate: &Candidate,
    subject: &str,
) -> Result<Intent> {
    let problems = crate::candidate::validate(ws, config, candidate);
    if !problems.is_empty() {
        bail!(
            "the release candidate is not valid:\n{}",
            problems.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
        );
    }

    let selected: BTreeMap<&str, &UnitDeclaration> = candidate
        .units
        .iter()
        .filter_map(|unit| candidate.declaration(unit).map(|d| (unit.as_str(), d)))
        .collect();

    let mut stages = Vec::new();
    let mut tags = Vec::new();

    for unit in UNIT_ORDER {
        let Some(declaration) = selected.get(unit) else {
            continue;
        };

        let packages = match unit {
            "compiler" => package_order(ws, config, Unit::Compiler)?,
            "sdk" => package_order(ws, config, Unit::Sdk)?,
            // Templates publish no crates; the unit exists for its tag and
            // release artifacts.
            _ => Vec::new(),
        };

        stages.push(Stage {
            unit: unit.to_string(),
            version: declaration.version.to_string(),
            prerelease: declaration.prerelease,
            packages,
        });
        tags.push(Tag {
            unit: unit.to_string(),
            name: declaration.tag.clone(),
        });
    }

    // Every selected unit must be recognized, or a typo would silently produce
    // a release that omits it.
    if stages.len() != selected.len() {
        let known = UNIT_ORDER.join(", ");
        let unknown: Vec<&str> =
            selected.keys().filter(|unit| !UNIT_ORDER.contains(unit)).copied().collect();
        bail!("unknown unit(s): {}; known units are {known}", unknown.join(", "));
    }

    Ok(Intent {
        schema_version: SCHEMA_VERSION,
        subject: subject.to_string(),
        candidate_digest: candidate_digest(candidate)?,
        stages,
        tags,
    })
}

fn package_order(ws: &Workspace, config: &Config, unit: Unit) -> Result<Vec<String>> {
    let selected = config.packages_in(unit).map(|p| p.name.clone()).collect();
    order::topological(ws, &selected)
}

/// Digest the candidate's canonical form, not its file bytes, so that
/// reformatting a comment does not change the identity of the release.
fn candidate_digest(candidate: &Candidate) -> Result<String> {
    let canonical = serde_json::to_string(candidate)?;
    Ok(crate::registry::sha256_hex(canonical.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        candidate::UnitDeclaration,
        config::Config,
        workspace::{EdgeKind, Package, Workspace},
    };

    fn workspace() -> Workspace {
        let packages = [
            ("sdk-leaf", vec![]),
            ("sdk-root", vec!["sdk-leaf"]),
            ("comp-leaf", vec![]),
            ("comp-root", vec!["comp-leaf", "sdk-root"]),
        ];
        Workspace {
            root: std::path::PathBuf::from("/tmp"),
            packages: packages
                .into_iter()
                .map(|(name, deps)| {
                    (
                        name.to_string(),
                        Package {
                            version: if name.starts_with("sdk") {
                                "0.14.0"
                            } else {
                                "0.10.0"
                            }
                            .to_string(),
                            manifest_path: std::path::PathBuf::from("/tmp/Cargo.toml"),
                            local_deps: deps
                                .into_iter()
                                .map(|d: &str| (d.to_string(), EdgeKind::Required))
                                .collect(),
                            publishable: true,
                        },
                    )
                })
                .collect(),
        }
    }

    fn config() -> Config {
        toml::from_str(
            r#"
schema-version = 1

[units.compiler]
tag = "v{version}"
changelog = "CHANGELOG.md"

[units.sdk]
tag = "sdk/v{version}"
changelog = "sdk/CHANGELOG.md"

[[packages]]
name = "comp-leaf"
unit = "compiler"
publish = true
version-source = "workspace"

[[packages]]
name = "comp-root"
unit = "compiler"
publish = true
version-source = "workspace"

[[packages]]
name = "sdk-leaf"
unit = "sdk"
publish = true
version-source = "sdk"

[[packages]]
name = "sdk-root"
unit = "sdk"
publish = true
version-source = "sdk"
"#,
        )
        .unwrap()
    }

    fn candidate(units: &[&str]) -> Candidate {
        let mut declarations = BTreeMap::new();
        if units.contains(&"compiler") {
            declarations.insert(
                "compiler".to_string(),
                UnitDeclaration {
                    version: semver::Version::parse("0.10.0").unwrap(),
                    tag: "v0.10.0".into(),
                    prerelease: false,
                },
            );
        }
        if units.contains(&"sdk") {
            declarations.insert(
                "sdk".to_string(),
                UnitDeclaration {
                    version: semver::Version::parse("0.14.0").unwrap(),
                    tag: "sdk/v0.14.0".into(),
                    prerelease: false,
                },
            );
        }
        Candidate {
            schema_version: 1,
            units: units.iter().map(|u| u.to_string()).collect(),
            declarations,
        }
    }

    #[test]
    fn sdk_publishes_before_compiler() {
        let intent =
            generate(&workspace(), &config(), &candidate(&["compiler", "sdk"]), "abc123").unwrap();
        let units: Vec<&str> = intent.stages.iter().map(|s| s.unit.as_str()).collect();
        assert_eq!(units, ["sdk", "compiler"], "compiler crates depend on sdk crates");
    }

    #[test]
    fn packages_within_a_stage_are_in_dependency_order() {
        let intent = generate(&workspace(), &config(), &candidate(&["sdk"]), "abc123").unwrap();
        assert_eq!(intent.stages[0].packages, ["sdk-leaf", "sdk-root"]);
    }

    #[test]
    fn a_single_unit_release_contains_only_that_unit() {
        let intent = generate(&workspace(), &config(), &candidate(&["sdk"]), "abc123").unwrap();
        assert_eq!(intent.stages.len(), 1);
        assert_eq!(
            intent.tags,
            [Tag {
                unit: "sdk".into(),
                name: "sdk/v0.14.0".into()
            }]
        );
    }

    #[test]
    fn intents_are_byte_identical_for_identical_inputs() {
        let (ws, config, candidate) = (workspace(), config(), candidate(&["compiler", "sdk"]));
        let first = generate(&ws, &config, &candidate, "abc123").unwrap();
        for _ in 0..8 {
            let again = generate(&ws, &config, &candidate, "abc123").unwrap();
            assert_eq!(first.to_canonical_json(), again.to_canonical_json());
            assert_eq!(first.digest(), again.digest());
        }
    }

    #[test]
    fn a_different_subject_yields_a_different_intent() {
        let (ws, config, candidate) = (workspace(), config(), candidate(&["sdk"]));
        let first = generate(&ws, &config, &candidate, "abc123").unwrap();
        let second = generate(&ws, &config, &candidate, "def456").unwrap();
        assert_ne!(first.digest(), second.digest());
    }

    #[test]
    fn a_candidate_disagreeing_with_the_manifests_is_rejected() {
        let mut candidate = candidate(&["sdk"]);
        candidate.declarations.get_mut("sdk").unwrap().version =
            semver::Version::parse("9.9.9").unwrap();
        candidate.declarations.get_mut("sdk").unwrap().tag = "sdk/v9.9.9".into();

        let err = generate(&workspace(), &config(), &candidate, "abc123").unwrap_err().to_string();
        assert!(err.contains("but 'sdk-leaf' is at 0.14.0"), "{err}");
    }

    #[test]
    fn an_unknown_unit_is_rejected() {
        let mut candidate = candidate(&["sdk"]);
        candidate.units.push("bogus".into());
        candidate.declarations.insert(
            "bogus".to_string(),
            UnitDeclaration {
                version: semver::Version::parse("1.0.0").unwrap(),
                tag: "bogus/v1.0.0".into(),
                prerelease: false,
            },
        );

        let err = generate(&workspace(), &config(), &candidate, "abc123").unwrap_err().to_string();
        assert!(err.contains("not defined in .release/config.toml"), "{err}");
    }
}
