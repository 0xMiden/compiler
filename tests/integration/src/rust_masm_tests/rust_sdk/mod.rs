use std::{collections::BTreeMap, env, path::PathBuf, sync::Arc};

use miden_core::{
    utils::{Deserializable, Serializable},
    Felt, FieldElement, Word,
};
use miden_debug::Executor;
use miden_lib::MidenLib;
use miden_mast_package::Package;
use miden_objects::account::{AccountComponentMetadata, AccountComponentTemplate, InitStorageData};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{interner::Symbol, FunctionIdent, Ident, SourceSpan};
use midenc_session::STDLIB;

use crate::{
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    CompilerTest, CompilerTestBuilder,
};

mod macros;
mod stdlib;

fn executor_with_std(args: Vec<Felt>) -> Executor {
    let mut exec = Executor::new(args);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut()
        .add(*std_library.digest(), std_library.clone().into());
    let base_library = Arc::new(MidenLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*base_library.digest(), base_library.clone().into());
    exec
}

#[test]
#[ignore = "until https://github.com/0xMiden/compiler/issues/439 is fixed"]
fn account() {
    let artifact_name = "miden_sdk_account_test";
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/account-test",
        config,
        [],
    );
    test.expect_wasm(expect_file![format!(
        "../../../expected/rust_sdk_account_test/{artifact_name}.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../../expected/rust_sdk_account_test/{artifact_name}.hir"
    )]);
    // test.expect_masm(expect_file![format!(
    //     "../../../expected/rust_sdk_account_test/{artifact_name}.masm"
    // )]);
}

#[test]
fn rust_sdk_swapp_note_bindings() {
    let name = "rust_sdk_swapp_note_bindings";
    let sdk_path = sdk_crate_path();
    let sdk_alloc_path = sdk_alloc_crate_path();
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2021"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "note-script"

[profile.release]
opt-level = "z"
panic = "abort"
debug = false
"#,
        name = name,
        sdk_path = sdk_path.display(),
        sdk_alloc_path = sdk_alloc_path.display(),
        component_package = component_package,
    );

    let lib_rs = r#"#![no_std]

use miden::*;

#[note_script]
fn run(_arg: Word) {
    let sender = active_note::get_sender();
    let script_root = active_note::get_script_root();
    let serial_number = active_note::get_serial_number();
    let balance = account::get_balance(sender);

    assert_eq!(sender.prefix, sender.prefix);
    assert_eq!(sender.suffix, sender.suffix);
    assert_eq!(script_root, script_root);
    assert_eq!(serial_number, serial_number);
    assert_eq!(balance, balance);
}
"#;

    let cargo_proj =
        project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build();

    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    test.expect_wasm(expect_file![format!(
        "../../../expected/rust_sdk/rust_sdk_swapp_note_bindings.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../../expected/rust_sdk/rust_sdk_swapp_note_bindings.hir"
    )]);
    test.expect_masm(expect_file![format!(
        "../../../expected/rust_sdk/rust_sdk_swapp_note_bindings.masm"
    )]);
    // Ensure the crate compiles all the way to a package, exercising the bindings.
    test.compiled_package();
}

#[test]
fn rust_sdk_cross_ctx_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account",
        config.clone(),
        [],
    );
    test.expect_wasm(expect_file![format!("../../../expected/rust_sdk/cross_ctx_account.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/rust_sdk/cross_ctx_account.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/rust_sdk/cross_ctx_account.masm")]);
    let account_package = test.compiled_package();
    let lib = account_package.unwrap_library();
    assert!(
        !lib.exports()
            .any(|export| { export.name.to_string().starts_with("intrinsics") }),
        "expected no intrinsics in the exports"
    );
    let expected_module = "miden:cross-ctx-account/foo@1.0.0";
    let expected_function = "process-felt";
    assert!(
        lib.exports().any(|export| {
            export.name.module.to_string() == expected_module
                && export.name.name.as_str() == expected_function
        }),
        "expected one of the exports to contain module '{expected_module}' and function \
         '{expected_function}"
    );
    // Test that the package loads
    let bytes = account_package.to_bytes();
    let loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-note",
        config,
        [],
    );

    let mut test = builder.build();
    test.expect_wasm(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note.masm")]);
    let package = test.compiled_package();
    let program = package.unwrap_program();
    let mut exec = executor_with_std(vec![]);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}

#[test]
fn rust_sdk_cross_ctx_account_and_note_word() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account-word",
        config.clone(),
        [],
    );
    test.expect_wasm(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_account_word.wat"
    )]);
    test.expect_ir(expect_file![format!("../../../expected/rust_sdk/cross_ctx_account_word.hir")]);
    test.expect_masm(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_account_word.masm"
    )]);
    let account_package = test.compiled_package();
    let lib = account_package.unwrap_library();
    let expected_module = "miden:cross-ctx-account-word/foo@1.0.0";
    let expected_function = "process-word";
    let exports = lib
        .exports()
        .filter(|e| !e.name.module.to_string().starts_with("intrinsics"))
        .map(|e| format!("{}::{}", e.name.module, e.name.name.as_str()))
        .collect::<Vec<_>>();
    // dbg!(&exports);
    assert!(
        lib.exports().any(|export| {
            export.name.module.to_string() == expected_module
                && export.name.name.as_str() == expected_function
        }),
        "expected one of the exports to contain module '{expected_module}' and function \
         '{expected_function}"
    );
    // Test that the package loads
    let bytes = account_package.to_bytes();
    let loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-note-word",
        config,
        [],
    );

    let mut test = builder.build();
    test.expect_wasm(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note_word.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note_word.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note_word.masm")]);
    let package = test.compiled_package();
    let mut exec = executor_with_std(vec![]);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
}

#[test]
fn pure_rust_hir2() {
    let _ = env_logger::builder().is_test(true).try_init();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../rust-apps-wasm/rust-sdk/add", config, []);
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/rust_sdk/{artifact_name}.hir")]);
}

#[test]
fn rust_sdk_cross_ctx_word_arg_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account-word-arg",
        config.clone(),
        [],
    );
    test.expect_wasm(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_account_word_arg.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_account_word_arg.hir"
    )]);
    test.expect_masm(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_account_word_arg.masm"
    )]);
    let account_package = test.compiled_package();

    let lib = account_package.unwrap_library();
    let expected_module = "miden:cross-ctx-account-word-arg/foo@1.0.0";
    let expected_function = "process-word";
    let exports = lib
        .exports()
        .filter(|e| !e.name.module.to_string().starts_with("intrinsics"))
        .map(|e| format!("{}::{}", e.name.module, e.name.name.as_str()))
        .collect::<Vec<_>>();
    dbg!(&exports);
    assert!(
        lib.exports().any(|export| {
            export.name.module.to_string() == expected_module
                && export.name.name.as_str() == expected_function
        }),
        "expected one of the exports to contain module '{expected_module}' and function \
         '{expected_function}"
    );

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-note-word-arg",
        config,
        [],
    );
    let mut test = builder.build();
    test.expect_wasm(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_note_word_arg.wat"
    )]);
    test.expect_ir(expect_file![format!("../../../expected/rust_sdk/cross_ctx_note_word_arg.hir")]);
    test.expect_masm(expect_file![format!(
        "../../../expected/rust_sdk/cross_ctx_note_word_arg.masm"
    )]);
    let package = test.compiled_package();
    assert!(package.is_program());
    let mut exec = executor_with_std(vec![]);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
}
