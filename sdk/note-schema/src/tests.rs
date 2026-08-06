//! Unit tests for schema interpretation, building, and decoding.

use miden_field_repr::ToFeltRepr;
use miden_protocol::{account::AccountId, address::NetworkId};

use crate::{
    ACCOUNT_ID_FQN, CodecRegistry, DecodedValueKind, Felt, NoteStorage, NoteStorageSchema,
    SchemaTypeKind,
};

const LAYOUT_SCHEMA: &str = r#"
package example:layout-schema@1.2.3;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id, felt};

    record nested-values {
        /// A wide counter.
        wide-count: u64,
        small-count: u8,
    }

    variant selection {
        empty,
        count(u32),
    }

    record layout-note {
        bedrock: felt,
        nested: nested-values,
        maybe-enabled: option<bool>,
        selected: selection,
        target-account-id: account-id,
    }

    type storage = layout-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt {
            inner: f32,
        }

        record account-id {
            prefix: felt,
            suffix: felt,
        }
    }
}
"#;

/// Returns a valid account ID and its mainnet bech32 form.
fn account_id() -> (AccountId, String) {
    let account_id = AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
    let bech32 = account_id.to_bech32(NetworkId::Mainnet);
    (account_id, bech32)
}

#[test]
fn resolves_alias_fqns_docs_and_structural_widths() {
    let schema = NoteStorageSchema::from_wit_text(LAYOUT_SCHEMA).unwrap();

    assert_eq!(
        schema.root().fqn(),
        Some("example:layout-schema/note-storage@1.2.3.layout-note")
    );
    assert_eq!(schema.layout().minimum(), 8);
    assert_eq!(schema.layout().maximum(), 10);
    assert_eq!(schema.layout().fixed_width(), None);

    let SchemaTypeKind::Record(root_fields) = schema.root().kind() else {
        panic!("storage root must be a record");
    };
    let nested = root_fields.iter().find(|field| field.name() == "nested").unwrap();
    let SchemaTypeKind::Record(nested_fields) = nested.ty().kind() else {
        panic!("nested must be a record");
    };
    assert_eq!(nested_fields[0].docs(), Some("A wide counter."));

    let account_id = root_fields.iter().find(|field| field.name() == "target-account-id").unwrap();
    assert_eq!(account_id.ty().fqn(), Some(ACCOUNT_ID_FQN));
    assert_eq!(account_id.ty().layout().fixed_width(), Some(2));
}

#[test]
fn wit_reader_reports_missing_schema_surface() {
    let error = NoteStorageSchema::from_wit_text(
        "package example:missing@1.0.0; interface other { record value {} }",
    )
    .err()
    .unwrap()
    .to_string();

    assert!(error.contains("does not define the `note-storage` interface"));
}

#[test]
fn builder_normalizes_paths_and_uses_declaration_order() {
    let schema = NoteStorageSchema::from_wit_text(LAYOUT_SCHEMA).unwrap();
    let (account_id, bech32) = account_id();
    let storage = schema
        .builder()
        .set("bedrock", "5")
        .unwrap()
        .set("nested.wide_count", "4294967298")
        .unwrap()
        .set("nested.small-count", "7")
        .unwrap()
        .set("maybe_enabled", "some(true)")
        .unwrap()
        .set("selected", "count(9)")
        .unwrap()
        .set("target_account_id", &bech32)
        .unwrap()
        .build()
        .unwrap();

    let expected = NoteStorage::new(vec![
        Felt::from_u32(5),
        Felt::from_u32(2),
        Felt::from_u32(1),
        Felt::from_u32(7),
        Felt::ONE,
        Felt::ONE,
        Felt::ONE,
        Felt::from_u32(9),
        account_id.prefix().as_felt(),
        account_id.suffix(),
    ])
    .unwrap();
    assert_eq!(storage, expected);

    let decoded = schema.decode(&storage).unwrap();
    assert_eq!(
        decoded.field("nested").unwrap().field("wide_count").unwrap().to_string(),
        "4294967298"
    );
    assert_eq!(decoded.field("maybe_enabled").unwrap().to_string(), "some(true)");
    assert_eq!(decoded.field("selected").unwrap().to_string(), "count(9)");
    assert_eq!(decoded.field("target_account_id").unwrap().to_string(), bech32);
}

