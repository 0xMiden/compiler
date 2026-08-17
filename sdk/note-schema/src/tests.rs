//! Unit tests for schema interpretation, building, and decoding.

use miden_field_repr::ToFeltRepr;
use miden_protocol::{account::AccountId, address::NetworkId};

use crate::{
    ACCOUNT_ID_FQN, CodecRegistry, DecodedValueKind, Felt, MAX_NOTE_STORAGE_SCHEMA_BYTES,
    MAX_NOTE_STORAGE_SCHEMA_DEPTH, MAX_NOTE_STORAGE_SCHEMA_FELTS, MAX_NOTE_STORAGE_SCHEMA_TYPES,
    NoteStorage, NoteStorageSchema, SchemaTypeKind,
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

const NESTED_CONSTRUCTOR_SCHEMA: &str = r#"
package example:nested-constructors@1.0.0;

interface note-storage {
    record payload {
        value: u64,
    }

    variant selection {
        empty,
        nested(payload),
    }

    record constructor-note {
        maybe-payload: option<payload>,
        selected: selection,
    }

    type storage = constructor-note;
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
fn reader_rejects_noncanonical_native_core_type_shapes() {
    for (type_name, definitions) in [
        ("felt", "record felt { inner: u32 }"),
        ("word", "record felt { inner: f32 } record word { a: felt, b: felt, c: felt }"),
        (
            "account-id",
            "record felt { inner: f32 } record account-id { suffix: felt, prefix: felt }",
        ),
        ("asset-amount", "record felt { inner: f32 } record asset-amount { inner: u64 }"),
    ] {
        let wit = format!(
            r#"
package example:bad-core-shape@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {{
    use core-types.{{{type_name}}};
    record bad-note {{ value: {type_name} }}
    type storage = bad-note;
}}

package miden:base@1.0.0 {{
    interface core-types {{ {definitions} }}
}}
"#
        );
        let error = NoteStorageSchema::from_wit_text(&wit).err().unwrap().to_string();
        assert!(
            error.contains(&format!("miden:base/core-types@1.0.0.{type_name}")),
            "unexpected error for {type_name}: {error}"
        );
        assert!(error.contains("pinned canonical shape"));
    }
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

    let decoded = schema.decode_with_registry(&storage, &CodecRegistry::empty()).unwrap();
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
fn builder_rejects_nested_record_constructor_paths() {
    let schema = NoteStorageSchema::from_wit_text(NESTED_CONSTRUCTOR_SCHEMA).unwrap();

    let option_error =
        schema.builder().set("maybe_payload", "some(value)").err().unwrap().to_string();
    assert!(option_error.contains("path `maybe-payload`"));
    assert!(option_error.contains("option with record payload"));
    assert!(option_error.contains("does not support nested record constructors"));

    let variant_error =
        schema.builder().set("selected", "nested(value)").err().unwrap().to_string();
    assert!(variant_error.contains("path `selected`"));
    assert!(variant_error.contains("variant case `nested` with a record payload"));
    assert!(variant_error.contains("does not support nested record constructors"));
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

#[test]
fn repeated_pair_schema_is_memoized_and_fails_fast_at_width_limit() {
    let schema = NoteStorageSchema::from_wit_text(&repeated_pair_schema(8)).unwrap();
    let SchemaTypeKind::Record(fields) = schema.root().kind() else {
        panic!("the repeated-pair root must be a record");
    };
    assert!(
        core::ptr::eq(fields[0].ty(), fields[1].ty()),
        "both references to the completed named type must share one memoized node"
    );

    let error =
        NoteStorageSchema::from_wit_text(&repeated_pair_schema(MAX_NOTE_STORAGE_SCHEMA_DEPTH / 2))
            .err()
            .expect("an over-wide repeated-pair schema must fail")
            .to_string();
    assert!(error.contains("maximum width"), "unexpected repeated-pair error: {error}");
    assert!(
        error.contains(&MAX_NOTE_STORAGE_SCHEMA_FELTS.to_string()),
        "the protocol width must be present in the diagnostic: {error}"
    );
}

#[test]
fn deep_schema_chain_fails_fast_at_depth_limit() {
    let error =
        NoteStorageSchema::from_wit_text(&deep_chain_schema(MAX_NOTE_STORAGE_SCHEMA_DEPTH + 1))
            .err()
            .expect("an over-deep schema must fail")
            .to_string();

    assert!(error.contains("nesting depth"), "unexpected deep-chain error: {error}");
    assert!(
        error.contains(&MAX_NOTE_STORAGE_SCHEMA_DEPTH.to_string()),
        "the nesting limit must be present in the diagnostic: {error}"
    );
}

#[test]
fn schema_reader_enforces_documented_byte_type_and_root_width_limits() {
    let oversized = " ".repeat(MAX_NOTE_STORAGE_SCHEMA_BYTES + 1);
    let byte_error = NoteStorageSchema::from_wit_text(&oversized)
        .err()
        .expect("an oversized schema document must fail")
        .to_string();
    assert!(byte_error.contains("schema section"));
    assert!(byte_error.contains(&MAX_NOTE_STORAGE_SCHEMA_BYTES.to_string()));

    let mut too_many_types =
        String::from("package example:many-types@1.0.0; interface note-storage { ");
    for index in 0..=MAX_NOTE_STORAGE_SCHEMA_TYPES {
        too_many_types.push_str(&format!("type t{index} = u8; "));
    }
    too_many_types.push_str("record root { value: u8 } type storage = root; }");
    let type_error = NoteStorageSchema::from_wit_text(&too_many_types)
        .err()
        .expect("a schema with too many types must fail")
        .to_string();
    assert!(type_error.contains("WIT types"), "unexpected type-count error: {type_error}");
    assert!(type_error.contains(&MAX_NOTE_STORAGE_SCHEMA_TYPES.to_string()));

    let mut wide_root =
        String::from("package example:wide-root@1.0.0; interface note-storage { record root { ");
    for index in 0..=MAX_NOTE_STORAGE_SCHEMA_FELTS {
        wide_root.push_str(&format!("field-{index}: u8, "));
    }
    wide_root.push_str("} type storage = root; }");
    let width_error = NoteStorageSchema::from_wit_text(&wide_root)
        .err()
        .expect("an over-wide schema root must fail")
        .to_string();
    assert!(width_error.contains("maximum width"), "unexpected width error: {width_error}");
    assert!(width_error.contains(&MAX_NOTE_STORAGE_SCHEMA_FELTS.to_string()));
}

/// Builds a linear-size WIT DAG whose resolved layout doubles at each level.
fn repeated_pair_schema(levels: usize) -> String {
    let mut wit = String::from(
        "package example:repeated-pair@1.0.0; interface note-storage { record t0 { value: u8 } ",
    );
    for level in 1..=levels {
        let previous = level - 1;
        wit.push_str(&format!("record t{level} {{ left: t{previous}, right: t{previous} }} "));
    }
    wit.push_str(&format!("type storage = t{levels}; }}"));
    wit
}

/// Builds a WIT record chain with one nested named type per level.
fn deep_chain_schema(levels: usize) -> String {
    let mut wit = String::from(
        "package example:deep-chain@1.0.0; interface note-storage { record t0 { value: u8 } ",
    );
    for level in 1..=levels {
        let previous = level - 1;
        wit.push_str(&format!("record t{level} {{ value: t{previous} }} "));
    }
    wit.push_str(&format!("type storage = t{levels}; }}"));
    wit
}
