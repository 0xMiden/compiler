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
