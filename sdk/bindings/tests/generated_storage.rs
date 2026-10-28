//! End-to-end test for generated account storage bindings in a scratch consumer crate.

mod common;

use miden_mast_package::Section;
use miden_protocol::{
    account::{
        AccountComponentMetadata, StorageSlotName,
        component::{SchemaType, StorageSchema, StorageSlotSchema},
    },
    utils::serde::Serializable,
};
use midenc_frontend_wasm_metadata::package_account_storage_schema_section_id;
use midenc_integration_test_support::{compile_project, workspace_root, write_masp_file_atomic};

/// An account storage schema with custom slot types, swapped into a copy of the storage-example
/// package.
const CUSTOM_SCHEMA: &str = r#"package example:custom-storage-schema;

use miden:base/core-types@1.0.0;

interface account-storage {
    use core-types.{account-id, felt};

    /// A balance with a lock flag.
    record balance {
        amount: felt,
        locked: bool,
    }

    /// How deposits are limited.
    variant mode {
        open,
        capped(u32),
    }

    record storage {
        mode: mode,
        balances: list<tuple<account-id, balance>>,
        limit: option<u16>,
    }
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record account-id { prefix: felt, suffix: felt }
    }
}
"#;

/// Returns protocol metadata that declares the slots of [`CUSTOM_SCHEMA`].
fn custom_metadata() -> AccountComponentMetadata {
    let slot =
        |leaf: &str| StorageSlotName::new(format!("example::custom_storage::{leaf}")).unwrap();
    let word = SchemaType::native_word;
    AccountComponentMetadata::new("custom-storage").with_storage_schema(
        StorageSchema::new([
            (slot("mode"), StorageSlotSchema::value("mode", word())),
            (slot("balances"), StorageSlotSchema::map("balances", word(), word())),
            (slot("limit"), StorageSlotSchema::value("limit", word())),
        ])
        .unwrap(),
    )
}

#[test]
fn generated_storage_bindings_compile_and_run_in_a_consumer_crate() {
    let workspace = workspace_root();
    let storage_dir = workspace.join("examples/storage-example");
    let storage_example = compile_project(&storage_dir);
    write_masp_file_atomic(&storage_example, storage_dir.join("target/miden/release"))
        .expect("failed to persist the storage-example package");

    let temp = tempfile::tempdir().unwrap();
    let custom_dir = temp.path().join("packages/custom");
    let mut custom = (*storage_example).clone();
    let schema_id = package_account_storage_schema_section_id();
    custom.sections.retain(|section| {
        section.id != schema_id
            && section.id != miden_mast_package::SectionId::ACCOUNT_COMPONENT_METADATA
    });
    custom.sections.push(Section::new(schema_id, CUSTOM_SCHEMA.as_bytes().to_vec()));
    custom.sections.push(Section::new(
        miden_mast_package::SectionId::ACCOUNT_COMPONENT_METADATA,
        custom_metadata().to_bytes(),
    ));
    write_masp_file_atomic(&custom, &custom_dir).expect("failed to persist the custom package");
    let custom_path = custom_dir.join("storage-example.masp");

    let source = format!(
        r#"use miden_bindings::{{
    Felt, Word,
    account::{{AccountId, StorageSlotName, component::WordValue}},
    asset::AssetAmount,
}};

mod project_bindings {{
    miden_bindings::from_project!({project_dir:?});
}}

mod custom_bindings {{
    miden_bindings::from_package!({custom_path:?});
}}

/// Returns a word from small integers.
fn word(values: [u32; 4]) -> Word {{
    Word::new(values.map(Felt::from_u32))
}}

/// Returns the literal word of a value slot.
fn value_word(data: &miden_bindings::InitStorageData, slot: &str) -> Word {{
    match data.slot_value_entry(&StorageSlotName::new(slot).unwrap()) {{
        Some(WordValue::FullyTyped(word)) => *word,
        other => panic!("slot `{{slot}}` has no literal word: {{other:?}}"),
    }}
}}

/// Returns the literal words of the entries of a map slot.
fn map_words(data: &miden_bindings::InitStorageData, slot: &str) -> Vec<(Word, Word)> {{
    data.map_entries(&StorageSlotName::new(slot).unwrap())
        .unwrap()
        .iter()
        .map(|entry| match entry {{
            (WordValue::FullyTyped(key), WordValue::FullyTyped(value)) => (*key, *value),
            other => panic!("slot `{{slot}}` has an entry without literal words: {{other:?}}"),
        }})
        .collect()
}}

