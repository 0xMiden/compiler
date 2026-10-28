//! End-to-end tests for the account storage schema of the storage-example component package.

#![cfg(feature = "protocol")]

use std::sync::{Arc, OnceLock};

use miden_account_storage_schema::{
    AccountStorageSchema, Felt, StorageSlotKind, Word, felts_from_word, word_from_felts,
};
use miden_mast_package::{Package, SectionId};
use miden_protocol::{
    account::{
        AccountComponent, StorageMapKey, StorageSlotContent, StorageSlotName,
        component::{
            AccountComponentMetadata, InitStorageData, StorageSchema, StorageSlotSchema, WordSchema,
        },
    },
    utils::serde::{Deserializable, Serializable},
};
use midenc_integration_test_support::{compile_project, workspace_root};

/// Builds the storage-example package once for this test process.
fn package() -> Arc<Package> {
    static PACKAGE: OnceLock<Arc<Package>> = OnceLock::new();
    PACKAGE
        .get_or_init(|| compile_project(&workspace_root().join("examples/storage-example")))
        .clone()
}

/// Returns the package's protocol component metadata.
fn metadata(package: &Package) -> AccountComponentMetadata {
    let section = package
        .sections
        .iter()
        .find(|section| section.id == SectionId::ACCOUNT_COMPONENT_METADATA)
        .expect("the component package must carry its protocol metadata");
    AccountComponentMetadata::read_from_bytes(&section.data).unwrap()
}

/// Returns the full protocol name of the slot with `leaf`.
fn slot_name(metadata: &AccountComponentMetadata, leaf: &str) -> StorageSlotName {
    metadata
        .storage_schema()
        .iter()
        .map(|(name, _)| name)
        .find(|name| name.as_str().rsplit("::").next() == Some(leaf))
        .unwrap_or_else(|| panic!("no protocol slot ends with `{leaf}`"))
        .clone()
}

/// Returns a felt from a small integer.
fn felt(value: u32) -> Felt {
    Felt::from_u32(value)
}

#[test]
fn from_package_names_slots_after_the_protocol_metadata() {
    let package = package();
    let schema = AccountStorageSchema::from_package(&package).unwrap();
    let metadata = metadata(&package);

    for slot in schema.slots() {
        assert_eq!(slot.name(), Some(slot_name(&metadata, slot.leaf()).as_str()));
    }
    let owner = schema.slot("owner_public_key").unwrap().name().unwrap();
    assert!(owner.ends_with("::owner_public_key"), "{owner}");

    let from_text = AccountStorageSchema::from_wit_text(schema.wit_text()).unwrap();
    assert!(from_text.slots().iter().all(|slot| slot.name().is_none()));
    let error = from_text.init_builder().build().unwrap_err().to_string();
    assert!(error.contains("has no protocol slot name"), "{error}");
}

#[test]
fn schema_and_protocol_metadata_must_declare_the_same_slots() {
    let package = package();
    let wit_text = AccountStorageSchema::from_package(&package).unwrap().wit_text().to_owned();
    let metadata = metadata(&package);
    let owner = slot_name(&metadata, "owner_public_key");
    let map = slot_name(&metadata, "asset_qty_map");
    let word =
        || WordSchema::new_simple(miden_protocol::account::component::SchemaType::native_word());
    let value_slot = || StorageSlotSchema::value("value", word());
    let map_slot = || {
        StorageSlotSchema::map(
            "map",
            miden_protocol::account::component::SchemaType::native_word(),
            miden_protocol::account::component::SchemaType::native_word(),
        )
    };
    let with_slots = |slots: Vec<(StorageSlotName, StorageSlotSchema)>| {
        AccountComponentMetadata::new("storage-example")
            .with_storage_schema(StorageSchema::new(slots).unwrap())
    };

    // The metadata as built matches.
    AccountStorageSchema::from_wit_text_and_metadata(&wit_text, &metadata).unwrap();

    let missing = with_slots(vec![(owner.clone(), value_slot())]);
    let error = AccountStorageSchema::from_wit_text_and_metadata(&wit_text, &missing)
        .unwrap_err()
        .to_string();
    assert!(error.contains("account storage slot `asset_qty_map` has no slot"), "{error}");

    let wrong_kind = with_slots(vec![(owner.clone(), map_slot()), (map.clone(), map_slot())]);
    let error = AccountStorageSchema::from_wit_text_and_metadata(&wit_text, &wrong_kind)
        .unwrap_err()
        .to_string();
    assert!(error.contains("`owner_public_key` is a value slot"), "{error}");
    assert!(error.contains("as a map slot"), "{error}");

    let extra_name = StorageSlotName::new("miden::storage_example::extra").unwrap();
    let extra =
        with_slots(vec![(owner, value_slot()), (map, map_slot()), (extra_name, value_slot())]);
    let error = AccountStorageSchema::from_wit_text_and_metadata(&wit_text, &extra)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("protocol slot `miden::storage_example::extra` has no slot"),
        "{error}"
    );

    // A package whose metadata disagrees with its schema does not load.
    let mut mismatched = (*package).clone();
    for section in &mut mismatched.sections {
        if section.id == SectionId::ACCOUNT_COMPONENT_METADATA {
            *section = miden_mast_package::Section::new(
                SectionId::ACCOUNT_COMPONENT_METADATA,
                missing.to_bytes(),
            );
        }
    }
    let error = AccountStorageSchema::from_package(&mismatched).unwrap_err().to_string();
    assert!(error.contains("has no slot named"), "{error}");

    let mut without_metadata = (*package).clone();
    without_metadata
        .sections
        .retain(|section| section.id != SectionId::ACCOUNT_COMPONENT_METADATA);
    let error = AccountStorageSchema::from_package(&without_metadata).unwrap_err().to_string();
    assert!(error.contains("no `account_component_metadata` section"), "{error}");
}

