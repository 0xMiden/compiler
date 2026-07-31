//! Deciding what still needs to be published.
//!
//! Every publication attempt starts here, including the first one. The executor
//! queries live registry state, classifies each planned crate, and invokes
//! Cargo with only the crates that remain. A resume is therefore not a separate
//! code path: publishing 33 crates from scratch and publishing the 21 that a
//! failed attempt left behind are the same operation with different inputs.
//!
//! The classification is deliberately conservative. Anything ambiguous stops
//! the release rather than guessing, because the alternative is publishing over
//! a version someone else owns or skipping one that was never really published.

use std::collections::BTreeSet;

use anyhow::Result;

use crate::{order, registry::client::IndexClient, workspace::Workspace};

/// What the executor should do about one planned crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Not present in the registry; publish it.
    Publish,
    /// Present, resolvable, and matching what was planned; skip it.
    Skip,
    /// Present but not usable as planned. Never publish over this.
    Conflict(Conflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// The version exists with different content than was planned.
    ChecksumMismatch { expected: String, found: String },
    /// The version exists but is yanked, so dependents cannot resolve it.
    /// This is a conflict even when the checksum matches: the version can
    /// never be republished, so the release cannot complete as planned.
    Yanked,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChecksumMismatch { expected, found } => write!(
                f,
                "already published with a different checksum (expected {expected}, found {found})"
            ),
            Self::Yanked => f.write_str(
                "already published but yanked; it can never be republished, so this release \
                 cannot complete as planned",
            ),
        }
    }
}

/// One planned publication.
#[derive(Debug, Clone)]
pub struct Planned {
    pub name: String,
    pub version: String,
    /// The digest of the prepared `.crate`, when one has been sealed. Without
    /// it, presence alone is taken as a match.
    pub expected_cksum: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub name: String,
    pub version: String,
    pub disposition: Disposition,
}

#[derive(Debug)]
pub struct Reconciliation {
    pub outcomes: Vec<Outcome>,
    /// Crates still to publish, in dependency order.
    pub to_publish: Vec<String>,
}

impl Reconciliation {
    pub fn conflicts(&self) -> impl Iterator<Item = &Outcome> {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.disposition, Disposition::Conflict(_)))
    }

    /// Whether the executor may proceed to publish.
    pub fn is_publishable(&self) -> bool {
        self.conflicts().next().is_none()
    }

    /// Whether everything planned is already published, so Cargo must not be
    /// invoked at all. `cargo publish` errors when every `-p` package already
    /// exists, so this case has to be recognized rather than attempted.
    pub fn is_complete(&self) -> bool {
        self.is_publishable() && self.to_publish.is_empty()
    }
}

