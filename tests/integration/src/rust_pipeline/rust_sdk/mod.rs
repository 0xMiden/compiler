use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::PathBuf,
};

use miden_core::{
    Felt, Word,
    program::Program,
    serde::{Deserializable, Serializable},
};
use miden_protocol::{
    account::{AccountComponentMetadata, component::InitStorageData},
    note::NoteScript,
};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{FunctionIdent, Ident, SourceSpan, interner::Symbol};
use midenc_session::STDLIB;

use crate::{
    CompilerTest, CompilerTestBuilder,
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    testing::{self, executor_with_std},
};

mod base;
mod macros;
mod stdlib;

/// Rebuilds an executable program from a compiled note-script package for direct execution tests.
fn note_script_program(package: &miden_mast_package::Package) -> Program {
    let note_script =
        NoteScript::from_package(package).expect("compiled package should contain a note script");
    Program::new(note_script.mast(), note_script.entrypoint())
}

/// Assert that package metadata exposes the same exported paths as the underlying library.
fn assert_manifest_exports_match_library(package: &miden_mast_package::Package) {
    let library_exports = package
        .mast
        .exports()
        .map(|export| export.path().as_ref().as_str().to_string())
        .collect::<BTreeSet<_>>();
    let manifest_exports = package
        .manifest
        .exports()
        .map(|export| export.path().as_ref().as_str().to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        manifest_exports, library_exports,
        "package manifest exports diverged from library exports"
    );
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
edition = "2024"
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
#![feature(alloc_error_handler)]

use miden::*;

#[note]
struct Note;

#[note]
impl Note {
    #[note_script]
    pub fn run(self, _arg: Word) {
        let sender = active_note::get_sender();
        let script_root = active_note::get_script_root();
        let serial_number = active_note::get_serial_number();
        let balance = active_account::get_balance(sender);

        assert_eq!(sender.prefix, sender.prefix);
        assert_eq!(sender.suffix, sender.suffix);
        assert_eq!(script_root, script_root);
        assert_eq!(serial_number, serial_number);
        assert_eq!(balance, balance);
    }
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

    // Ensure the crate compiles all the way to a package, exercising the bindings.
    test.compile_package();
}

/// Regression test for https://github.com/0xMiden/compiler/issues/831
///
/// Previously, compilation could panic during MASM codegen with:
/// `invalid stack offset for movup: 16 is out of range`.
#[test]
fn rust_sdk_invalid_stack_offset_movup_16_issue_831() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/issue-invalid-stack-offset-movup",
        config,
        [],
    );

    // Ensure the crate compiles all the way to a package. This previously triggered the #831
    // panic in MASM codegen.
    let package = test.compile_package();
}

#[test]
fn rust_sdk_cross_ctx_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-account",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    assert_manifest_exports_match_library(account_package.as_ref());
    let lib = account_package.mast.clone();
    let exports = lib
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        !lib.exports()
            .any(|export| export.path().as_ref().as_str().starts_with("intrinsics")),
        "expected no intrinsics in the exports"
    );
    let expected_module_prefix = "::\"miden:cross-ctx-account/";
    let expected_function_suffix = "\"process-felt\"";
    assert!(
        exports.iter().any(|export| export.starts_with(expected_module_prefix)
            && export.ends_with(expected_function_suffix)),
        "expected one of the exports to start with '{expected_module_prefix}' and end with \
         '{expected_function_suffix}', got exports: {exports:?}"
    );
    // Test that the package loads
    let bytes = account_package.to_bytes();
    let loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
    assert_manifest_exports_match_library(&loaded_package);

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-note",
        config,
        [],
    );

    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}

#[test]
fn rust_sdk_cross_ctx_account_and_note_word() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-account-word",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    let lib = account_package.mast.clone();
    let expected_module_prefix = "::\"miden:cross-ctx-account-word/";
    let expected_function_suffix = "\"process-word\"";
    let exports = lib
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    // dbg!(&exports);
    assert!(
        exports.iter().any(|export| export.starts_with(expected_module_prefix)
            && export.ends_with(expected_function_suffix)),
        "expected one of the exports to start with '{expected_module_prefix}' and end with \
         '{expected_function_suffix}', got exports: {exports:?}"
    );
    // Test that the package loads
    let bytes = account_package.to_bytes();
    let loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-note-word",
        config,
        [],
    );

    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}

#[test]
fn rust_sdk_cross_ctx_word_arg_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-account-word-arg",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();

    assert!(account_package.is_library());
    let lib = account_package.mast.clone();
    let expected_module_prefix = "::\"miden:cross-ctx-account-word-arg/";
    let expected_function_suffix = "\"process-word\"";
    let exports = lib
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        exports.iter().any(|export| export.starts_with(expected_module_prefix)
            && export.ends_with(expected_function_suffix)),
        "expected one of the exports to start with '{expected_module_prefix}' and end with \
         '{expected_function_suffix}', got exports: {exports:?}"
    );

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-note-word-arg",
        config,
        [],
    );
    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}