#[test]
fn init_storage_data_instantiates_the_component_and_decodes_back() {
    let package = package();
    let schema = AccountStorageSchema::from_package(&package).unwrap();
    let metadata = metadata(&package);
    let owner_key = Word::new([felt(1), felt(2), felt(3), felt(4)]);
    let asset = Word::new([felt(5), felt(6), felt(7), felt(8)]);

    let data: InitStorageData = schema
        .init_builder()
        .set("owner-public-key", "[1, 2, 3, 4]")
        .unwrap()
        .set_entry("asset_qty_map", &asset.to_hex(), "100")
        .unwrap()
        .set_entry_words("asset_qty_map", owner_key, word_from_felts(&[felt(7)]).unwrap())
        .unwrap()
        .build()
        .unwrap();

    let component = AccountComponent::from_package((*package).clone(), &data).unwrap();
    let mut saw_value = false;
    let mut saw_map = false;
    for slot in component.storage_slots() {
        match slot.content() {
            StorageSlotContent::Value(word) => {
                assert_eq!(slot.name(), &slot_name(&metadata, "owner_public_key"));
                assert_eq!(*word, owner_key);
                let decoded = schema.decode_value("owner_public_key", *word).unwrap();
                assert_eq!(decoded.to_string(), owner_key.to_hex());
                saw_value = true;
            }
            StorageSlotContent::Map(map) => {
                assert_eq!(slot.name(), &slot_name(&metadata, "asset_qty_map"));
                assert_eq!(map.num_entries(), 2);
                let stored = map.get(&StorageMapKey::new(asset));
                assert_eq!(
                    felts_from_word(stored),
                    [felt(100), Felt::ZERO, Felt::ZERO, Felt::ZERO]
                );
                let (key, value) = schema.decode_map_entry("asset_qty_map", asset, stored).unwrap();
                assert_eq!(key.to_string(), asset.to_hex());
                assert_eq!(value.to_string(), "100");
                saw_map = true;
            }
        }
    }
    assert!(saw_value && saw_map);
}

#[test]
fn init_builder_rejects_incomplete_and_conflicting_values() {
    let package = package();
    let schema = AccountStorageSchema::from_package(&package).unwrap();

    // Map slots may stay empty; value slots may not.
    let error = schema.init_builder().build().unwrap_err().to_string();
    assert!(
        error.contains("missing a value for account storage slot `owner_public_key`"),
        "{error}"
    );
    schema
        .init_builder()
        .set("owner_public_key", "[1, 2, 3, 4]")
        .unwrap()
        .build()
        .unwrap();

    let error = schema
        .init_builder()
        .set("owner_public_key", "[1, 2, 3, 4]")
        .unwrap()
        .set("owner_public_key", "[1, 2, 3, 4]")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("already has a value"), "{error}");

    let error = schema.init_builder().set("asset_qty_map", "1").err().unwrap().to_string();
    assert!(error.contains("is a map slot, not a value slot"), "{error}");
    let error = schema.init_builder().set("missing", "1").err().unwrap().to_string();
    assert!(error.contains("has no slot named `missing`"), "{error}");
    let error = schema
        .init_builder()
        .set_entry("asset_qty_map", "[1, 2, 3, 4]", "not-a-number")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("map value of account storage slot `asset_qty_map`"), "{error}");

    let key = Word::new([felt(1), felt(2), felt(3), felt(4)]);
    let error = schema
        .init_builder()
        .set("owner_public_key", "[1, 2, 3, 4]")
        .unwrap()
        .set_entry("asset_qty_map", &key.to_hex(), "1")
        .unwrap()
        .set_entry("asset_qty_map", &key.to_hex(), "2")
        .unwrap()
        .build()
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate init key"), "{error}");
}

#[test]
fn decoding_rejects_nonzero_padding_and_wrong_slot_kinds() {
    let package = package();
    let schema = AccountStorageSchema::from_package(&package).unwrap();
    let key = Word::new([felt(1), felt(2), felt(3), felt(4)]);

    let padded = Word::new([felt(100), Felt::ZERO, Felt::ZERO, felt(1)]);
    let error = schema.decode_map_entry("asset_qty_map", key, padded).unwrap_err().to_string();
    assert!(error.contains("map value of account storage slot `asset_qty_map`"), "{error}");
    assert!(error.contains("nonzero padding felt at index 3"), "{error}");
    let error = schema
        .init_builder()
        .set_entry_words("asset_qty_map", key, padded)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("nonzero padding felt"), "{error}");

    let error = schema.decode_value("asset_qty_map", key).unwrap_err().to_string();
    assert!(error.contains("is a map slot"), "{error}");
    let error = schema.decode_map_entry("owner_public_key", key, key).unwrap_err().to_string();
    assert!(error.contains("is a value slot"), "{error}");

    let StorageSlotKind::Map { .. } = schema.slot("asset_qty_map").unwrap().kind() else {
        panic!("asset_qty_map must be a map slot");
    };
    assert!(word_from_felts(&[Felt::ZERO; 5]).is_err());
}
