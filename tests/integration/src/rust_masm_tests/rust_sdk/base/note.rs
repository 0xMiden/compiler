use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_note_binding_test(name: &str, method: &str, protocol_function: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::*;

#[component]
struct TestNote;

#[component]
impl TestNote {{
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

    assert_masm_execs_protocol_link(&mut test, "note", protocol_function);
}

#[test]
fn rust_sdk_note_build_recipient_binding() {
    run_note_binding_test(
        "rust_sdk_note_build_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::build_recipient(
            Word::from([Felt::new(0); 4]),
            Word::from([Felt::new(0); 4]),
            alloc::vec![Felt::new(0); 4],
        )
    }",
        "build_recipient",
    );
}
