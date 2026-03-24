use std::panic::{self, AssertUnwindSafe};

use super::*;

#[test]
fn component_macros_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut account = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/component-macros-account",
        config.clone(),
        [],
    );
    let result = panic::catch_unwind(AssertUnwindSafe(move || account.compile_package()));
    let panic_message = match result {
        Ok(_) => {
            panic!("Expected component export lifting with indirect pointer parameters to fail")
        }
        Err(panic_info) => {
            if let Some(message) = panic_info.downcast_ref::<String>() {
                message.clone()
            } else if let Some(message) = panic_info.downcast_ref::<&str>() {
                message.to_string()
            } else {
                "Unknown panic".to_string()
            }
        }
    };

    assert!(
        panic_message.contains("not yet implemented")
            && panic_message.contains("indirect pointer parameters"),
        "unexpected panic message: {panic_message}"
    );

    //    let builder = CompilerTestBuilder::rust_source_cargo_miden(
    //        "../rust-apps-wasm/rust-sdk/component-macros-note",
    //        config,
    //        [],
    //    assert!(
    //        panic_message.contains("not yet implemented")
    //            && panic_message.contains("indirect pointer parameters"),
    //        "unexpected panic message: {panic_message}"
    //    );
    //    let mut note = builder.build();
    //    let note_package = note.compile_package();
    //    let program = note_package.unwrap_program();
    //
    //    let mut exec = executor_with_std(vec![], None);
    //    exec.dependency_resolver_mut()
    //        .add(account_package.digest(), account_package.into());
    //    exec.with_dependencies(note_package.manifest.dependencies())
    //        .expect("failed to add package dependencies");
    //    exec.execute(&program, note.session.source_manager.clone());
}
