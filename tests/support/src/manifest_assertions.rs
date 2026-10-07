//! Lookups and assertions over compiled package manifests, shared by tests (in both the
//! compiler tier and the protocol-linked tier) that inspect attribute-tagged procedure exports.

use std::collections::BTreeSet;

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

/// Returns the Miden paths the package's embedded WIT declares as exports, plus the
/// compiler's `<namespace>::init` initializer: the exact set a manifest must expose.
pub fn expected_exports_from_wit(package: &Package, namespace: &str) -> BTreeSet<String> {
    // Manifest paths are absolute.
    let absolute = |path: &str| {
        if path.starts_with("::") {
            path.to_string()
        } else {
            format!("::{path}")
        }
    };
    let external_id = |function: &wit_parser::Function| {
        absolute(function.external_id.as_deref().unwrap_or_else(|| {
            panic!("exported WIT function `{}` carries no external-id", function.name)
        }))
    };
    let wit =
        midenc_frontend_wasm_metadata::package_wit(package).expect("package must embed its WIT");
    let wit = core::str::from_utf8(wit).expect("embedded WIT must be UTF-8");
    let parsed = wit_parser::UnresolvedPackageGroup::parse("package.wit", wit)
        .unwrap_or_else(|(map, err)| panic!("embedded WIT must parse: {}", err.render(&map)));
    let mut expected = BTreeSet::from([absolute(&format!(
        "{namespace}::{}",
        midenc_frontend_wasm_metadata::COMPONENT_INIT_PROCEDURE
    ))]);
    for (_, world) in parsed.main.worlds.iter() {
        for export in world.exports.values() {
            match export {
                wit_parser::WorldItem::Interface { id, .. } => {
                    for function in parsed.main.interfaces[*id].functions.values() {
                        expected.insert(external_id(function));
                    }
                }
                wit_parser::WorldItem::Function(function) => {
                    expected.insert(external_id(function));
                }
                wit_parser::WorldItem::Type { .. } => {}
            }
        }
    }
    expected
}

/// Asserts that the manifest of `package` exports exactly [`expected_exports_from_wit`].
pub fn assert_exports_match_wit(package: &Package, namespace: &str) {
    let actual = package
        .manifest
        .exports()
        .filter_map(|export| export.as_procedure())
        .map(|export| export.path.as_str().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual,
        expected_exports_from_wit(package, namespace),
        "the manifest must export exactly the WIT-declared procedures and init"
    );
}
