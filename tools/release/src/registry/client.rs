//! Reading published state from a sparse index.
//!
//! This is the read-only half of registry interaction: what versions exist,
//! with what checksums, and whether they are yanked. Reconciliation depends on
//! it, and reconciliation is what makes a resume safe, so the failure mode that
//! matters most is a lookup that *appears* to succeed while returning nothing.
//! A missing crate and an unreachable registry are therefore distinct results,
//! not both "no versions".

use std::{collections::BTreeMap, process::Command};

use anyhow::{Context, Result, bail};

use super::index::{IndexEntry, index_path};

/// Read access to a registry's index.
pub trait IndexClient: Send + Sync {
    /// All published versions of a crate, or an empty vector if the crate has
    /// never been published.
    ///
    /// Returns an error if the registry could not be reached, which must never
    /// be confused with "no versions published".
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>>;
}

/// Queries a sparse index over HTTP using `curl`.
///
/// Deliberately uncached. Reconciliation asks about each crate exactly once per
/// pass, so a cache cannot help within a pass -- and across passes it is
/// actively harmful: a cached "not published" answer taken after a successful
/// upload would tell the executor to publish a crate that already exists.
pub struct SparseIndex {
    base_url: String,
}

impl SparseIndex {
    /// `base_url` accepts either a bare URL or Cargo's `sparse+` form.
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        let base_url = base_url.strip_prefix("sparse+").unwrap_or(&base_url).to_string();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

impl IndexClient for SparseIndex {
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>> {
        let url = format!("{}/{}", self.base_url, index_path(name));
        let output = Command::new("curl")
            .args(["--silent", "--location", "--max-time", "60"])
            .args(["--write-out", "%{http_code}"])
            .arg(&url)
            .output()
            .with_context(|| format!("failed to query {url}"))?;

        if !output.status.success() {
            bail!("failed to query {url}: curl exited with {}", output.status);
        }

        // The status code is appended to the body by `--write-out`.
        let body = String::from_utf8_lossy(&output.stdout);
        let split = body.len().saturating_sub(3);
        let (body, status) = body.split_at(split);

        let entries = match status {
            "404" => Vec::new(),
            "200" => body
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    serde_json::from_str::<IndexEntry>(line)
                        .with_context(|| format!("malformed index entry for '{name}'"))
                })
                .collect::<Result<Vec<_>>>()?,
            other => bail!("unexpected status {other} querying {url}"),
        };

        Ok(entries)
    }
}

/// An in-memory index, for tests.
#[derive(Debug, Default)]
pub struct StubIndex {
    entries: BTreeMap<String, Vec<IndexEntry>>,
    unreachable: bool,
}

impl StubIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a published version.
    pub fn publish(mut self, name: &str, version: &str, cksum: &str, yanked: bool) -> Self {
        self.entries.entry(name.to_string()).or_default().push(IndexEntry {
            name: name.to_string(),
            vers: version.to_string(),
            deps: Vec::new(),
            cksum: cksum.to_string(),
            features: Default::default(),
            yanked,
            links: None,
            rust_version: None,
        });
        self
    }

    /// Make every lookup fail, as an unreachable registry would.
    pub fn unreachable(mut self) -> Self {
        self.unreachable = true;
        self
    }
}

impl IndexClient for StubIndex {
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>> {
        if self.unreachable {
            bail!("registry unreachable");
        }
        Ok(self.entries.get(name).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_prefix_is_stripped() {
        let index = SparseIndex::new("sparse+http://127.0.0.1:8732/");
        assert_eq!(index.base_url, "http://127.0.0.1:8732");
    }

    #[test]
    fn stub_distinguishes_missing_from_unreachable() {
        let present = StubIndex::new().publish("a", "1.0.0", "abc", false);
        assert_eq!(present.versions("a").unwrap().len(), 1);
        assert!(present.versions("absent").unwrap().is_empty());

        assert!(StubIndex::new().unreachable().versions("a").is_err());
    }
}
