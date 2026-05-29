use std::{collections::BTreeSet, path::Path};

use miden_assembly::ast::types::{FunctionType, Type};
use miden_core::{
    program::Program,
    serde::{Deserializable, Serializable},
};
use miden_mast_package::{PackageExport, ProcedureExport};
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest, CompilerTestBuilder,
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    testing::executor_with_std,
};

mod base;
mod canonabi;
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

/// Writes a compiled package where `miden::generate!` expects Cargo Miden dependency artifacts.
fn persist_cargo_miden_dependency(
    project_path: impl AsRef<Path>,
    package: &miden_mast_package::Package,
) {
    package
        .write_masp_file(project_path.as_ref().join("target").join("miden").join("release"))
        .expect("failed to persist compiled Miden dependency package");
}

fn find_manifest_procedure<'a>(
    package: &'a miden_mast_package::Package,
    description: &str,
    mut predicate: impl FnMut(&str) -> bool,
) -> &'a ProcedureExport {
    let matches = package
        .manifest
        .exports()
        .filter_map(|export| match export {
            PackageExport::Procedure(export) => Some(export),
            PackageExport::Constant(_) | PackageExport::Type(_) => None,
        })
        .filter(|export| predicate(export.path.as_ref().as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one manifest procedure matching {description}, got {:?}",
        package
            .manifest
            .exports()
            .filter_map(|export| match export {
                PackageExport::Procedure(export) => Some(export.path.as_ref().as_str().to_string()),
                PackageExport::Constant(_) | PackageExport::Type(_) => None,
            })
            .collect::<Vec<_>>(),
    );
    matches[0]
}

fn assert_export_signature<'a>(
    function: &'a ProcedureExport,
    expected_params: &[&str],
    expected_result: &str,
) -> &'a FunctionType {
    let signature = function.signature.as_ref().expect("procedure export should have a signature");
    let params = signature.params.iter().map(ToString::to_string).collect::<Vec<_>>();
    let expected_params = expected_params.iter().map(|param| param.to_string()).collect::<Vec<_>>();
    assert_eq!(params, expected_params);

    let result = match signature.results.as_slice() {
        [] => "void".to_string(),
        [result] => result.to_string(),
        results => {
            format!("({})", results.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
        }
    };
    assert_eq!(result, expected_result);
    signature
}

fn assert_struct_field_types(ty: &Type, expected_fields: &[&str]) {
    let Type::Struct(struct_ty) = ty else {
        panic!("expected struct type, got {ty:?}");
    };
    let actual_fields =
        struct_ty.fields().iter().map(|field| field.ty.to_string()).collect::<Vec<_>>();
    let expected_fields = expected_fields.iter().map(|ty| ty.to_string()).collect::<Vec<_>>();
    assert_eq!(actual_fields, expected_fields);
}

fn assert_component_export_signatures_match_wit(package: &miden_mast_package::Package) {
    let component_export =
        find_manifest_procedure(package, "component export process-mixed", |name| {
            name.starts_with("::\"miden:cross-ctx-account-word/foo@1.0.0\"::")
                && name.ends_with("::\"process-mixed\"")
        });
    assert_eq!(
        component_export
            .signature
            .as_ref()
            .expect("component export should have a signature")
            .calling_convention()
            .as_str(),
        "component-model",
    );
    let mixed_struct = "struct {u64, struct miden:base/core-types@1.0.0/felt {\n    felt}, u32, \
                        struct miden:base/core-types@1.0.0/felt {felt}, u8, i1, u16}";
    let signature = assert_export_signature(component_export, &[mixed_struct], mixed_struct);
    assert_struct_field_types(
        &signature.params[0],
        &[
            "u64",
            "struct miden:base/core-types@1.0.0/felt {felt}",
            "u32",
            "struct miden:base/core-types@1.0.0/felt {felt}",
            "u8",
            "i1",
            "u16",
        ],
    );
    assert_struct_field_types(
        &signature.results[0],
        &[
            "u64",
            "struct miden:base/core-types@1.0.0/felt {felt}",
            "u32",
            "struct miden:base/core-types@1.0.0/felt {felt}",
            "u8",
            "i1",
            "u16",
        ],
    );
}

fn component_namespace(name: &str) -> String {
    let package = name.replace('_', "-");
    format!("miden:{package}/miden-{package}@0.0.1")
}

#[test]
fn rust_sdk_swapp_note_bindings() {
    let name = "rust_sdk_swapp_note_bindings";
    let namespace = component_namespace(name);
    let sdk_path = sdk_crate_path();
    let sdk_alloc_path = sdk_alloc_crate_path();
    let miden_project_toml = format!(
        r#"
        [package]
        name = "{name}"
        version = "0.0.1"

        [lib]
        kind = "note"
        namespace = "{namespace}"
        "#
    );
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

[profile.release]
opt-level = "z"
panic = "abort"
debug = false
"#,
        name = name,
        sdk_path = sdk_path.display(),
        sdk_alloc_path = sdk_alloc_path.display(),
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
        let asset_key = Word::from([Felt::new(0).unwrap(); 4]);
        let balance = active_account::get_balance(asset_key);

        assert_eq!(sender.prefix, sender.prefix);
        assert_eq!(sender.suffix, sender.suffix);
        assert_eq!(script_root, script_root);
        assert_eq!(serial_number, serial_number);
        assert_eq!(balance, balance);
    }
}
"#;

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build();

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
#[ignore = "https://github.com/0xMiden/compiler/issues/1120"]
fn rust_sdk_invalid_stack_offset_movup_16_issue_831() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/issue-invalid-stack-offset-movup",
        config,
        [],
    );

    // Ensure the crate compiles all the way to a package. This previously triggered the #831
    // panic in MASM codegen.
    let _package = test.compile_package();
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
    persist_cargo_miden_dependency(
        "../fixtures/components/cross-ctx-account",
        account_package.as_ref(),
    );
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
    let _trace = exec.execute(&program, test.session.source_manager.clone());
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
    persist_cargo_miden_dependency(
        "../fixtures/components/cross-ctx-account-word",
        account_package.as_ref(),
    );
    assert!(account_package.is_library());
    let lib = account_package.mast.clone();
    assert_component_export_signatures_match_wit(account_package.as_ref());
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
    let _loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();

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
    let _trace = exec.execute(&program, test.session.source_manager.clone());
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
    persist_cargo_miden_dependency(
        "../fixtures/components/cross-ctx-account-word-arg",
        account_package.as_ref(),
    );

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
    let _trace = exec.execute(&program, test.session.source_manager.clone());
}
