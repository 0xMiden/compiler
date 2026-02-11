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
    account.expect_wasm(expect_file![format!(
        "../../../expected/rust_sdk/component_macros_account.wat"
    )]);
    account.expect_ir(expect_file![format!(
        "../../../expected/rust_sdk/component_macros_account.hir"
    )]);
    account.expect_masm(expect_file![format!(
        "../../../expected/rust_sdk/component_macros_account.masm"
    )]);
    let account_package = account.compiled_package();

    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/component-macros-note",
        config,
        [],
    );
    let mut note = builder.build();
    note.expect_wasm(expect_file![format!("../../../expected/rust_sdk/component_macros_note.wat")]);
    note.expect_ir(expect_file![format!("../../../expected/rust_sdk/component_macros_note.hir")]);
    note.expect_masm(expect_file![format!(
        "../../../expected/rust_sdk/component_macros_note.masm"
    )]);
    let note_package = note.compiled_package();
    let program = note_package.unwrap_program();

    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(note_package.manifest.dependencies())
        .expect("failed to add package dependencies");
    exec.execute(&program, note.session.source_manager.clone());
}
