use std::collections::BTreeSet;

use miden_mast_package::PackageExport;
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;

use super::persist_cargo_miden_dependency;
use crate::{CompilerTestBuilder, assert_helpers::assert_unique_protocol_export};

/// Assert that the counter contract package exposes only lifted Component Model wrappers.
fn assert_counter_contract_exports_are_lifted_component_wrappers(
    package: &miden_mast_package::Package,
) {
    let expected_exports = BTreeSet::from([
        r#"::"miden:counter-contract/miden-counter-contract@0.1.0"::"get-count""#.to_string(),
        r#"::"miden:counter-contract/miden-counter-contract@0.1.0"::"increment-count""#.to_string(),
    ]);

    let procedure_exports = package
        .mast
        .exports()
        .filter_map(|export| export.as_procedure())
        .collect::<Vec<_>>();
    let mast_exports = procedure_exports
        .iter()
        .map(|export| export.path.as_ref().as_str().to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        mast_exports, expected_exports,
        "counter-contract should only export lifted Component Model wrappers",
    );

    for export in procedure_exports {
        assert!(
            export
                .signature
                .as_ref()
                .expect("lifted component export should have a signature")
                .calling_convention()
                .is_wasm_canonical_abi(),
            "export {} should use the Component Model calling convention",
            export.path
        );
    }

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
        "counter-contract manifest exports should match MAST exports",
    );
}

#[test]
fn counter_note() {
    let config = WasmTranslationConfig::default();
    let counter_contract_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-contract",
        config.clone(),
        [],
    );
    let mut counter_contract = counter_contract_builder.build();
    let counter_contract_package = counter_contract.compile_package();
    assert_counter_contract_exports_are_lifted_component_wrappers(
        counter_contract_package.as_ref(),
    );
    persist_cargo_miden_dependency(
        "../../examples/counter-contract",
        counter_contract_package.as_ref(),
    );

    // build and check counter-note
    let builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-note", config, []);

    let mut test = builder.build();

    let package = test.compile_package();
    assert!(package.is_library(), "expected library");
    let _note_script =
        NoteScript::from_package(package.as_ref()).expect("expected a note-script package");
    assert_unique_protocol_export(package.as_ref(), "note_script", "run");

    // TODO: uncomment after the testing environment implemented (node, devnet, etc.)
    //
    // let mut exec = Executor::new(vec![]);
    // for dep_path in test.dependencies {
    //     let account_package =
    //         Arc::new(Package::read_from_bytes(&std::fs::read(dep_path).unwrap()).unwrap());
    //     exec.dependency_resolver_mut()
    //         .add(account_package.digest(), account_package.into());
    // }
    // exec.with_dependencies(&package.manifest.dependencies).unwrap();
    // let trace = exec.execute(&package.unwrap_program(), &test.session);
}
