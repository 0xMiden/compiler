extern crate miden_base_macros;
use miden_base::{StorageMap, Value};
use miden_base_macros::component;

#[component]
struct TestComponent {
    #[storage(
        slot(0),
        description = "test value",
        type = "auth::rpo_falcon512::pub_key"
    )]
    owner_public_key: Value,

    #[storage(slot(1), description = "test map")]
    foo_map: StorageMap,

    #[storage(slot(2))]
    without_description: Value,
}

#[test]
fn test_component_macro_expansion() {
    assert_eq!(TestComponent.owner_public_key.slot, 0);
    assert_eq!(TestComponent.foo_map.slot, 1);
    assert_eq!(TestComponent.without_description.slot, 2);
}
