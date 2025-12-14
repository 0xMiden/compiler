use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_tx_binding_test(name: &str, body: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[tx_script]
fn run(_arg: Word) {{
    {body}
}}
",
        body = body
    );

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
project-kind = "transaction-script"

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

    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", &lib_rs)
        .build();

    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    test.expect_wasm(expect_file![format!("../../../../expected/rust_sdk/{name}.wat")]);
    test.expect_ir(expect_file![format!("../../../../expected/rust_sdk/{name}.hir")]);
    test.expect_masm(expect_file![format!("../../../../expected/rust_sdk/{name}.masm")]);

    test.compiled_package();
}

#[test]
fn rust_sdk_account_tx_get_block_commitment_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_block_commitment_binding",
        "let commitment = tx::get_block_commitment();\n    let _ = commitment;",
    );
}

#[test]
fn rust_sdk_account_tx_get_block_number_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_block_number_binding",
        "let _ = tx::get_block_number();",
    );
}

#[test]
fn rust_sdk_account_tx_get_block_timestamp_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_block_timestamp_binding",
        "let _ = tx::get_block_timestamp();",
    );
}

#[test]
fn rust_sdk_account_tx_get_num_input_notes_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_num_input_notes_binding",
        "let _ = tx::get_num_input_notes();",
    );
}

#[test]
fn rust_sdk_account_tx_get_num_output_notes_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_num_output_notes_binding",
        "let _ = tx::get_num_output_notes();",
    );
}

#[test]
fn rust_sdk_account_tx_get_expiration_block_delta_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_expiration_block_delta_binding",
        "let _ = tx::get_expiration_block_delta();",
    );
}

#[test]
fn rust_sdk_account_tx_update_expiration_block_delta_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_update_expiration_block_delta_binding",
        "tx::update_expiration_block_delta(Felt::from_u32(42));",
    );
}

#[test]
fn rust_sdk_account_tx_get_input_notes_commitment_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_input_notes_commitment_binding",
        "let commitment = tx::get_input_notes_commitment();\n    let _ = commitment;",
    );
}

#[test]
fn rust_sdk_account_tx_get_output_notes_commitment_binding() {
    run_tx_binding_test(
        "rust_sdk_account_tx_get_output_notes_commitment_binding",
        "let commitment = tx::get_output_notes_commitment();\n    let _ = commitment;",
    );
}
