use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_faucet_binding_test(name: &str, method: &str) {
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

    test.expect_wasm(expect_file![format!("../../../../expected/rust_sdk/{name}.wat")]);
    test.expect_ir(expect_file![format!("../../../../expected/rust_sdk/{name}.hir")]);
    test.expect_masm(expect_file![format!("../../../../expected/rust_sdk/{name}.masm")]);

    test.compiled_package();
}

#[test]
fn rust_sdk_account_faucet_create_fungible_asset_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_create_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        faucet::create_fungible_asset(Felt::from_u32(10))
    }",
    );
}

#[test]
fn rust_sdk_account_faucet_create_non_fungible_asset_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_create_non_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let hash = Word::from([Felt::from_u32(0); 4]);
        faucet::create_non_fungible_asset(hash)
    }",
    );
}

#[test]
fn rust_sdk_account_faucet_mint_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_mint_binding",
        "pub fn binding(&self) -> Asset {
        let asset = Asset::from([Felt::from_u32(0); 4]);
        faucet::mint(asset)
    }",
    );
}

#[test]
fn rust_sdk_account_faucet_burn_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_burn_binding",
        "pub fn binding(&self) -> Asset {
        let asset = Asset::from([Felt::from_u32(0); 4]);
        faucet::burn(asset)
    }",
    );
}

#[test]
fn rust_sdk_account_faucet_get_total_issuance_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_get_total_issuance_binding",
        "pub fn binding(&self) -> Felt {
        faucet::get_total_issuance()
    }",
    );
}

#[test]
fn rust_sdk_account_faucet_is_non_fungible_asset_issued_binding() {
    run_faucet_binding_test(
        "rust_sdk_account_faucet_is_non_fungible_asset_issued_binding",
        "pub fn binding(&self) -> Felt {
        let asset = Asset::from([Felt::from_u32(0); 4]);
        if faucet::is_non_fungible_asset_issued(asset) {
            Felt::from_u32(1)
        } else {
            Felt::from_u32(0)
        }
    }",
    );
}
