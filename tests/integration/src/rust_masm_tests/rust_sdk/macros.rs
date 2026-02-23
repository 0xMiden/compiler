use std::fs;

use super::*;

#[test]
fn component_macros_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut account = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/component-macros-account",
        config.clone(),
        [],
    );
    let account_package = account.compile_package();

    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/component-macros-note",
        config,
        [],
    );
    let mut note = builder.build();
    let note_package = note.compile_package();
    let program = note_package.unwrap_program();

    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(note_package.manifest.dependencies())
        .expect("failed to add package dependencies");
    exec.execute(&program, note.session.source_manager.clone());
}
