//! Publication ordering.
//!
//! Cargo computes a packaging order internally, but that order is not reliable
//! at this workspace's scale: with 33 publishable crates, an alphabetically
//! ordered `-p` list deterministically fails because Cargo packages
//! `midenc-hir` before `midenc-session`, its own dependency. The identical set
//! in topological order succeeds. See `tasks/design/release-tooling.md` §8.4.
//!
//! Every packaging and publishing invocation therefore takes its `-p` list from
//! this module.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::workspace::{Package, Workspace};

/// Order `selected` so that every package appears after all of its dependencies
/// within the selection.
///
/// The result is deterministic: ties are broken by name, so the same selection
/// always produces the same order and the same `-p` list.
pub fn topological(ws: &Workspace, selected: &BTreeSet<String>) -> Result<Vec<String>> {
    for name in selected {
        if !ws.packages.contains_key(name) {
            bail!("'{name}' is not a member of this workspace");
        }
    }

    let deps: BTreeMap<&str, Vec<&str>> = selected
        .iter()
        .map(|name| {
            let pkg: &Package = &ws.packages[name];
            let edges = pkg
                .local_deps
                .iter()
                .map(|(dep, _)| dep.as_str())
                .filter(|dep| selected.contains(*dep))
                .collect();
            (name.as_str(), edges)
        })
        .collect();

    let mut order = Vec::with_capacity(selected.len());
    let mut visited = BTreeSet::new();
    let mut on_path = Vec::new();

    for name in selected {
        visit(name, &deps, &mut visited, &mut on_path, &mut order)?;
    }

    Ok(order)
}

fn visit<'a>(
    name: &'a str,
    deps: &BTreeMap<&'a str, Vec<&'a str>>,
    visited: &mut BTreeSet<&'a str>,
    on_path: &mut Vec<&'a str>,
    order: &mut Vec<String>,
) -> Result<()> {
    if visited.contains(name) {
        return Ok(());
    }
    if let Some(start) = on_path.iter().position(|n| *n == name) {
        let mut cycle: Vec<&str> = on_path[start..].to_vec();
        cycle.push(name);
        bail!(
            "dependency cycle among selected packages: {}\nA cycle here means these crates cannot \
             be published in any order. Break it by removing a version requirement from the \
             dev-dependency that closes it.",
            cycle.join(" -> ")
        );
    }

    on_path.push(name);
    for dep in deps.get(name).into_iter().flatten() {
        visit(dep, deps, visited, on_path, order)?;
    }
    on_path.pop();

    visited.insert(name);
    order.push(name.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(packages: &[(&str, &[&str])]) -> Workspace {
        use crate::workspace::EdgeKind;
        Workspace {
            root: std::path::PathBuf::from("/tmp"),
            packages: packages
                .iter()
                .map(|(name, deps)| {
                    (
                        name.to_string(),
                        Package {
                            version: "0.1.0".into(),
                            manifest_path: std::path::PathBuf::from("/tmp/Cargo.toml"),
                            local_deps: deps
                                .iter()
                                .map(|d| (d.to_string(), EdgeKind::Required))
                                .collect(),
                            publishable: true,
                        },
                    )
                })
                .collect(),
        }
    }

    fn selection(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn dependencies_precede_dependents() {
        // Mirrors the real failure: `hir` depends on `session`, which depends on
        // `macros`. Alphabetically `hir` sorts first, so a naive order breaks.
        let ws =
            ws(&[("hir", &["session"][..]), ("session", &["macros"][..]), ("macros", &[][..])]);
        let order = topological(&ws, &selection(&["hir", "session", "macros"])).unwrap();
        assert_eq!(order, ["macros", "session", "hir"]);
    }

    #[test]
    fn order_is_deterministic() {
        let ws = ws(&[("a", &[][..]), ("b", &[][..]), ("c", &["a", "b"][..])]);
        let selected = selection(&["a", "b", "c"]);
        let first = topological(&ws, &selected).unwrap();
        for _ in 0..8 {
            assert_eq!(topological(&ws, &selected).unwrap(), first);
        }
    }

    #[test]
    fn dependencies_outside_the_selection_are_ignored() {
        let ws = ws(&[("a", &["external"][..]), ("external", &[][..])]);
        let order = topological(&ws, &selection(&["a"])).unwrap();
        assert_eq!(order, ["a"]);
    }

    #[test]
    fn cycles_are_reported_with_the_path() {
        let ws = ws(&[("a", &["b"][..]), ("b", &["a"][..])]);
        let err = topological(&ws, &selection(&["a", "b"])).unwrap_err().to_string();
        assert!(err.contains("dependency cycle"), "{err}");
        assert!(err.contains("a -> b -> a") || err.contains("b -> a -> b"), "{err}");
    }

    #[test]
    fn unknown_packages_are_rejected() {
        let ws = ws(&[("a", &[][..])]);
        let err = topological(&ws, &selection(&["nope"])).unwrap_err().to_string();
        assert!(err.contains("not a member"), "{err}");
    }
}
