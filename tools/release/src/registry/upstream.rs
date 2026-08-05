//! Fetching index entries and archives for crates the rehearsal registry does
//! not own.
//!
//! This is behind a trait for two reasons: unit tests must run with no network
//! access at all, and the production rehearsal needs a cache so that a 33-crate
//! publish does not re-fetch the same third-party index entries thousands of
//! times. Measured on the real workspace, caching turned ~15,000 lookups into
//! 445 upstream requests.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Command,
    sync::{Mutex, RwLock},
};

const UPSTREAM_INDEX: &str = "https://index.crates.io";
const UPSTREAM_ARCHIVES: &str = "https://static.crates.io/crates";

pub trait Upstream: Send + Sync + std::fmt::Debug {
    /// Fetch a sparse-index path such as `/se/rd/serde`.
    fn fetch_index(&self, path: &str) -> Option<Vec<u8>>;

    /// Fetch a published `.crate` archive.
    fn fetch_archive(&self, name: &str, version: &str) -> Option<Vec<u8>>;
}

/// An upstream that knows nothing, for tests that must not touch the network.
#[derive(Debug, Default)]
pub struct NoUpstream;

impl Upstream for NoUpstream {
    fn fetch_index(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }

    fn fetch_archive(&self, _name: &str, _version: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Proxies crates.io over HTTPS using `curl`.
///
/// `curl` is used rather than an HTTP client crate because this is test-only
/// infrastructure, `curl` is present wherever Cargo runs (Cargo itself links
/// libcurl), and it keeps a TLS stack out of the dependency graph.
#[derive(Debug)]
pub struct CurlUpstream {
    cache_dir: Option<PathBuf>,
    memory: RwLock<BTreeMap<String, Option<Vec<u8>>>>,
    /// Serializes cache writes; reads are lock-free through `memory`.
    disk: Mutex<()>,
}

impl CurlUpstream {
    /// Create an upstream that caches responses in memory, and on disk when a
    /// cache directory is given.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            cache_dir,
            memory: RwLock::new(BTreeMap::new()),
            disk: Mutex::new(()),
        }
    }

    fn cache_path(&self, key: &str) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        Some(dir.join(key.trim_start_matches('/').replace('/', "_")))
    }

    fn cached(&self, key: &str) -> Option<Option<Vec<u8>>> {
        if let Some(hit) = self.memory.read().ok()?.get(key) {
            return Some(hit.clone());
        }
        let path = self.cache_path(key)?;
        let bytes = std::fs::read(path).ok()?;
        Some(Some(bytes))
    }

    fn store(&self, key: &str, value: &Option<Vec<u8>>) {
        if let Ok(mut memory) = self.memory.write() {
            memory.insert(key.to_string(), value.clone());
        }
        let (Some(bytes), Some(path)) = (value.as_ref(), self.cache_path(key)) else {
            return;
        };
        let _guard = self.disk.lock();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, bytes);
    }

    fn get(&self, url: &str, key: &str) -> Option<Vec<u8>> {
        if let Some(hit) = self.cached(key) {
            return hit;
        }
        let result = curl_get(url);
        self.store(key, &result);
        result
    }
}

impl Upstream for CurlUpstream {
    fn fetch_index(&self, path: &str) -> Option<Vec<u8>> {
        let path = path.trim_start_matches('/');
        self.get(&format!("{UPSTREAM_INDEX}/{path}"), path)
    }

    fn fetch_archive(&self, name: &str, version: &str) -> Option<Vec<u8>> {
        let url = format!("{UPSTREAM_ARCHIVES}/{name}/{name}-{version}.crate");
        self.get(&url, &format!("archive/{name}-{version}"))
    }
}

fn curl_get(url: &str) -> Option<Vec<u8>> {
    let output = Command::new("curl")
        .args(["--silent", "--fail", "--location", "--max-time", "60", url])
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_upstream_never_resolves() {
        assert!(NoUpstream.fetch_index("/se/rd/serde").is_none());
        assert!(NoUpstream.fetch_archive("serde", "1.0.0").is_none());
    }

    #[test]
    fn cache_keys_are_derived_from_the_path() {
        let upstream = CurlUpstream::new(Some(PathBuf::from("/tmp/cache")));
        let path = upstream.cache_path("se/rd/serde").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/cache/se_rd_serde"));
    }

    #[test]
    fn memory_cache_serves_repeated_lookups() {
        let upstream = CurlUpstream::new(None);
        upstream.store("se/rd/serde", &Some(b"entry".to_vec()));
        assert_eq!(upstream.cached("se/rd/serde"), Some(Some(b"entry".to_vec())));
        // A miss is distinguishable from a cached negative result.
        assert_eq!(upstream.cached("no/pe/nope"), None);
    }
}
