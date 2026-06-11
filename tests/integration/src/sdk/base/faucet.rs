use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_faucet_binding_test(name: &str, method: &str) {
    let component = account_component_source(
        "struct TestFaucetStorage;",
        "TestFaucetStorage",
        "TestFaucet",
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
    let namespace = account_component_namespace(name, "test-faucet");
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"

[package.metadata.miden]
supported-types = ["FungibleFaucet", "NonFungibleFaucet"]
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
fn account_faucet_create_fungible_asset_binding() {
    run_faucet_binding_test(
        "account_faucet_create_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        faucet::create_fungible_asset(Felt::new(10).unwrap())
    }",
    );
}

#[test]
fn account_faucet_create_non_fungible_asset_binding() {
    run_faucet_binding_test(
        "account_faucet_create_non_fungible_asset_binding",
        "pub fn binding(&self) -> Asset {
        let hash = Word::from([Felt::new(0).unwrap(); 4]);
        faucet::create_non_fungible_asset(hash)
    }",
    );
}

#[test]
fn account_faucet_mint_binding() {
    run_faucet_binding_test(
        "account_faucet_mint_binding",
        "pub fn binding(&self) -> Felt {
        let asset = Asset::new(Word::from([Felt::new(0).unwrap(); 4]), \
         Word::from([Felt::new(0).unwrap(); 4]));
        faucet::mint(asset);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn account_faucet_burn_binding() {
    run_faucet_binding_test(
        "account_faucet_burn_binding",
        "pub fn binding(&self) -> Felt {
        let asset = Asset::new(Word::from([Felt::new(0).unwrap(); 4]), \
         Word::from([Felt::new(0).unwrap(); 4]));
        faucet::burn(asset);
        Felt::new(0).unwrap()
    }",
    );
}

#[test]
fn account_faucet_has_callbacks_binding() {
    run_faucet_binding_test(
        "account_faucet_has_callbacks_binding",
        "pub fn binding(&self) -> Felt {
        if faucet::has_callbacks() {
            Felt::new(1).unwrap()
        } else {
            Felt::new(0).unwrap()
        }
    }",
    );
}