/// Classify each planned crate against live registry state.
pub fn reconcile(
    ws: &Workspace,
    index: &dyn IndexClient,
    planned: &[Planned],
) -> Result<Reconciliation> {
    let mut outcomes = Vec::with_capacity(planned.len());
    let mut publish = BTreeSet::new();

    for item in planned {
        // A lookup failure propagates: an unreachable registry must never be
        // read as "nothing is published", which would republish the world.
        let published = index.versions(&item.name)?;
        let existing = published.iter().find(|entry| entry.vers == item.version);

        let disposition = match existing {
            None => {
                publish.insert(item.name.clone());
                Disposition::Publish
            }
            Some(entry) if entry.yanked => Disposition::Conflict(Conflict::Yanked),
            Some(entry) => match &item.expected_cksum {
                Some(expected) if *expected != entry.cksum => {
                    Disposition::Conflict(Conflict::ChecksumMismatch {
                        expected: expected.clone(),
                        found: entry.cksum.clone(),
                    })
                }
                _ => Disposition::Skip,
            },
        };

        outcomes.push(Outcome {
            name: item.name.clone(),
            version: item.version.clone(),
            disposition,
        });
    }

    let to_publish = order::topological(ws, &publish)?;
    Ok(Reconciliation {
        outcomes,
        to_publish,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        registry::client::StubIndex,
        workspace::{EdgeKind, Package, Workspace},
    };

    fn workspace() -> Workspace {
        let packages = [("leaf", vec![]), ("mid", vec!["leaf"]), ("root", vec!["mid"])];
        Workspace {
            root: std::path::PathBuf::from("/tmp"),
            packages: packages
                .into_iter()
                .map(|(name, deps)| {
                    (
                        name.to_string(),
                        Package {
                            version: "1.0.0".into(),
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

    fn planned(names: &[&str]) -> Vec<Planned> {
        names
            .iter()
            .map(|name| Planned {
                name: name.to_string(),
                version: "1.0.0".into(),
                expected_cksum: Some("expected".into()),
            })
            .collect()
    }

    #[test]
    fn a_first_attempt_publishes_everything_in_dependency_order() {
        let result =
            reconcile(&workspace(), &StubIndex::new(), &planned(&["root", "mid", "leaf"])).unwrap();
        assert!(result.is_publishable());
        assert!(!result.is_complete());
        assert_eq!(result.to_publish, ["leaf", "mid", "root"]);
    }

    #[test]
    fn a_resume_publishes_only_what_is_missing_and_keeps_the_order() {
        // A previous attempt got through `leaf` before dying.
        let index = StubIndex::new().publish("leaf", "1.0.0", "expected", false);
        let result = reconcile(&workspace(), &index, &planned(&["root", "mid", "leaf"])).unwrap();

        assert!(result.is_publishable());
        assert_eq!(result.to_publish, ["mid", "root"]);
        let leaf = result.outcomes.iter().find(|o| o.name == "leaf").unwrap();
        assert_eq!(leaf.disposition, Disposition::Skip);
    }

    #[test]
    fn a_completed_release_asks_for_no_cargo_invocation() {
        let index = StubIndex::new()
            .publish("leaf", "1.0.0", "expected", false)
            .publish("mid", "1.0.0", "expected", false)
            .publish("root", "1.0.0", "expected", false);
        let result = reconcile(&workspace(), &index, &planned(&["root", "mid", "leaf"])).unwrap();

        assert!(result.is_complete());
        assert!(result.to_publish.is_empty());
    }

    #[test]
    fn a_different_checksum_is_a_conflict() {
        let index = StubIndex::new().publish("leaf", "1.0.0", "something-else", false);
        let result = reconcile(&workspace(), &index, &planned(&["leaf"])).unwrap();

        assert!(!result.is_publishable());
        assert_eq!(result.conflicts().count(), 1);
        assert!(matches!(
            result.outcomes[0].disposition,
            Disposition::Conflict(Conflict::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn a_yanked_version_is_a_conflict_even_when_the_checksum_matches() {
        let index = StubIndex::new().publish("leaf", "1.0.0", "expected", true);
        let result = reconcile(&workspace(), &index, &planned(&["leaf"])).unwrap();

        assert!(!result.is_publishable());
        assert_eq!(result.outcomes[0].disposition, Disposition::Conflict(Conflict::Yanked));
    }

    #[test]
    fn an_unrelated_version_of_the_same_crate_does_not_count_as_published() {
        let index = StubIndex::new().publish("leaf", "0.9.0", "expected", false);
        let result = reconcile(&workspace(), &index, &planned(&["leaf"])).unwrap();
        assert_eq!(result.to_publish, ["leaf"]);
    }

    #[test]
    fn an_unreachable_registry_is_an_error_not_an_empty_registry() {
        let result = reconcile(&workspace(), &StubIndex::new().unreachable(), &planned(&["leaf"]));
        assert!(result.is_err(), "an unreachable registry must not look like an empty one");
    }

    #[test]
    fn presence_alone_suffices_when_no_checksum_was_sealed() {
        let index = StubIndex::new().publish("leaf", "1.0.0", "whatever", false);
        let unsealed = vec![Planned {
            name: "leaf".into(),
            version: "1.0.0".into(),
            expected_cksum: None,
        }];
        let result = reconcile(&workspace(), &index, &unsealed).unwrap();
        assert_eq!(result.outcomes[0].disposition, Disposition::Skip);
    }
}
