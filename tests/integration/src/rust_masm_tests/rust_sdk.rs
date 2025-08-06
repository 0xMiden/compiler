use std::{collections::BTreeMap, env, path::PathBuf, sync::Arc};

use miden_core::{
    crypto::hash::RpoDigest,
    utils::{Deserializable, Serializable},
    Felt, FieldElement,
};
use miden_mast_package::Package;
use miden_objects::account::{AccountComponentMetadata, AccountComponentTemplate, InitStorageData};
use midenc_debug::Executor;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{interner::Symbol, FunctionIdent, Ident, SourceSpan};

use crate::{
    cargo_proj::project, compiler_test::sdk_crate_path, CompilerTest, CompilerTestBuilder,
};

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
        "../../expected/rust_sdk_account_test/{artifact_name}.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../expected/rust_sdk_account_test/{artifact_name}.hir"
    )]);
    // test.expect_masm(expect_file![format!(
    //     "../../expected/rust_sdk_account_test/{artifact_name}.masm"
    // )]);
}

#[test]
fn rust_sdk_cross_ctx_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account",
        config.clone(),
        [],
    );
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/cross_ctx_account.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/cross_ctx_account.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/cross_ctx_account.masm")]);
    let account_package = test.compiled_package();
    let lib = account_package.unwrap_library();
    let expected_module = "miden:cross-ctx-account/foo@1.0.0";
    let expected_function = "process-felt";
    let exports = lib
        .exports()
        .filter(|e| !e.module.to_string().starts_with("intrinsics"))
        .map(|e| format!("{}::{}", e.module, e.name.as_str()))
        .collect::<Vec<_>>();
    dbg!(&exports);
    assert!(
        lib.exports().any(|export| {
            export.module.to_string() == expected_module
                && export.name.as_str() == expected_function
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
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/cross_ctx_note.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/cross_ctx_note.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/cross_ctx_note.masm")]);
    let package = test.compiled_package();
    let program = package.unwrap_program();
    let mut exec = Executor::new(vec![]);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(&package.manifest.dependencies).unwrap();
    let trace = exec.execute(&program, &test.session);
}

#[test]
fn rust_sdk_cross_ctx_account_and_note_word() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account-word",
        config.clone(),
        [],
    );
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/cross_ctx_account_word.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/cross_ctx_account_word.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/cross_ctx_account_word.masm")]);
    let account_package = test.compiled_package();
    let lib = account_package.unwrap_library();
    let expected_module = "miden:cross-ctx-account-word/foo@1.0.0";
    let expected_function = "process-word";
    let exports = lib
        .exports()
        .filter(|e| !e.module.to_string().starts_with("intrinsics"))
        .map(|e| format!("{}::{}", e.module, e.name.as_str()))
        .collect::<Vec<_>>();
    // dbg!(&exports);
    assert!(
        lib.exports().any(|export| {
            export.module.to_string() == expected_module
                && export.name.as_str() == expected_function
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
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/cross_ctx_note_word.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/cross_ctx_note_word.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/cross_ctx_note_word.masm")]);
    let package = test.compiled_package();
    let mut exec = Executor::new(vec![]);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(&package.manifest.dependencies).unwrap();
    let trace = exec.execute(&package.unwrap_program(), &test.session);
}

#[test]
fn pure_rust_hir2() {
    let _ = env_logger::builder().is_test(true).try_init();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../rust-apps-wasm/rust-sdk/add", config, []);
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/{artifact_name}.hir")]);
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
        "../../expected/rust_sdk/cross_ctx_account_word_arg.wat"
    )]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/cross_ctx_account_word_arg.hir")]);
    test.expect_masm(expect_file![format!(
        "../../expected/rust_sdk/cross_ctx_account_word_arg.masm"
    )]);
    let account_package = test.compiled_package();

    let lib = account_package.unwrap_library();
    let expected_module = "miden:cross-ctx-account-word-arg/foo@1.0.0";
    let expected_function = "process-word";
    let exports = lib
        .exports()
        .filter(|e| !e.module.to_string().starts_with("intrinsics"))
        .map(|e| format!("{}::{}", e.module, e.name.as_str()))
        .collect::<Vec<_>>();
    dbg!(&exports);
    assert!(
        lib.exports().any(|export| {
            export.module.to_string() == expected_module
                && export.name.as_str() == expected_function
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
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/cross_ctx_note_word_arg.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/cross_ctx_note_word_arg.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/cross_ctx_note_word_arg.masm")]);
    let package = test.compiled_package();
    assert!(package.is_program());
    let mut exec = Executor::new(vec![]);
    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(&package.manifest.dependencies).unwrap();
    let trace = exec.execute(&package.unwrap_program(), &test.session);
}
