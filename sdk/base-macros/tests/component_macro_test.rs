extern crate miden_base_macros;
use expect_test::expect;
// NOTE: StorageMap and Value imports might be needed again once the macro works
// use miden_base::{StorageMap, Value};
use miden_base_macros::component;

#[component]
struct TestComponent {
    #[storage(
        slot(0),
        description = "test value",
        type = "auth::rpo_falcon512::pub_key"
    )]
    // NOTE: Type might need adjustment if miden_base::Value is not in scope
    owner_public_key: miden_base::Value,

    #[storage(slot(1), description = "test map")]
    // NOTE: Type might need adjustment if miden_base::StorageMap is not in scope
    foo_map: miden_base::StorageMap,

    #[storage(slot(2))]
    // NOTE: Type might need adjustment if miden_base::Value is not in scope
    without_description: miden_base::Value,
}

#[test]
fn test_component_macro_expansion() {
    let test_component = TestComponent::default();
    assert_eq!(test_component.owner_public_key.slot, 0);
    assert_eq!(test_component.foo_map.slot, 1);
    assert_eq!(test_component.without_description.slot, 2);
}

#[test]
fn test_component_metadata_serialization() {
    use miden_objects::{account::AccountComponentMetadata, utils::Deserializable};

    let metadata =
        AccountComponentMetadata::read_from_bytes(&__MIDEN_ACCOUNT_COMPONENT_METADATA_BYTES)
            .expect("Failed to deserialize AccountComponentMetadata");

    let toml = metadata.as_toml().unwrap();

    expect![[r#"
        name = "miden-base-macros"
        description = "Provides proc macro support for Miden rollup SDK"
        version = "0.0.7"
        supported-types = []

        [[storage]]
        name = "owner_public_key"
        description = "test value"
        slot = 0
        type = "auth::rpo_falcon512::pub_key"

        [[storage]]
        name = "foo_map"
        description = "test map"
        slot = 1
        values = []

        [[storage]]
        name = "without_description"
        slot = 2
        type = "word"
    "#]]
    .assert_eq(&toml);
}