fn main() {{
    use project_bindings::{{Holding, Limits, Storage}};

    assert_eq!(Storage::OWNER_PUBLIC_KEY_SLOT, "miden::storage_example::foo::owner_public_key");
    assert_eq!(Storage::ASSET_QTY_MAP_SLOT, "miden::storage_example::foo::asset_qty_map");
    assert_eq!(Storage::LIMITS_SLOT, "miden::storage_example::foo::limits");
    assert_eq!(Storage::HOLDINGS_SLOT, "miden::storage_example::foo::holdings");
    let schema = Storage::schema().unwrap();
    assert_eq!(schema.slot("owner_public_key").unwrap().name(), Some(Storage::OWNER_PUBLIC_KEY_SLOT));

    let owner = word([1, 2, 3, 4]);
    let asset = word([5, 6, 7, 8]);
    let quantity = AssetAmount::new(100).unwrap();
    let limits = Limits {{ max_qty: AssetAmount::new(50).unwrap(), paused: true }};
    let holding = Holding {{ qty: AssetAmount::new(7).unwrap(), locked: true }};
    let storage = Storage {{
        owner_public_key: owner,
        asset_qty_map: vec![(asset, quantity)],
        limits: limits.clone(),
        holdings: vec![(asset, holding.clone())],
    }};
    let data = storage.to_init_storage_data().unwrap();
    assert_eq!(value_word(&data, Storage::OWNER_PUBLIC_KEY_SLOT), owner);
    let entries = map_words(&data, Storage::ASSET_QTY_MAP_SLOT);
    assert_eq!(entries, [(asset, word([100, 0, 0, 0]))]);
    let limits_word = value_word(&data, Storage::LIMITS_SLOT);
    assert_eq!(limits_word, word([50, 1, 0, 0]));
    let holdings = map_words(&data, Storage::HOLDINGS_SLOT);
    assert_eq!(holdings, [(asset, word([7, 1, 0, 0]))]);

    assert_eq!(Storage::decode_owner_public_key(owner).unwrap(), owner);
    let (key, value) = entries[0];
    assert_eq!(Storage::decode_asset_qty_map_entry(key, value).unwrap(), (asset, quantity));
    let error = Storage::decode_asset_qty_map_entry(key, word([100, 0, 0, 1])).unwrap_err();
    assert!(error.to_string().contains("nonzero padding felt at index 3"), "{{error}}");
    assert_eq!(Storage::decode_limits(limits_word).unwrap(), limits);
    let (key, value) = holdings[0];
    assert_eq!(Storage::decode_holdings_entry(key, value).unwrap(), (asset, holding.clone()));
    assert!(Storage::decode_holdings_entry(key, word([7, 2, 0, 0])).is_err());

    // A duplicate map key is rejected by the init storage data.
    let duplicate = Storage {{
        owner_public_key: owner,
        asset_qty_map: vec![(asset, quantity), (asset, quantity)],
        limits,
        holdings: vec![(asset, holding)],
    }};
    assert!(duplicate.to_init_storage_data().is_err());

    use custom_bindings::{{Balance, Mode}};

    let account_id =
        AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
    let balance = Balance {{ amount: Felt::from_u32(5), locked: true }};
    let custom = custom_bindings::Storage {{
        mode: Mode::Capped(7),
        balances: vec![(account_id, balance.clone())],
        limit: Some(3),
    }};
    let data = custom.to_init_storage_data().unwrap();
    assert_eq!(value_word(&data, custom_bindings::Storage::MODE_SLOT), word([1, 7, 0, 0]));
    assert_eq!(value_word(&data, custom_bindings::Storage::LIMIT_SLOT), word([1, 3, 0, 0]));
    let entries = map_words(&data, custom_bindings::Storage::BALANCES_SLOT);
    let account_word = Word::new([
        account_id.prefix().as_felt(),
        account_id.suffix(),
        Felt::ZERO,
        Felt::ZERO,
    ]);
    assert_eq!(entries, [(account_word, word([5, 1, 0, 0]))]);

    assert_eq!(
        custom_bindings::Storage::decode_mode(word([1, 7, 0, 0])).unwrap(),
        Mode::Capped(7),
    );
    assert_eq!(custom_bindings::Storage::decode_mode(word([0, 0, 0, 0])).unwrap(), Mode::Open);
    assert!(custom_bindings::Storage::decode_mode(word([0, 7, 0, 0])).is_err());
    assert_eq!(custom_bindings::Storage::decode_limit(word([0, 0, 0, 0])).unwrap(), None);
    let (key, value) = entries[0];
    assert_eq!(
        custom_bindings::Storage::decode_balances_entry(key, value).unwrap(),
        (account_id, balance),
    );
}}
"#,
        project_dir = storage_dir.to_string_lossy(),
        custom_path = custom_path.to_string_lossy(),
    );

    common::run_consumer(temp.path(), "storage-bindings-consumer", &source);
}
