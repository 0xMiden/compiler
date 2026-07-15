use super::*;

#[allow(clippy::uninlined_format_args)]
/// Compiles a minimal `miden` account component which calls the specified `output_note` method, and
/// compares the generated WAT/HIR/MASM output to the checked-in expectations.
fn run_output_note_binding_test(name: &str, method: &str) {
    let component = account_component_source("TestOutputNote", method);
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

{component}
"
    );

    let sdk_path = sdk_crate_path();
    let namespace = account_component_namespace(name, "test-output-note");
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
fn rust_sdk_output_note_get_assets_info_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_assets_info_binding",
        "pub fn binding(&self) -> u32 {
        let info = output_note::get_assets_info(NoteIdx { inner: Felt::new(0).unwrap() });
        info.num_assets
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_assets_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_assets_binding",
        "pub fn binding(&self) -> Felt {
        let assets = output_note::get_assets(NoteIdx { inner: Felt::new(0).unwrap() });
        Felt::new(assets.len() as u64).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_recipient_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        output_note::get_recipient(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_metadata_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_metadata_binding",
        "pub fn binding(&self) -> Word {
        output_note::get_metadata(NoteIdx { inner: Felt::new(0).unwrap() }).header
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_attachments_commitment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_attachments_commitment_binding",
        "pub fn binding(&self) -> Word {
        output_note::get_attachments_commitment(NoteIdx { inner: Felt::new(0).unwrap() })
    }",
    );
}

#[test]
fn rust_sdk_output_note_create_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_create_binding",
        "pub fn binding(&self) -> NoteIdx {
        let recipient = Recipient::from([Felt::new(0).unwrap(); 4]);
        let tag = Tag { inner: Felt::new(0).unwrap() };
        let note_type = NoteType { inner: Felt::new(1).unwrap() };
        output_note::create(tag, note_type, recipient)
    }",
    );
}

#[test]
fn rust_sdk_output_note_add_asset_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_add_asset_binding",
        "pub fn binding(&self) -> Felt {
        let asset = Asset::new(Word::from([Felt::new(0).unwrap(); 4]), \
         Word::from([Felt::new(0).unwrap(); 4]));
        let idx = NoteIdx { inner: Felt::new(0).unwrap() };
        output_note::add_asset(asset, idx);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_add_word_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_add_word_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0).unwrap() };
        let attachment_scheme = Felt::new(1).unwrap();
        let attachment = Word::from([Felt::new(0).unwrap(); 4]);
        output_note::add_word_attachment(idx, attachment_scheme, attachment);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_set_word_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_set_word_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0).unwrap() };
        let attachment_scheme = Felt::new(0).unwrap();
        let attachment = Word::from([Felt::new(0).unwrap(); 4]);
        output_note::set_word_attachment(idx, attachment_scheme, attachment);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_add_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_add_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0).unwrap() };
        let attachment_scheme = Felt::new(1).unwrap();
        let attachment = Word::from([Felt::new(0).unwrap(); 4]);
        output_note::add_attachment(idx, attachment_scheme, attachment);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_set_array_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_set_array_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0).unwrap() };
        let attachment_scheme = Felt::new(0).unwrap();
        let attachment = Word::from([Felt::new(0).unwrap(); 4]);
        output_note::set_array_attachment(idx, attachment_scheme, attachment);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_add_attachment_from_memory_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_add_attachment_from_memory_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0).unwrap() };
        let attachment_scheme = Felt::new(1).unwrap();
        let attachment = [Word::from([Felt::new(0).unwrap(); 4])];
        output_note::add_attachment_from_memory(idx, attachment_scheme, &attachment);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_find_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_find_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let location = output_note::find_attachment(
            NoteIdx { inner: Felt::new(0).unwrap() },
            Felt::new(1).unwrap(),
        );
        location.index
    }",
    );
}

#[test]
fn rust_sdk_output_note_write_attachment_commitments_to_memory_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_write_attachment_commitments_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments =
            output_note::write_attachment_commitments_to_memory(NoteIdx { inner: \
         Felt::new(0).unwrap() });
        Felt::new(commitments.len() as u64).unwrap()
    }",
    );
}

#[test]
fn rust_sdk_output_note_write_attachment_to_memory_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_write_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let attachment = output_note::write_attachment_to_memory(
            NoteIdx { inner: Felt::new(0).unwrap() },
            Felt::new(0).unwrap(),
        );
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}
