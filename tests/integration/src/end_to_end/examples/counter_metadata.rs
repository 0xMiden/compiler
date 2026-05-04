use std::borrow::Borrow;

use miden_core::serde::Deserializable;
use miden_mast_package::SectionId;
use miden_protocol::account::AccountComponentMetadata;
use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::CompilerTestBuilder;

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
