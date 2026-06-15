use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_active_note_binding_test(name: &str, method: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[component]
struct TestActiveNote;

#[component]
impl TestActiveNote {{
    {method}
}}
",
        method = method
    );

    let sdk_path = sdk_crate_path();
    let namespace = component_namespace(name);
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
"#
    );
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

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        name = name,
        sdk_path = sdk_path.display(),
    );

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
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
fn active_note_is_public_binding() {
    run_active_note_binding_test(
        "active_note_is_public_binding",
        "pub fn binding(&self) -> Felt {
        if active_note::is_public() {
            Felt::new(1).unwrap()
        } else {
            Felt::new(0).unwrap()
        }
    }",
    );
}

#[test]
fn active_note_is_private_binding() {
    run_active_note_binding_test(
        "active_note_is_private_binding",
        "pub fn binding(&self) -> Felt {
        if active_note::is_private() {
            Felt::new(1).unwrap()
        } else {
            Felt::new(0).unwrap()
        }
    }",
    );
}

#[test]
fn active_note_get_attachments_commitment_binding() {
    run_active_note_binding_test(
        "active_note_get_attachments_commitment_binding",
        "pub fn binding(&self) -> Word {
        active_note::get_attachments_commitment()
    }",
    );
}

#[test]
fn active_note_write_attachment_commitments_to_memory_binding() {
    run_active_note_binding_test(
        "active_note_write_attachment_commitments_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments = active_note::write_attachment_commitments_to_memory();
        Felt::new(commitments.len() as u64).unwrap()
    }",
    );
}

#[test]
fn active_note_write_attachment_to_memory_binding() {
    run_active_note_binding_test(
        "active_note_write_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let attachment = active_note::write_attachment_to_memory(Felt::new(0).unwrap());
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}

#[test]
fn active_note_find_attachment_binding() {
    run_active_note_binding_test(
        "active_note_find_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let location = active_note::find_attachment(Felt::new(1).unwrap());
        location.index
    }",
    );
}
