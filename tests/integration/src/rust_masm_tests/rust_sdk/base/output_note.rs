use super::*;

#[allow(clippy::uninlined_format_args)]
/// Compiles a minimal `miden` account component which calls the specified `output_note` method, and
/// compares the generated WAT/HIR/MASM output to the checked-in expectations.
fn run_output_note_binding_test(name: &str, method: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[component]
struct TestOutputNote;

#[component]
impl TestOutputNote {{
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

    test.expect_wasm(expect_file![format!("../../../../expected/rust_sdk/{name}.wat")]);
    test.expect_ir(expect_file![format!("../../../../expected/rust_sdk/{name}.hir")]);
    test.expect_masm(expect_file![format!("../../../../expected/rust_sdk/{name}.masm")]);

    test.compiled_package();
}

#[test]
fn rust_sdk_output_note_get_assets_info_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_assets_info_binding",
        "pub fn binding(&self) -> Felt {
        let info = output_note::get_assets_info(NoteIdx { inner: Felt::new(0) });
        info.num_assets
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_assets_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_assets_binding",
        "pub fn binding(&self) -> Felt {
        let assets = output_note::get_assets(NoteIdx { inner: Felt::new(0) });
        Felt::new(assets.len() as u64)
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_recipient_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        output_note::get_recipient(NoteIdx { inner: Felt::new(0) })
    }",
    );
}

#[test]
fn rust_sdk_output_note_get_metadata_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_get_metadata_binding",
        "pub fn binding(&self) -> Word {
        output_note::get_metadata(NoteIdx { inner: Felt::new(0) }).header
    }",
    );
}

#[test]
fn rust_sdk_output_note_create_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_create_binding",
        "pub fn binding(&self) -> NoteIdx {
        let recipient = Recipient::from([Felt::new(0); 4]);
        let tag = Tag { inner: Felt::new(0) };
        let note_type = NoteType { inner: Felt::new(1) };
        output_note::create(tag, note_type, recipient)
    }",
    );
}

#[test]
fn rust_sdk_output_note_add_asset_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_add_asset_binding",
        "pub fn binding(&self) -> Felt {
        let asset = Asset::from([Felt::new(0); 4]);
        let idx = NoteIdx { inner: Felt::new(0) };
        output_note::add_asset(asset, idx);
        Felt::new(0)
    }",
    );
}

#[test]
fn rust_sdk_output_note_set_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_set_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0) };
        let attachment_scheme = Felt::new(0);
        let attachment_kind = Felt::new(0);
        let attachment = Word::from([Felt::new(0); 4]);
        output_note::set_attachment(idx, attachment_scheme, attachment_kind, attachment);
        Felt::new(0)
    }",
    );
}

#[test]
fn rust_sdk_output_note_set_word_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_set_word_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0) };
        let attachment_scheme = Felt::new(0);
        let attachment = Word::from([Felt::new(0); 4]);
        output_note::set_word_attachment(idx, attachment_scheme, attachment);
        Felt::new(0)
    }",
    );
}

#[test]
fn rust_sdk_output_note_set_array_attachment_binding() {
    run_output_note_binding_test(
        "rust_sdk_output_note_set_array_attachment_binding",
        "pub fn binding(&self) -> Felt {
        let idx = NoteIdx { inner: Felt::new(0) };
        let attachment_scheme = Felt::new(0);
        let attachment = Word::from([Felt::new(0); 4]);
        output_note::set_array_attachment(idx, attachment_scheme, attachment);
        Felt::new(0)
    }",
    );
}
