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
