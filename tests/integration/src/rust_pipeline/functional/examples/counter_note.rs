use miden_core::serde::{Deserializable, Serializable};
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest, CompilerTestBuilder, rust_pipeline::support::assert_unique_protocol_export,
};

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
