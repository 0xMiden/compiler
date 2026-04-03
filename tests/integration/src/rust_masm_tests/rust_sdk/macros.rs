use std::panic::{self, AssertUnwindSafe};

use super::*;

#[test]
fn component_macros_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut account = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/component-macros-account",
        config.clone(),
        [],
    );
    let result = panic::catch_unwind(AssertUnwindSafe(move || account.compile_package()));
    let panic_message = match result {
        Ok(_) => {
            panic!("Expected component export lifting with indirect pointer parameters to fail")
        }
        Err(panic_info) => {
            if let Some(message) = panic_info.downcast_ref::<String>() {
                message.clone()
            } else if let Some(message) = panic_info.downcast_ref::<&str>() {
                message.to_string()
            } else {
                "Unknown panic".to_string()
            }
        }
    };

    assert!(
        panic_message.contains("not yet implemented"),
        "unexpected panic message: {panic_message}"
    );

    //    let builder = CompilerTestBuilder::rust_source_cargo_miden(
    //        "../rust-apps-wasm/rust-sdk/component-macros-note",
    //        config,
    //        [],
    //    assert!(
    //        panic_message.contains("not yet implemented")
    //            && panic_message.contains("indirect pointer parameters"),
    //        "unexpected panic message: {panic_message}"
    //    );
    //    let mut note = builder.build();
    //    let note_package = note.compile_package();
    //    let program = note_package.unwrap_program();
    //
    //    let mut exec = executor_with_std(vec![], None);
    //    exec.dependency_resolver_mut()
    //        .add(account_package.digest(), account_package.into());
    //    exec.with_dependencies(note_package.manifest.dependencies())
    //        .expect("failed to add package dependencies");
    //    exec.execute(&program, note.session.source_manager.clone());
}

#[test]
fn auth_components_require_an_auth_script_method() {
    let name = "auth_components_require_an_auth_script_method";
    let sdk_path = sdk_crate_path();
    let component_package = format!("miden:{}", name.replace('_', "-"));
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
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "authentication-component"
"#,
        name = name,
        sdk_path = sdk_path.display(),
        component_package = component_package,
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, Word};

#[component]
struct AuthComponent;

#[component]
impl AuthComponent {
    pub fn auth_procedure(&self, _arg: Word) {}
}
"#;

    let cargo_proj =
        project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build();

    let output = std::process::Command::new("cargo")
        .arg("check")
        .arg("--target")
        .arg("wasm32-wasip2")
        .current_dir(cargo_proj.root())
        .output()
        .expect("failed to spawn `cargo check` for the auth-component regression test");
    assert!(
        !output.status.success(),
        "expected auth-component compilation to fail without `#[auth_script]`"
    );
    let panic_message = String::from_utf8_lossy(&output.stderr);

    assert!(
        panic_message
            .contains("authentication components require exactly one `#[auth_script]` method"),
        "unexpected panic message: {panic_message}"
    );
}
