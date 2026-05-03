use std::borrow::Borrow;

use miden_core::serde::Deserializable;
use miden_mast_package::SectionId;
use miden_protocol::account::AccountComponentMetadata;
use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, CompilerTestBuilder, testing::stripped_mast_size_str};

#[test]
fn storage_example() {
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../../examples/storage-example", config, []);

    let package = test.compile_package();
    let account_component_metadata_bytes = package
        .as_ref()
        .sections
        .iter()
        .find_map(|s| {
            if s.id == SectionId::ACCOUNT_COMPONENT_METADATA {
                Some(s.data.borrow())
            } else {
                None
            }
        })
        .unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(account_component_metadata_bytes)
        .unwrap()
        .to_toml()
        .unwrap();
    expect![[r#"
        name = "storage-example"
        description = "A simple example of a Miden account storage API"
        version = "0.1.0"
        supported-types = ["RegularAccountUpdatableCode"]

        [[storage.slots]]
        name = "miden_storage_example::my_account::owner_public_key"
        description = "owner public key"
        type = "word"

        [[storage.slots]]
        name = "miden_storage_example::my_account::asset_qty_map"
        description = "asset quantity map"

        [storage.slots.type]
        key = "word"
        value = "felt"
    "#]]
    .assert_eq(&toml);
}

#[test]
fn counter_contract() {
    let config = WasmTranslationConfig::default();
    let mut builder_release = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-contract",
        config.clone(),
        [],
    );
    builder_release.with_release(true);
    let mut test_release = builder_release.build();
    let package = test_release.compile_package();
    let account_component_metadata_bytes = package
        .as_ref()
        .sections
        .iter()
        .find_map(|s| {
            if s.id == SectionId::ACCOUNT_COMPONENT_METADATA {
                Some(s.data.borrow())
            } else {
                None
            }
        })
        .unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(account_component_metadata_bytes)
        .unwrap()
        .to_toml()
        .unwrap();
    expect![[r#"
        name = "counter-contract"
        description = "A simple example of a Miden counter contract using the Account Storage API"
        version = "0.1.0"
        supported-types = ["RegularAccountUpdatableCode"]

        [[storage.slots]]
        name = "miden_counter_contract::counter_contract::count_map"
        description = "counter contract storage map"

        [storage.slots.type]
        key = "word"
        value = "felt"
    "#]]
    .assert_eq(&toml);
}

#[test]
fn basic_wallet_and_p2id() {
    let config = WasmTranslationConfig::default();
    let mut account_test =
        CompilerTest::rust_source_cargo_miden("../../examples/basic-wallet", config.clone(), []);
    let account_package = account_test.compile_package();
    assert!(account_package.is_library(), "expected library");
    expect!["35906"].assert_eq(stripped_mast_size_str(&account_package));

    let mut tx_script_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/basic-wallet-tx-script",
        config.clone(),
        [],
    );
    let tx_script_package = tx_script_test.compile_package();
    assert!(tx_script_package.is_program(), "expected program");
    expect!["56437"].assert_eq(stripped_mast_size_str(&tx_script_package));

    let mut p2id_test =
        CompilerTest::rust_source_cargo_miden("../../examples/p2id-note", config.clone(), []);
    let note_package = p2id_test.compile_package();
    assert!(note_package.is_library(), "expected library");
    expect!["53082"].assert_eq(stripped_mast_size_str(&note_package));

    let mut p2ide_test =
        CompilerTest::rust_source_cargo_miden("../../examples/p2ide-note", config, []);
    let p2ide_package = p2ide_test.compile_package();
    assert!(p2ide_package.is_library(), "expected library");
    expect!["62672"].assert_eq(stripped_mast_size_str(&p2ide_package));
}
