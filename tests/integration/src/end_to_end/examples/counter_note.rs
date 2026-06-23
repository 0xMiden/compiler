use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;

use super::persist_cargo_miden_dependency;
use crate::{
    CompilerTestBuilder,
    assert_helpers::{assert_lifted_component_exports, assert_unique_protocol_export},
};

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
    assert!(counter_contract_package.is_library(), "expected library");
    assert_lifted_component_exports(
        counter_contract_package.as_ref(),
        &[
            r#"::"miden:counter-contract/miden-counter-contract@0.1.0"::"get-count""#,
            r#"::"miden:counter-contract/miden-counter-contract@0.1.0"::"increment-count""#,
        ],
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