#[test]
fn decoder_uses_structural_fallback_without_a_codec() {
    let schema = NoteStorageSchema::from_wit_text(LAYOUT_SCHEMA).unwrap();
    let (account_id, bech32) = account_id();
    let storage = schema
        .builder()
        .set("bedrock", "5")
        .unwrap()
        .set("nested.wide-count", "0")
        .unwrap()
        .set("nested.small-count", "0")
        .unwrap()
        .set("maybe-enabled", "none")
        .unwrap()
        .set("selected", "empty")
        .unwrap()
        .set("target-account-id", &bech32)
        .unwrap()
        .build()
        .unwrap();

    let decoded = schema.decode_with_registry(&storage, &CodecRegistry::new()).unwrap();
    let account = decoded.field("target-account-id").unwrap();
    let DecodedValueKind::Record(fields) = account.kind() else {
        panic!("account-id must use its structural record fallback");
    };
    assert_eq!(fields[0].name(), Some("prefix"));
    assert_eq!(fields[0].to_string(), account_id.prefix().as_u64().to_string());
    assert_eq!(fields[1].name(), Some("suffix"));
    assert_eq!(fields[1].to_string(), account_id.suffix().as_canonical_u64().to_string());
}

#[test]
fn builder_reports_missing_unknown_conflicting_and_range_errors() {
    let schema = NoteStorageSchema::from_wit_text(LAYOUT_SCHEMA).unwrap();
    let (_, bech32) = account_id();

    let missing = schema.builder().build().unwrap_err().to_string();
    assert!(missing.contains("missing note storage value for `bedrock`"));

    let unknown = schema.builder().set("nested.unknown", "1").err().unwrap().to_string();
    assert!(unknown.contains("has no field named `unknown`"));

    let conflict = schema
        .builder()
        .set("nested", "value")
        .unwrap()
        .set("nested.wide-count", "1")
        .err()
        .unwrap()
        .to_string();
    assert!(conflict.contains("conflicts"));

    let range = schema
        .builder()
        .set("bedrock", "0")
        .unwrap()
        .set("nested.wide-count", "0")
        .unwrap()
        .set("nested.small-count", "256")
        .unwrap()
        .set("maybe-enabled", "none")
        .unwrap()
        .set("selected", "empty")
        .unwrap()
        .set("target-account-id", &bech32)
        .unwrap()
        .build()
        .unwrap_err()
        .to_string();
    assert!(range.contains("u8 value `256` is out of range"));
}

#[test]
fn decoder_rejects_invalid_option_and_variant_tags() {
    let schema = NoteStorageSchema::from_wit_text(LAYOUT_SCHEMA).unwrap();
    let (account_id, _) = account_id();
    let suffix = [account_id.prefix().as_felt(), account_id.suffix()];

    let invalid_option = NoteStorage::new(
        [
            vec![Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::from_u32(2)],
            vec![Felt::ZERO],
            suffix.to_vec(),
        ]
        .concat(),
    )
    .unwrap();
    assert!(
        schema
            .decode(&invalid_option)
            .unwrap_err()
            .to_string()
            .contains("invalid option tag")
    );

    let invalid_variant = NoteStorage::new(
        [
            vec![Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ZERO],
            vec![Felt::from_u32(2)],
            suffix.to_vec(),
        ]
        .concat(),
    )
    .unwrap();
    assert!(
        schema
            .decode(&invalid_variant)
            .unwrap_err()
            .to_string()
            .contains("invalid variant tag 2")
    );
}

#[test]
fn u64_layout_uses_shared_low_then_high_limb_encoding() {
    let value = 0x1234_5678_90ab_cdefu64;
    assert_eq!(value.to_felt_repr(), [Felt::from_u32(0x90ab_cdef), Felt::from_u32(0x1234_5678)]);
}
