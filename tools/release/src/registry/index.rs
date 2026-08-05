//! Sparse index layout and entry format.
//!
//! Mirrors the registry index schema Cargo expects, which is what makes the
//! rehearsal registry a faithful stand-in for crates.io rather than an
//! approximation of it.

use serde::{Deserialize, Serialize};

/// The sparse index path for a crate name, e.g. `mi/de/midenc-hir`.
///
/// Cargo derives this from the lowercased name: one- and two-character names
/// live under `1/` and `2/`, three-character names under `3/<first char>/`, and
/// everything else under `<first two>/<next two>/`.
pub fn index_path(name: &str) -> String {
    let name = name.to_lowercase();
    match name.len() {
        0 => unreachable!("crate names are never empty"),
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{}", &name[..1], name),
        _ => format!("{}/{}/{}", &name[..2], &name[2..4], name),
    }
}

/// One dependency as recorded in an index entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDependency {
    pub name: String,
    pub req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: String,
    pub registry: Option<String>,
    /// Set when the dependency is renamed in the manifest; `name` then holds the
    /// name it is known by locally.
    pub package: Option<String>,
}

/// One published version of a crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<IndexDependency>,
    pub cksum: String,
    pub features: std::collections::BTreeMap<String, Vec<String>>,
    pub yanked: bool,
    pub links: Option<String>,
    pub rust_version: Option<String>,
}

impl IndexEntry {
    /// Serialize to the one-entry-per-line form the sparse index uses.
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("index entry is serializable");
        line.push('\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_paths_follow_the_registry_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("serde"), "se/rd/serde");
        assert_eq!(index_path("midenc-hir"), "mi/de/midenc-hir");
    }

    #[test]
    fn index_paths_are_lowercased() {
        assert_eq!(index_path("Inflector"), "in/fl/inflector");
    }

    #[test]
    fn entries_serialize_one_per_line() {
        let entry = IndexEntry {
            name: "midenc-hir".into(),
            vers: "0.10.0".into(),
            deps: vec![],
            cksum: "abc".into(),
            features: Default::default(),
            yanked: false,
            links: None,
            rust_version: None,
        };
        let line = entry.to_line();
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let parsed: IndexEntry = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed.vers, "0.10.0");
    }
}
