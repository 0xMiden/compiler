use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_asset_binding_test(name: &str, method: &str) {
    let component = account_component_source(
        "struct TestAssetStorage;",
        "TestAssetStorage",
        "TestAsset",
        method,
    );
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

{component}
"
    );

    let sdk_path = sdk_crate_path();
    let namespace = account_component_namespace(name, "test-asset");
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
opt-level = "z"
panic = "abort"
debug = false
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
fn account_asset_create_fungible_asset_binding() {
    run_asset_binding_test(
        "account_asset_create_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let faucet = AccountId { prefix: Felt::new(1).unwrap(), suffix: Felt::new(0).unwrap() };
        asset::create_fungible_asset(faucet, Felt::new(10).unwrap(), false)
    }",
    );
}

#[test]
fn account_asset_create_non_fungible_asset_binding() {
    run_asset_binding_test(
        "account_asset_create_non_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let faucet = AccountId { prefix: Felt::new(1).unwrap(), suffix: Felt::new(0).unwrap() };
        let hash = Word::from([Felt::new(0).unwrap(); 4]);
        asset::create_non_fungible_asset(faucet, hash, false)
    }",
    );
}
