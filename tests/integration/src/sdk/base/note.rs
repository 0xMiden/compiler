use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_note_binding_test(name: &str, method: &str) {
    let component = account_component_source("TestNote", method);
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::*;

{component}
"
    );

    let sdk_path = sdk_crate_path();
    let namespace = account_component_namespace(name, "test-note");
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account"
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
fn note_compute_and_store_recipient_binding() {
    run_note_binding_test(
        "note_compute_and_store_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::compute_and_store_recipient(
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
            alloc::vec![Felt::new(0).unwrap(); 4],
        )
    }",
    );
}

#[test]
fn note_build_recipient_binding() {
    run_note_binding_test(
        "note_build_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::build_recipient(
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
            alloc::vec![Felt::new(0).unwrap(); 4],
        )
    }",
    );
}

#[test]
fn note_compute_storage_commitment_binding() {
    run_note_binding_test(
        "note_compute_storage_commitment_binding",
        "pub fn binding(&self) -> Word {
        let storage = alloc::vec![Felt::new(0).unwrap(); 4];
        note::compute_storage_commitment(&storage)
    }",
    );
}

#[test]
fn note_write_attachment_commitments_to_memory_binding() {
    run_note_binding_test(
        "note_write_attachment_commitments_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments = note::write_attachment_commitments_to_memory(
            Word::from([Felt::new(0).unwrap(); 4]),
        );
        Felt::new(commitments.len() as u64).unwrap()
    }",
    );
}

#[test]
fn note_write_attachment_to_memory_binding() {
    run_note_binding_test(
        "note_write_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let attachment = note::write_attachment_to_memory(
            Word::from([Felt::new(0).unwrap(); 4]),
        );
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}

#[test]
fn note_write_indexed_attachment_to_memory_binding() {
    run_note_binding_test(
        "note_write_indexed_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments = [Word::from([Felt::new(0).unwrap(); 4])];
        let attachment =
            note::write_indexed_attachment_to_memory(&commitments, 0);
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}

#[test]
fn note_compute_recipient_binding() {
    run_note_binding_test(
        "note_compute_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::compute_recipient(
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
        )
    }",
    );
}

#[test]
fn note_metadata_into_sender_binding() {
    run_note_binding_test(
        "note_metadata_into_sender_binding",
        "pub fn binding(&self) -> AccountId {
        note::metadata_into_sender(Word::from([Felt::new(0).unwrap(); 4]))
    }",
    );
}

#[test]
fn note_metadata_into_attachment_schemes_binding() {
    run_note_binding_test(
        "note_metadata_into_attachment_schemes_binding",
        "pub fn binding(&self) -> Word {
        note::metadata_into_attachment_schemes(Word::from([Felt::new(0).unwrap(); 4]))
    }",
    );
}

#[test]
fn note_metadata_into_note_type_binding() {
    run_note_binding_test(
        "note_metadata_into_note_type_binding",
        "pub fn binding(&self) -> Felt {
        note::metadata_into_note_type(Word::from([Felt::new(0).unwrap(); 4])).inner
    }",
    );
}

#[test]
fn note_metadata_into_tag_binding() {
    run_note_binding_test(
        "note_metadata_into_tag_binding",
        "pub fn binding(&self) -> Felt {
        note::metadata_into_tag(Word::from([Felt::new(0).unwrap(); 4])).inner
    }",
    );
}

#[test]
fn note_find_attachment_idx_binding() {
    run_note_binding_test(
        "note_find_attachment_idx_binding",
        "pub fn binding(&self) -> u32 {
        note::find_attachment_idx(
            Felt::new(1).unwrap(),
            Word::from([Felt::new(0).unwrap(); 4]),
        )
        .unwrap_or(0)
    }",
    );
}
