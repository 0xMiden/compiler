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
    let test_component = TestComponent::default();
    assert_eq!(test_component.owner_public_key.slot, 0);
    assert_eq!(test_component.foo_map.slot, 1);
    assert_eq!(test_component.without_description.slot, 2);
}

#[test]
fn test_component_metadata_serialization() {
    use miden_objects::{
        account::{
            AccountComponentMetadata, StorageEntry, StorageValueName, TemplateType,
            WordRepresentation,
        },
        utils::Deserializable,
    };

    let metadata =
        AccountComponentMetadata::read_from_bytes(&__MIDEN_ACCOUNT_COMPONENT_METADATA_BYTES)
            .expect("Failed to deserialize AccountComponentMetadata");

    assert_eq!(metadata.name(), "TestComponent");

    let storage_entries = metadata.storage_entries();
    assert_eq!(storage_entries.len(), 3);

    // Entry 0: owner_public_key
    match &storage_entries[0] {
        StorageEntry::Value { slot, word_entry } => {
            assert_eq!(*slot, 0);
            match word_entry {
                WordRepresentation::Template {
                    r#type,
                    name,
                    description,
                } => {
                    assert_eq!(r#type, &TemplateType::new("auth::rpo_falcon512::pub_key").unwrap());
                    assert_eq!(name, &StorageValueName::new("owner_public_key").unwrap());
                    assert_eq!(description.as_deref(), Some("test value"));
                }
                _ => panic!("Expected WordRepresentation::Template for owner_public_key"),
            }
        }
        _ => panic!("Expected StorageEntry::Value for slot 0"),
    }

    // Entry 1: foo_map
    match &storage_entries[1] {
        StorageEntry::Map { slot, map } => {
            assert_eq!(*slot, 1);
            assert_eq!(map.name(), &StorageValueName::new("foo_map").unwrap());
            assert_eq!(map.description(), Some(&"test map".to_string()));
            assert!(map.entries().is_empty());
        }
        _ => panic!("Expected StorageEntry::Map for slot 1"),
    }

    // Entry 2: without_description
    match &storage_entries[2] {
        StorageEntry::Value { slot, word_entry } => {
            assert_eq!(*slot, 2);
            match word_entry {
                WordRepresentation::Template {
                    r#type,
                    name,
                    description,
                } => {
                    assert_eq!(r#type, &TemplateType::native_word());
                    assert_eq!(name, &StorageValueName::new("without_description").unwrap());
                    assert_eq!(description.as_deref(), None);
                }
                _ => panic!("Expected WordRepresentation::Template for without_description"),
            }
        }
        _ => panic!("Expected StorageEntry::Value for slot 2"),
    }
}
