use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_asset_binding_test(name: &str, method: &str) {
    let lib_rs = format!(
        r"#![no_std]

use miden::*;

#[component]
struct TestAsset;

#[component]
impl TestAsset {{
    {method}
}}
",
        method = method
    );

    let sdk_path = sdk_crate_path();
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2021"
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
opt-level = "z"
panic = "abort"
debug = false
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
fn rust_sdk_account_asset_build_fungible_asset_binding() {
    run_asset_binding_test(
        "rust_sdk_account_asset_build_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let faucet = AccountId { prefix: Felt::from_u32(1), suffix: Felt::from_u32(0) };
        asset::build_fungible_asset(faucet, Felt::from_u32(10))
    }",
    );
}

#[test]
fn rust_sdk_account_asset_build_non_fungible_asset_binding() {
    run_asset_binding_test(
        "rust_sdk_account_asset_build_non_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let faucet = AccountId { prefix: Felt::from_u32(1), suffix: Felt::from_u32(0) };
        let hash = Word::from([Felt::from_u32(0); 4]);
        asset::build_non_fungible_asset(faucet, hash)
    }",
    );
}
