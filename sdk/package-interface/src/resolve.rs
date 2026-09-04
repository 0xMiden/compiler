//! Finding an export among the packages linked into a build.
//!
//! The Wasm frontend resolves a linker stub's fully qualified name through this trait; tests
//! implement it over fixture packages without a session.

use alloc::vec::Vec;

use miden_assembly_syntax::ast::Path;
use miden_mast_package::{PackageId, Version};

use crate::model::{PackageInterface, ProcedureItem};

/// A procedure found by an [`ExportResolver`], with the package that exports it.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedProcedure<'a> {
    /// The exporting package.
    pub package: &'a PackageInterface,
    /// The procedure.
    pub procedure: &'a ProcedureItem,
}

/// Something that can find a procedure export by its fully qualified path.
pub trait ExportResolver {
    /// The procedure at `path`, if any package this resolver knows exports it. Packages are
    /// searched in order and the first match wins.
    fn resolve_procedure(&self, path: &Path) -> Option<ResolvedProcedure<'_>>;

    /// The packages that would be searched, in order, for diagnostics.
    fn searched(&self) -> Vec<(PackageId, Version)>;
}

impl ExportResolver for [PackageInterface] {
    fn resolve_procedure(&self, path: &Path) -> Option<ResolvedProcedure<'_>> {
        self.iter().find_map(|package| {
            package
                .procedure(path)
                .map(|procedure| ResolvedProcedure { package, procedure })
        })
    }

    fn searched(&self) -> Vec<(PackageId, Version)> {
        self.iter().map(|p| (p.name.clone(), p.version.clone())).collect()
    }
}

impl ExportResolver for Vec<PackageInterface> {
    fn resolve_procedure(&self, path: &Path) -> Option<ResolvedProcedure<'_>> {
        self.as_slice().resolve_procedure(path)
    }

    fn searched(&self) -> Vec<(PackageId, Version)> {
        self.as_slice().searched()
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use miden_assembly_syntax::ast::Path;

    use super::*;
    use crate::{
        model::PackageInterface,
        testing::{FIXTURE_SOURCE, assemble_fixture},
    };

    const OTHER_SOURCE: &str = r#"
pub proc other(x: felt) -> felt
    nop
end
"#;

    fn packages() -> Vec<PackageInterface> {
        alloc::vec![
            PackageInterface::from_package(&assemble_fixture("fixture", "fixture", FIXTURE_SOURCE)),
            PackageInterface::from_package(&assemble_fixture("other", "other", OTHER_SOURCE)),
        ]
    }

    #[test]
    fn resolves_across_packages_by_fully_qualified_path() {
        let packages = packages();
        let hit = packages.resolve_procedure(Path::new("other::other")).unwrap();
        assert_eq!(AsRef::<str>::as_ref(&hit.package.name), "other");
        assert_eq!(hit.procedure.name(), "other");
        let hit = packages.resolve_procedure(Path::new("::fixture::id")).unwrap();
        assert_eq!(AsRef::<str>::as_ref(&hit.package.name), "fixture");
    }

    #[test]
    fn a_miss_reports_what_was_searched() {
        let packages = packages();
        assert!(packages.resolve_procedure(Path::new("fixture::nope")).is_none());
        let found = packages.searched();
        let searched: Vec<&str> =
            found.iter().map(|(name, _)| AsRef::<str>::as_ref(name)).collect();
        assert_eq!(searched, ["fixture", "other"]);
    }

    #[test]
    fn the_first_package_in_order_wins_on_a_duplicate_path() {
        let mut packages = packages();
        packages.push(PackageInterface::from_package(&assemble_fixture(
            "fixture",
            "fixture",
            FIXTURE_SOURCE,
        )));
        let hit = packages.resolve_procedure(Path::new("fixture::id")).unwrap();
        assert!(core::ptr::eq(hit.package, &packages[0]));
    }
}
