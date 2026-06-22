use std::collections::BTreeSet;

use miden_mast_package::PackageExport;

/// Asserts that the exported procedure carrying `attribute` is unique and preserves its leaf
/// export name.
pub(crate) fn assert_unique_protocol_export(
    package: &miden_mast_package::Package,
    attribute: &str,
    expected_export_name: &str,
) {
    let matching_exports = package
        .mast
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

/// Assert that every procedure exported by `package` is a lifted Component Model wrapper.
///
/// Internal procedures (lowered core Wasm functions, the component `init`, `cabi_*`, intrinsics)
/// do not use the Component Model calling convention, so this catches such procedures leaking into
/// the package export surface without having to enumerate the expected export names.
pub(crate) fn assert_all_exports_are_lifted_wrappers(package: &miden_mast_package::Package) {
    for export in package.mast.exports() {
        let Some(proc_export) = export.as_procedure() else {
            continue;
        };
        let is_lifted_wrapper = proc_export
            .signature
            .as_ref()
            .is_some_and(|signature| signature.calling_convention().is_wasm_canonical_abi());
        assert!(
            is_lifted_wrapper,
            "package should export only lifted Component Model wrappers, but `{}` is not one; \
             internal procedures must not leak into the package export surface",
            proc_export.path,
        );
    }
}

/// Assert that a package exposes exactly the expected lifted Component Model procedure wrappers.
pub(crate) fn assert_lifted_component_exports(
    package: &miden_mast_package::Package,
    expected_exports: &[&str],
) {
    let expected_exports = expected_exports
        .iter()
        .map(|export| (*export).to_string())
        .collect::<BTreeSet<_>>();

    let mast_exports = package
        .mast
        .exports()
        .filter_map(|export| export.as_procedure())
        .map(|export| export.path.as_ref().as_str().to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        mast_exports, expected_exports,
        "package should only export lifted Component Model wrappers",
    );

    assert_all_exports_are_lifted_wrappers(package);

    let manifest_exports = package
        .manifest
        .exports()
        .filter_map(|export| match export {
            PackageExport::Procedure(export) => Some(export.path.as_ref().as_str().to_string()),
            PackageExport::Constant(_) | PackageExport::Type(_) => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        manifest_exports, expected_exports,
        "package manifest exports should match MAST exports",
    );
}
