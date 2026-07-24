use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_input_note_binding_test(name: &str, method: &str) {
    let component = account_component_source("TestInputNote", method);
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

{component}
"
    );

    let sdk_path = sdk_crate_path();
    let namespace = account_component_namespace(name, "test-input-note");
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"
path = "src/lib.rs"

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
fn input_note_get_initial_assets_info_binding() {
    run_input_note_binding_test(
        "input_note_get_initial_assets_info_binding",
        "pub fn binding(&self) -> Felt {
        let info = input_note::get_initial_assets_info(NoteIdx { inner: Felt::new(0).unwrap() });
        info.num_assets
    }",
    );
}

#[test]
fn input_note_get_initial_assets_binding() {
    run_input_note_binding_test(
        "input_note_get_initial_assets_binding",
        "pub fn binding(&self) -> Felt {
        let assets = input_note::get_initial_assets(NoteIdx { inner: Felt::new(0).unwrap() });
        Felt::new(assets.len() as u64).unwrap()
    }",
    );
}

#[test]
fn input_note_get_recipient_binding() {
    run_input_note_binding_test(
        "input_note_get_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        input_note::get_recipient(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn input_note_get_metadata_binding() {
    run_input_note_binding_test(
        "input_note_get_metadata_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_metadata(NoteIdx { inner: Felt::new(0).unwrap() }).header
    }",
    );
}

#[test]
fn input_note_get_sender_binding() {
    run_input_note_binding_test(
        "input_note_get_sender_binding",
        "pub fn binding(&self) -> AccountId {
        input_note::get_sender(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn input_note_get_storage_info_binding() {
    run_input_note_binding_test(
        "input_note_get_storage_info_binding",
        "pub fn binding(&self) -> Felt {
        let info = input_note::get_storage_info(NoteIdx { inner: Felt::new(0).unwrap() });
        info.num_storage_items
    }",
    );
}

#[test]
fn input_note_get_script_root_binding() {
    run_input_note_binding_test(
        "input_note_get_script_root_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_script_root(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn input_note_get_serial_number_binding() {
    run_input_note_binding_test(
        "input_note_get_serial_number_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_serial_number(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn input_note_get_attachments_commitment_binding() {
    run_input_note_binding_test(
        "input_note_get_attachments_commitment_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_attachments_commitment(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn input_note_get_attachments_commitment_raw_binding() {
    run_input_note_binding_test(
        "input_note_get_attachments_commitment_raw_binding",
        "pub fn binding(&self) -> Word {
        input_note::get_attachments_commitment_raw(
            Felt::new(0).unwrap(),
            NoteIdx { inner: Felt::new(0).unwrap() },
        )
    }",
    );
}

#[test]
fn input_note_write_attachment_commitments_to_memory_binding() {
    run_input_note_binding_test(
        "input_note_write_attachment_commitments_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments =
            input_note::write_attachment_commitments_to_memory(NoteIdx { inner: \
         Felt::new(0).unwrap() });
        Felt::new(commitments.len() as u64).unwrap()
    }",
    );
}

#[test]
fn input_note_write_attachment_to_memory_binding() {
    run_input_note_binding_test(
        "input_note_write_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let attachment = input_note::write_attachment_to_memory(
            NoteIdx { inner: Felt::new(0).unwrap() },
            Felt::new(0).unwrap(),
        );
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}

#[test]
fn input_note_find_attachment_binding() {
    run_input_note_binding_test(
        "input_note_find_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let location = input_note::find_attachment(
            NoteIdx { inner: Felt::new(0).unwrap() },
            Felt::new(1).unwrap(),
        );
        location.index
    }",
    );
}
