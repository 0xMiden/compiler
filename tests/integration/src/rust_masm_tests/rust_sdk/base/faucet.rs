use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_faucet_binding_test(name: &str, method: &str, protocol_function: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[component]
struct TestFaucet;

#[component]
impl TestFaucet {{
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
supported-types = ["FungibleFaucet", "NonFungibleFaucet"]

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

    assert_masm_execs_protocol_link(&mut test, "faucet", protocol_function);
}

#[test]
fn rust_sdk_account_faucet_create_fungible_asset_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_create_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        faucet::create_fungible_asset(Felt::new(10))
    }",
        "create_fungible_asset",
    );
}

#[test]
fn rust_sdk_account_faucet_create_non_fungible_asset_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_create_non_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let hash = Word::from([Felt::new(0); 4]);
        faucet::create_non_fungible_asset(hash)
    }",
        "create_non_fungible_asset",
    );
}

#[test]
fn rust_sdk_account_faucet_mint_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_mint_binding",
        "pub fn binding(&self) -> Asset {
        let asset = Asset::new(Word::from([Felt::new(0); 4]), Word::from([Felt::new(0); 4]));
        faucet::mint(asset)
    }",
        "mint",
    );
}

#[test]
fn rust_sdk_account_faucet_burn_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_burn_binding",
        "pub fn binding(&self) -> Asset {
        let asset = Asset::new(Word::from([Felt::new(0); 4]), Word::from([Felt::new(0); 4]));
        faucet::burn(asset)
    }",
        "burn",
    );
}

#[test]
fn rust_sdk_account_faucet_mint_value_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_mint_value_binding",
        "pub fn binding(&self) -> Word {
        let asset = Asset::new(Word::from([Felt::new(0); 4]), Word::from([Felt::new(0); 4]));
        faucet::mint_value(asset)
    }",
        "mint",
    );
}

#[test]
fn rust_sdk_account_faucet_burn_value_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_burn_value_binding",
        "pub fn binding(&self) -> Word {
        let asset = Asset::new(Word::from([Felt::new(0); 4]), Word::from([Felt::new(0); 4]));
        faucet::burn_value(asset)
    }",
        "burn",
    );
}
