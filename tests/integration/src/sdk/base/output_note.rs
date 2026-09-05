use miden_processor::{ExecutionOptions, StackInputs, advice::AdviceInputs, execute_sync};

use super::*;
use crate::end_to_end::support::default_host_with_core_lib;

/// The fixed code that every guest panic reports through the VM assertion error (the same code
/// `DECODE_PANIC_CODE` pins in the integration-network tests). The panic message is not
/// observable at the VM level, so the empty-attachment and too-many-words asserts share this
/// code; it still separates a wrapper panic from kernel asserts and from the extern sentinel.
const WRAPPER_PANIC_CODE: &str = "assertion failed with error code: 10154102372021603817";

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

/// Compiles and executes the attachment-length boundary fixture against a mocked protocol call.
fn run_attachment_length_boundary_test(attachment_len: usize, should_succeed: bool) {
    let name = format!("rust_sdk_output_note_attachment_length_{attachment_len}");
    let main_fn = format!(
        r#"() -> Felt {{
        let idx = NoteIdx {{ inner: Felt::new(0).unwrap() }};
        let attachment_scheme = Felt::new(1).unwrap();
        let attachment = [Word::from([Felt::new(0).unwrap(); 4]); {attachment_len}];
        output_note::add_attachment_from_memory(idx, attachment_scheme, &attachment);
        Felt::new(1).unwrap()
    }}"#
    );
    let extern_body = if should_succeed {
        "drop drop drop drop"
    } else {
        // If an invalid length reaches the extern call instead of trapping in the Rust wrapper,
        // this sentinel distinguishes that failure from the wrapper assertion.
        r#"drop drop drop drop push.0 assert.err="attachment extern sentinel""#
    };
    let masm = format!(
        r#"
pub proc add_attachment_from_memory
    {extern_body}
end
"#
    );

    let mut test_builder = CompilerTestBuilder::rust_fn_body_with_sdk_without_protocol(
        name,
        &main_fn,
        WasmTranslationConfig::default(),
        [],
    );
    test_builder.link_with_masm_module("miden::protocol::output_note", masm);
    let mut test = test_builder.build();
    let package = test.compile_package();

    let mut host = default_host_with_core_lib();
    let program = package.unwrap_program();
    let result = execute_sync(
        &program,
        StackInputs::default(),
        AdviceInputs::default(),
        &mut host,
        ExecutionOptions::default(),
    );

    if should_succeed {
        let trace = result.expect("accepted attachment length should execute");
        assert_eq!(trace.stack.get_num_elements(1), &[miden_core::Felt::ONE]);
    } else {
        let error = result.expect_err("invalid attachment length should panic in the guest");
        let error = error.to_string();
        assert!(
            error.contains(WRAPPER_PANIC_CODE),
            "unexpected failure message (wanted `{WRAPPER_PANIC_CODE}`): {error}"
        );
    }
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
fn rust_sdk_output_note_add_attachment_from_memory_rejects_zero_words() {
    run_attachment_length_boundary_test(0, false);
}

#[test]
fn rust_sdk_output_note_add_attachment_from_memory_accepts_one_word() {
    run_attachment_length_boundary_test(1, true);
}

#[test]
fn rust_sdk_output_note_add_attachment_from_memory_accepts_256_words() {
    run_attachment_length_boundary_test(256, true);
}

#[test]
fn rust_sdk_output_note_add_attachment_from_memory_rejects_257_words() {
    run_attachment_length_boundary_test(257, false);
}

#[test]
fn rust_sdk_output_note_find_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_find_attachment_binding",
        "pub fn binding(&self) -> u32 {
        output_note::find_attachment(
            NoteIdx { inner: Felt::new(0).unwrap() },
            Felt::new(1).unwrap(),
        )
        .unwrap_or(0)
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
            0,
        );
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}
