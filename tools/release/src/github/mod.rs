//! GitHub operations: tags, draft releases, and assets.
//!
//! The ordering these support is the heart of §14.4: everything reversible
//! happens first. A draft is created and fully populated while it can still be
//! deleted, the tag is created only after approval, and the draft is published
//! last, at which point it becomes immutable.
//!
//! Two behaviours are specified rather than assumed, because getting either
//! wrong is unrecoverable:
//!
//! A draft release does not reserve its tag name. The tag simply does not exist
//! until something creates it, so between creating a draft and publishing it,
//! anything could create that tag elsewhere. GitHub documents `target_commitish`
//! as *unused if the tag already exists*, so publishing would then silently
//! adopt the wrong commit — and the automatic immutable-release attestation
//! would bind that mistake permanently. Tag creation therefore goes through an
//! endpoint that fails on an existing ref, and a conflict is compared against
//! the expected commit rather than assumed benign.
//!
//! Tags cannot be deleted once the ruleset protects them, so a tag created at
//! the wrong commit burns that version. Detection is all that is available.

pub mod rest;
#[cfg(test)]
pub mod stub_server;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// A release as GitHub reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub id: u64,
    pub tag: String,
    pub draft: bool,
    pub prerelease: bool,
    pub target_commitish: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: usize,
    pub digest: String,
}

/// What a caller needs from GitHub. Kept narrow deliberately: every method here
/// is one the release flow actually performs, so an implementation cannot drift
/// into speculative surface.
pub trait GitHub: Send + Sync {
    /// Create a tag ref, failing if it already exists.
    fn create_tag(&self, tag: &str, commit: &str) -> Result<()>;

    /// The commit a tag points at, or `None` if the tag does not exist.
    fn tag_commit(&self, tag: &str) -> Result<Option<String>>;

    /// Create a draft release for a tag that need not exist yet.
    fn create_draft(&self, tag: &str, commit: &str, prerelease: bool) -> Result<Release>;

    fn release_by_tag(&self, tag: &str) -> Result<Option<Release>>;

    fn upload_asset(&self, release: u64, name: &str, bytes: &[u8]) -> Result<Asset>;

    fn assets(&self, release: u64) -> Result<Vec<Asset>>;

    /// Download an asset back, so it can be verified while still mutable.
    fn download_asset(&self, release: u64, name: &str) -> Result<Vec<u8>>;

    fn delete_release(&self, release: u64) -> Result<()>;

    /// Publish a draft. After this the release and its assets are immutable.
    fn publish_release(&self, release: u64) -> Result<Release>;
}

