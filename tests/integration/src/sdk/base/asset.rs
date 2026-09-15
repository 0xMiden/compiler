use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_asset_binding_test(name: &str, method: &str) {
    let component = account_component_source("TestAsset", method);
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
path = "src/lib.rs"

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
fn asset_id_into_faucet_id_binding() {
    run_asset_binding_test(
        "asset_id_into_faucet_id_binding",
        "pub fn binding(&self) -> AccountId {
        let asset_id = Word::from([Felt::new(0).unwrap(); 4]);
        asset::id_into_faucet_id(asset_id)
    }",
    );
}

#[test]
fn asset_id_to_asset_class_binding() {
    run_asset_binding_test(
        "asset_id_to_asset_class_binding",
        "pub fn binding(&self) -> Felt {
        let asset_id = Word::from([Felt::new(0).unwrap(); 4]);
        let (class, echoed) = asset::id_to_asset_class(asset_id);
        class.prefix + class.suffix + echoed[0]
    }",
    );
}

#[test]
fn asset_id_into_asset_class_binding() {
    run_asset_binding_test(
        "asset_id_into_asset_class_binding",
        "pub fn binding(&self) -> Felt {
        let asset_id = Word::from([Felt::new(0).unwrap(); 4]);
        let class = asset::id_into_asset_class(asset_id);
        class.prefix + class.suffix
    }",
    );
}

#[test]
fn asset_id_into_composition_binding() {
    run_asset_binding_test(
        "asset_id_into_composition_binding",
        "pub fn binding(&self) -> Felt {
        let asset_id = Word::from([Felt::new(0).unwrap(); 4]);
        match asset::id_into_composition(asset_id) {
            AssetComposition::None => Felt::new(0).unwrap(),
            AssetComposition::Fungible => Felt::new(1).unwrap(),
            AssetComposition::Custom => Felt::new(2).unwrap(),
        }
    }",
    );
}
