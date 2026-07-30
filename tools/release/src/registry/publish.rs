//! The registry publish request body.
//!
//! Cargo uploads a length-prefixed metadata document followed by the
//! length-prefixed `.crate` archive:
//!
//! ```text
//! u32le json_len | json metadata | u32le crate_len | crate bytes
//! ```

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::Deserialize;

use super::index::{IndexDependency, IndexEntry};

#[derive(Debug, Clone, Deserialize)]
pub struct PublishMetadata {
    pub name: String,
    pub vers: String,
    #[serde(default)]
    pub deps: Vec<PublishDependency>,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub links: Option<String>,
    #[serde(default)]
    pub rust_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublishDependency {
    pub name: String,
    pub version_req: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_true")]
    pub default_features: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub registry: Option<String>,
    /// Present when the dependency is renamed in the consuming manifest.
    #[serde(default)]
    pub explicit_name_in_toml: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_kind() -> String {
    "normal".to_string()
}

/// A decoded publish request.
#[derive(Debug)]
pub struct PublishRequest {
    pub metadata: PublishMetadata,
    pub crate_bytes: Vec<u8>,
}

impl PublishRequest {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let json_len = read_u32(body, 0)? as usize;
        let json_end = 4 + json_len;
        if body.len() < json_end {
            bail!("publish body truncated: metadata length {json_len} exceeds body");
        }
        let metadata: PublishMetadata = serde_json::from_slice(&body[4..json_end])?;

        let crate_len = read_u32(body, json_end)? as usize;
        let crate_start = json_end + 4;
        let crate_end = crate_start + crate_len;
        if body.len() < crate_end {
            bail!("publish body truncated: crate length {crate_len} exceeds body");
        }

        Ok(Self {
            metadata,
            crate_bytes: body[crate_start..crate_end].to_vec(),
        })
    }

    /// Build the index entry this publication should produce.
    ///
    /// Renamed dependencies are recorded the way Cargo expects: `name` is the
    /// name used locally and `package` is the real crate name.
    pub fn to_index_entry(&self, cksum: String) -> IndexEntry {
        let deps = self
            .metadata
            .deps
            .iter()
            .map(|dep| match &dep.explicit_name_in_toml {
                Some(local_name) => IndexDependency {
                    name: local_name.clone(),
                    package: Some(dep.name.clone()),
                    req: dep.version_req.clone(),
                    features: dep.features.clone(),
                    optional: dep.optional,
                    default_features: dep.default_features,
                    target: dep.target.clone(),
                    kind: dep.kind.clone(),
                    registry: dep.registry.clone(),
                },
                None => IndexDependency {
                    name: dep.name.clone(),
                    package: None,
                    req: dep.version_req.clone(),
                    features: dep.features.clone(),
                    optional: dep.optional,
                    default_features: dep.default_features,
                    target: dep.target.clone(),
                    kind: dep.kind.clone(),
                    registry: dep.registry.clone(),
                },
            })
            .collect();

        IndexEntry {
            name: self.metadata.name.clone(),
            vers: self.metadata.vers.clone(),
            deps,
            cksum,
            features: self.metadata.features.clone(),
            yanked: false,
            links: self.metadata.links.clone(),
            rust_version: self.metadata.rust_version.clone(),
        }
    }
}

fn read_u32(body: &[u8], offset: usize) -> Result<u32> {
    let end = offset + 4;
    if body.len() < end {
        bail!("publish body truncated: expected a length prefix at offset {offset}");
    }
    let bytes: [u8; 4] = body[offset..end].try_into().expect("slice is exactly 4 bytes");
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(metadata: &str, crate_bytes: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        body.extend_from_slice(metadata.as_bytes());
        body.extend_from_slice(&(crate_bytes.len() as u32).to_le_bytes());
        body.extend_from_slice(crate_bytes);
        body
    }

    #[test]
    fn decodes_metadata_and_archive() {
        let body = encode(r#"{"name":"midenc-hir","vers":"0.10.0"}"#, b"crate-bytes");
        let request = PublishRequest::decode(&body).unwrap();
        assert_eq!(request.metadata.name, "midenc-hir");
        assert_eq!(request.metadata.vers, "0.10.0");
        assert_eq!(request.crate_bytes, b"crate-bytes");
    }

    #[test]
    fn renamed_dependencies_record_the_real_package_name() {
        let metadata = r#"{
            "name":"a","vers":"1.0.0",
            "deps":[{"name":"miden-thiserror","version_req":"^1.0",
                     "explicit_name_in_toml":"thiserror"}]
        }"#;
        let request = PublishRequest::decode(&encode(metadata, b"x")).unwrap();
        let entry = request.to_index_entry("cksum".into());
        assert_eq!(entry.deps[0].name, "thiserror");
        assert_eq!(entry.deps[0].package.as_deref(), Some("miden-thiserror"));
    }

    #[test]
    fn plain_dependencies_have_no_package_field() {
        let metadata = r#"{"name":"a","vers":"1.0.0",
                           "deps":[{"name":"serde","version_req":"^1.0"}]}"#;
        let request = PublishRequest::decode(&encode(metadata, b"x")).unwrap();
        let entry = request.to_index_entry("cksum".into());
        assert_eq!(entry.deps[0].name, "serde");
        assert!(entry.deps[0].package.is_none());
        // Cargo defaults these when absent; getting them wrong would silently
        // change resolution for consumers of the rehearsal registry.
        assert!(entry.deps[0].default_features);
        assert_eq!(entry.deps[0].kind, "normal");
    }

    #[test]
    fn truncated_bodies_are_rejected() {
        let mut body = encode(r#"{"name":"a","vers":"1.0.0"}"#, b"payload");
        body.truncate(body.len() - 3);
        let err = PublishRequest::decode(&body).unwrap_err().to_string();
        assert!(err.contains("truncated"), "{err}");
    }
}