/// Create a tag, treating an existing one as a hard failure unless it already
/// points where we intend.
///
/// This is the fail-closed path §14.4 requires. A tag that exists at the right
/// commit is a resume; at any other commit it is an incident, because the
/// version can no longer be released as planned and the tag cannot be removed.
pub fn create_tag_idempotent(github: &dyn GitHub, tag: &str, commit: &str) -> Result<TagOutcome> {
    match github.create_tag(tag, commit) {
        Ok(()) => Ok(TagOutcome::Created),
        Err(_) => match github.tag_commit(tag)? {
            Some(existing) if existing == commit => Ok(TagOutcome::AlreadyCorrect),
            Some(existing) => bail!(
                "tag '{tag}' already exists at {existing}, not {commit}; it cannot be moved or \
                 deleted, so this version must be abandoned and a new one released"
            ),
            None => bail!("failed to create tag '{tag}' and it does not exist"),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagOutcome {
    Created,
    /// Left by an earlier attempt at the same commit.
    AlreadyCorrect,
}

/// Upload assets and read every one back, verifying size and digest while the
/// release is still a draft and therefore still fixable.
pub fn upload_and_verify(
    github: &dyn GitHub,
    release: u64,
    assets: &[(String, Vec<u8>)],
) -> Result<Vec<Asset>> {
    let mut uploaded = Vec::new();
    for (name, bytes) in assets {
        github
            .upload_asset(release, name, bytes)
            .with_context(|| format!("uploading '{name}'"))?;
    }

    for (name, bytes) in assets {
        let fetched = github
            .download_asset(release, name)
            .with_context(|| format!("reading back '{name}'"))?;
        if fetched != *bytes {
            bail!(
                "'{name}' does not match what was uploaded ({} bytes sent, {} bytes returned); \
                 the release must not be published",
                bytes.len(),
                fetched.len()
            );
        }
        uploaded.push(Asset {
            name: name.clone(),
            size: bytes.len(),
            digest: crate::registry::sha256_hex(bytes),
        });
    }
    Ok(uploaded)
}

/// An in-memory GitHub, for tests.
#[derive(Debug, Default)]
pub struct StubGitHub {
    state: std::sync::Mutex<StubState>,
}

#[derive(Debug, Default)]
struct StubState {
    next_id: u64,
    tags: BTreeMap<String, String>,
    releases: BTreeMap<u64, Release>,
    assets: BTreeMap<u64, BTreeMap<String, Vec<u8>>>,
    /// Assets that should come back corrupted, to exercise verification.
    corrupt: BTreeMap<String, Vec<u8>>,
}

impl StubGitHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-create a tag, as a squatter or an earlier attempt would have.
    pub fn with_tag(self, tag: &str, commit: &str) -> Self {
        self.state.lock().unwrap().tags.insert(tag.to_string(), commit.to_string());
        self
    }

    /// Make an asset come back with different bytes than were uploaded.
    pub fn corrupting(self, name: &str, replacement: &[u8]) -> Self {
        self.state
            .lock()
            .unwrap()
            .corrupt
            .insert(name.to_string(), replacement.to_vec());
        self
    }

    pub fn is_published(&self, tag: &str) -> bool {
        self.state.lock().unwrap().releases.values().any(|r| r.tag == tag && !r.draft)
    }
}

impl GitHub for StubGitHub {
    fn create_tag(&self, tag: &str, commit: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.tags.contains_key(tag) {
            bail!("reference already exists");
        }
        state.tags.insert(tag.to_string(), commit.to_string());
        Ok(())
    }

    fn tag_commit(&self, tag: &str) -> Result<Option<String>> {
        Ok(self.state.lock().unwrap().tags.get(tag).cloned())
    }

    fn create_draft(&self, tag: &str, commit: &str, prerelease: bool) -> Result<Release> {
        let mut state = self.state.lock().unwrap();
        state.next_id += 1;
        let release = Release {
            id: state.next_id,
            tag: tag.to_string(),
            draft: true,
            prerelease,
            target_commitish: commit.to_string(),
        };
        state.releases.insert(release.id, release.clone());
        Ok(release)
    }

    fn release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        Ok(self.state.lock().unwrap().releases.values().find(|r| r.tag == tag).cloned())
    }

    fn upload_asset(&self, release: u64, name: &str, bytes: &[u8]) -> Result<Asset> {
        let mut state = self.state.lock().unwrap();
        if !state.releases.contains_key(&release) {
            bail!("no such release {release}");
        }
        state
            .assets
            .entry(release)
            .or_default()
            .insert(name.to_string(), bytes.to_vec());
        Ok(Asset {
            name: name.to_string(),
            size: bytes.len(),
            digest: crate::registry::sha256_hex(bytes),
        })
    }

    fn assets(&self, release: u64) -> Result<Vec<Asset>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .assets
            .get(&release)
            .map(|assets| {
                assets
                    .iter()
                    .map(|(name, bytes)| Asset {
                        name: name.clone(),
                        size: bytes.len(),
                        digest: crate::registry::sha256_hex(bytes),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn download_asset(&self, release: u64, name: &str) -> Result<Vec<u8>> {
        let state = self.state.lock().unwrap();
        if let Some(corrupt) = state.corrupt.get(name) {
            return Ok(corrupt.clone());
        }
        state
            .assets
            .get(&release)
            .and_then(|assets| assets.get(name))
            .cloned()
            .with_context(|| format!("no asset '{name}' on release {release}"))
    }

    fn delete_release(&self, release: u64) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        match state.releases.get(&release) {
            Some(existing) if !existing.draft => {
                bail!("release {release} is published and will not be deleted")
            }
            _ => {
                state.releases.remove(&release);
                state.assets.remove(&release);
                Ok(())
            }
        }
    }

    fn publish_release(&self, release: u64) -> Result<Release> {
        let mut state = self.state.lock().unwrap();
        let Some(existing) = state.releases.get_mut(&release) else {
            bail!("no such release {release}");
        };
        existing.draft = false;
        Ok(existing.clone())
    }
}

#[cfg(test)]
mod rest_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_a_fresh_tag_succeeds() {
        let github = StubGitHub::new();
        assert_eq!(create_tag_idempotent(&github, "v1.0.0", "abc").unwrap(), TagOutcome::Created);
        assert_eq!(github.tag_commit("v1.0.0").unwrap().as_deref(), Some("abc"));
    }

    #[test]
    fn a_tag_left_by_an_earlier_attempt_is_a_resume() {
        let github = StubGitHub::new().with_tag("v1.0.0", "abc");
        assert_eq!(
            create_tag_idempotent(&github, "v1.0.0", "abc").unwrap(),
            TagOutcome::AlreadyCorrect
        );
    }

    #[test]
    fn a_tag_at_the_wrong_commit_is_an_incident() {
        let github = StubGitHub::new().with_tag("v1.0.0", "someone-else");
        let err = create_tag_idempotent(&github, "v1.0.0", "abc").unwrap_err().to_string();
        assert!(err.contains("already exists at someone-else"), "{err}");
        assert!(
            err.contains("abandoned"),
            "the operator needs to know it is unrecoverable: {err}"
        );
    }

    #[test]
    fn assets_are_read_back_and_compared() {
        let github = StubGitHub::new();
        let release = github.create_draft("v1.0.0", "abc", false).unwrap();
        let assets = vec![
            ("midenc".to_string(), b"binary-bytes".to_vec()),
            ("SHA256SUMS".to_string(), b"sums".to_vec()),
        ];

        let verified = upload_and_verify(&github, release.id, &assets).unwrap();
        assert_eq!(verified.len(), 2);
        assert_eq!(verified[0].digest, crate::registry::sha256_hex(b"binary-bytes"));
    }

    #[test]
    fn an_asset_that_comes_back_different_stops_the_release() {
        let github = StubGitHub::new().corrupting("midenc", b"tampered");
        let release = github.create_draft("v1.0.0", "abc", false).unwrap();
        let assets = vec![("midenc".to_string(), b"binary-bytes".to_vec())];

        let err = upload_and_verify(&github, release.id, &assets).unwrap_err().to_string();
        assert!(err.contains("does not match what was uploaded"), "{err}");
        assert!(err.contains("must not be published"), "{err}");
    }

    #[test]
    fn a_published_release_is_never_deleted() {
        let github = StubGitHub::new();
        let release = github.create_draft("v1.0.0", "abc", false).unwrap();
        github.publish_release(release.id).unwrap();

        let err = github.delete_release(release.id).unwrap_err().to_string();
        assert!(err.contains("published and will not be deleted"), "{err}");
    }

    #[test]
    fn drafts_can_be_deleted_which_is_what_makes_phase_c_reversible() {
        let github = StubGitHub::new();
        let release = github.create_draft("v1.0.0", "abc", false).unwrap();
        github.upload_asset(release.id, "midenc", b"bytes").unwrap();

        github.delete_release(release.id).unwrap();
        assert!(github.release_by_tag("v1.0.0").unwrap().is_none());
    }
}
