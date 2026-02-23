use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_input_note_binding_test(name: &str, method: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[component]
struct TestInputNote;

#[component]
impl TestInputNote {{
    {method}
}}
",
        method = method
    );

    let sdk_path = sdk_crate_path();
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let cargo_toml = format!(
        r#"
cargo-features = ["trim-paths"]

[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        name = name,
        sdk_path = sdk_path.display(),
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

    test.compile_package();
}

#[test]
fn rust_sdk_input_note_get_assets_info_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_assets_info_binding",
        "pub fn binding(&self) -> Felt {
        let info = input_note::get_assets_info(NoteIdx { inner: Felt::from_u32(0) });
        info.num_assets
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_assets_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_assets_binding",
        "pub fn binding(&self) -> Felt {
        let assets = input_note::get_assets(NoteIdx { inner: Felt::from_u32(0) });
        Felt::from_u32(assets.len() as u32)
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_recipient_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        input_note::get_recipient(NoteIdx { inner: Felt::from_u32(0) })
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_metadata_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_metadata_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_metadata(NoteIdx { inner: Felt::from_u32(0) }).header
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_sender_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_sender_binding",
        "pub fn binding(&self) -> AccountId {
        input_note::get_sender(NoteIdx { inner: Felt::from_u32(0) })
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_inputs_info_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_inputs_info_binding",
        "pub fn binding(&self) -> Felt {
        let info = input_note::get_inputs_info(NoteIdx { inner: Felt::from_u32(0) });
        info.num_inputs
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_script_root_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_script_root_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_script_root(NoteIdx { inner: Felt::from_u32(0) })
    }",
    );
}

#[test]
fn rust_sdk_input_note_get_serial_number_binding() {
    run_input_note_binding_test(
        "rust_sdk_input_note_get_serial_number_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_serial_number(NoteIdx { inner: Felt::from_u32(0) })
    }",
    );
}
