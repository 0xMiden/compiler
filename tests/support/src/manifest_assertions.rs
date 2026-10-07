//! Lookups and assertions over compiled package manifests, shared by tests (in both the
//! compiler tier and the protocol-linked tier) that inspect attribute-tagged procedure exports.

use miden_mast_package::{Package, PackageExport, ProcedureExport};

/// Returns the manifest procedure export uniquely matching `predicate`.
///
/// # Panics
/// Panics unless exactly one procedure export matches; `description` names the search for the
/// panic message.
pub fn find_manifest_procedure<'a>(
    package: &'a Package,
    description: &str,
    mut predicate: impl FnMut(&str) -> bool,
) -> &'a ProcedureExport {
    let matches = package
        .manifest
        .exports()
        .filter_map(|export| export.as_procedure())
        .filter(|export| predicate(export.path.as_ref().as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one manifest procedure matching {description}, got {:?}",
        package
            .manifest
            .exports()
            .filter_map(|export| match export {
                PackageExport::Procedure(export) => Some(export.path.as_ref().as_str().to_string()),
                PackageExport::Constant(_) | PackageExport::Type(_) => None,
            })
            .collect::<Vec<_>>(),
    );
    matches[0]
}

/// Asserts that the exported procedure carrying `attribute` is unique and preserves its leaf
/// export name.
pub fn assert_unique_protocol_export(
    package: &Package,
    attribute: &str,
    expected_export_name: &str,
) {
    let matching_exports = package
        .manifest
        .exports()
        .filter_map(|export| {
            let proc_export = export.as_procedure()?;
            proc_export.attributes.has(attribute).then_some(proc_export)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matching_exports.len(),
        1,
        "expected exactly one exported procedure to carry the `{attribute}` attribute",
    );

    let export_name = matching_exports[0]
        .path
        .last()
        .expect("protocol export should have a procedure name");
    assert_eq!(
        export_name, expected_export_name,
        "expected the `{attribute}` export to preserve the user-defined procedure name",
    );
}
