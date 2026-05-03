use miden_core::serde::{Deserializable, Serializable};
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest, CompilerTestBuilder, rust_pipeline::support::assert_unique_protocol_export,
};

#[test]
fn counter_contract_debug_build() {
    // This build checks the dev profile build compilation for counter-contract
    // see https://github.com/0xMiden/compiler/issues/510
    let config = WasmTranslationConfig::default();
    let mut builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-contract", config, []);
    builder.with_release(false);
    let mut test = builder.build();
    let package = test.compile_package();
}

#[test]
fn counter_note() {
    // build and check counter-note
    let config = WasmTranslationConfig::default();
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

#[test]
fn auth_component_no_auth() {
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../../examples/auth-component-no-auth", config, []);
    let auth_comp_package = test.compile_package();
    assert!(auth_comp_package.is_library());
    assert_unique_protocol_export(auth_comp_package.as_ref(), "auth_script", "auth-procedure");

    // Test that the package loads
    let bytes = auth_comp_package.to_bytes();
    let _loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
}

#[test]
fn auth_component_rpo_falcon512() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/auth-component-rpo-falcon512",
        config,
        [],
    );
    let auth_comp_package = test.compile_package();
    assert!(auth_comp_package.is_library());
    assert_unique_protocol_export(auth_comp_package.as_ref(), "auth_script", "check-signature");

    // Test that the package loads
    let bytes = auth_comp_package.to_bytes();
    let _loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
}
