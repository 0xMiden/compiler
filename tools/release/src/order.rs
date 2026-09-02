//! Publication ordering.
//!
//! Cargo computes a packaging order internally, but that order is not reliable
//! at scale: given dozens of publishable crates, an alphabetically ordered
//! `-p` list deterministically fails because Cargo reaches a crate before the
//! workspace dependency it needs. The identical set in topological order
//! succeeds. See `tools/release/README.md`.
//!
//! Every packaging and publishing invocation therefore takes its `-p` list from
//! this module.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::{
    config::{Config, UnitKind},
    workspace::{Package, Workspace},
};

/// The seed plus every transitively-reachable `library`-owned package.
///
/// A `library` unit publishes crates but is never released on its own, so its
/// crates must be pulled into whichever releasable unit depends on them. This
/// walks `seed`'s local dependencies, pulling in any package owned by a
/// `library` unit, transitively; a dependency owned by no unit, or by a
/// non-library unit, is left for that unit (or nobody) to publish.
///
/// Insert-then-push bounds termination by set membership rather than any
/// assumption that the dependency graph is acyclic.
pub fn library_closure(
    ws: &Workspace,
    config: &Config,
    seed: BTreeSet<String>,
) -> BTreeSet<String> {
    let mut wanted = seed;
    let mut queue: Vec<String> = wanted.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        let Some(package) = ws.packages.get(&name) else {
            continue;
        };
        for (dep, _) in &package.local_deps {
            // A dependency in another releasable unit is that unit's to
            // publish; only library crates are pulled across.
            let is_library = config
                .unit_of(dep)
                .and_then(|owner| config.units.get(owner))
                .is_some_and(|owner| owner.kind == UnitKind::Library);
            if is_library && wanted.insert(dep.clone()) {
                queue.push(dep.clone());
            }
        }
    }
    wanted
}

/// The packages a `--unit` selection publishes.
///
/// A unit's own packages plus the transitive `library` closure over them, which
/// is the same set `intent::package_order` stages. A flat `packages_in` omits
/// the library crates a unit depends on, and every consequence of that omission
/// is a *quiet* one: an ordering that leaves a crate out of the `-p` list, a
/// closure check that calls a crate this release publishes an unpublished
/// outside dependency, a reconciliation that reports nothing to publish while a
/// library crate is genuinely absent from the registry.
///
/// The unit is resolved first, so an unknown name fails with the loader's
/// message rather than as an empty selection.
pub fn selection_for_unit(ws: &Workspace, config: &Config, unit: &str) -> Result<BTreeSet<String>> {
    config.unit(unit)?;
    let seed: BTreeSet<String> = config.packages_in(unit).map(|p| p.name.clone()).collect();
    Ok(library_closure(ws, config, seed))
}

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

    // `library_closure`: the shared traversal `intent::package_order` and
    // `changelog::unit_paths` both build on.

    fn library_closure_config() -> Config {
        crate::config::testing::config(
            r#"
schema-version = 2

[units.app]
kind = "crates"
version-source = "own"
tag = "app/v{version}"
changelog = "CHANGELOG.md"

[units.other]
kind = "crates"
version-source = "own"
tag = "other/v{version}"
changelog = "other/CHANGELOG.md"

[units.lib1]
kind = "library"
version-source = "workspace"

[units.lib2]
kind = "library"
version-source = "workspace"

[[packages]]
name = "app"
unit = "app"

[[packages]]
name = "other"
unit = "other"

[[packages]]
name = "lib1"
unit = "lib1"

[[packages]]
name = "lib2"
unit = "lib2"
"#,
        )
    }

    #[test]
    fn library_closure_reaches_a_library_depending_on_a_library() {
        // app -> lib1 -> lib2, both library-owned: the closure must reach past
        // lib1 to lib2.
        let ws = ws(&[
            ("app", &["lib1", "other"][..]),
            ("lib1", &["lib2"][..]),
            ("lib2", &[][..]),
            ("other", &[][..]),
        ]);
        let config = library_closure_config();

        let closure = library_closure(&ws, &config, selection(&["app"]));

        assert_eq!(closure, selection(&["app", "lib1", "lib2"]), "{closure:?}");
    }

    #[test]
    fn library_closure_leaves_a_non_library_owned_dependency_for_its_own_unit() {
        // "other" belongs to a `crates` unit, not `library`: it publishes
        // itself, so pulling it into "app"'s closure would double-publish it.
        let ws = ws(&[("app", &["other"][..]), ("other", &[][..])]);
        let config = library_closure_config();

        let closure = library_closure(&ws, &config, selection(&["app"]));

        assert_eq!(closure, selection(&["app"]), "{closure:?}");
    }

    #[test]
    fn library_closure_ignores_a_dependency_owned_by_no_unit() {
        // "unclassified" exists in the workspace graph but appears in no
        // `[[packages]]` entry, so `config.unit_of` has nothing to say about
        // it -- it must not be treated as a library.
        let ws = ws(&[("app", &["unclassified"][..]), ("unclassified", &[][..])]);
        let config = library_closure_config();

        let closure = library_closure(&ws, &config, selection(&["app"]));

        assert_eq!(closure, selection(&["app"]), "{closure:?}");
    }

    #[test]
    fn library_closure_terminates_on_a_cycle() {
        // lib1 <-> lib2 depend on each other: without the insert-then-push
        // guard this loops forever instead of returning.
        let ws = ws(&[("lib1", &["lib2"][..]), ("lib2", &["lib1"][..])]);
        let config = library_closure_config();

        let closure = library_closure(&ws, &config, selection(&["lib1"]));

        assert_eq!(closure, selection(&["lib1", "lib2"]), "{closure:?}");
    }

    // `selection_for_unit`: what a `--unit` flag selects. Every command taking
    // one goes through this, so a fifth call site cannot quietly go back to a
    // flat `packages_in`.

    #[test]
    fn a_unit_selection_includes_the_library_crates_it_depends_on() {
        // The shape that made `verify-closure --unit app` report a spurious
        // "not self-contained": lib1 is published by this release, but a flat
        // `packages_in("app")` leaves it looking like an outside dependency.
        let ws = ws(&[
            ("app", &["lib1", "other"][..]),
            ("lib1", &["lib2"][..]),
            ("lib2", &[][..]),
            ("other", &[][..]),
        ]);
        let selected = selection_for_unit(&ws, &library_closure_config(), "app").unwrap();
        assert_eq!(selected, selection(&["app", "lib1", "lib2"]), "{selected:?}");
    }

    #[test]
    fn a_unit_selection_leaves_another_units_packages_to_that_unit() {
        let ws = ws(&[("app", &["other"][..]), ("other", &[][..])]);
        let selected = selection_for_unit(&ws, &library_closure_config(), "app").unwrap();
        assert_eq!(selected, selection(&["app"]), "{selected:?}");
    }

    #[test]
    fn an_unknown_unit_is_rejected_rather_than_selecting_nothing() {
        let ws = ws(&[("app", &[][..])]);
        let err = selection_for_unit(&ws, &library_closure_config(), "nonsense")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a release unit"), "{err}");
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
