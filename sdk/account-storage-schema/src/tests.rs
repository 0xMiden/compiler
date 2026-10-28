//! Unit tests for the account storage schema reader.

use crate::{
    ASSET_AMOUNT_FQN, AccountStorageSchema, PrimitiveType, SchemaTypeKind, StorageSlotKind,
    WORD_FQN,
};

/// The embedded core-types package, reduced to the types the tests use.
const CORE_TYPES: &str = r#"
package miden:base@1.0.0 {
    interface core-types {
        record felt {
            inner: f32,
        }

        record word {
            a: felt,
            b: felt,
            c: felt,
            d: felt,
        }

        record asset-amount {
            inner: felt
        }
    }
}
"#;

/// Wraps an `account-storage` interface body in a schema package with the core types.
fn schema_wit(package: &str, body: &str) -> String {
    format!(
        "package {package};\n\nuse miden:base/core-types@1.0.0;\n\ninterface account-storage \
         {{\n{body}\n}}\n{CORE_TYPES}"
    )
}

/// Resolves a schema of the `miden:example-storage-schema` package.
fn resolve(body: &str) -> crate::Result<AccountStorageSchema> {
    AccountStorageSchema::from_wit_text(&schema_wit("miden:example-storage-schema", body))
}

/// Returns the error message of a schema that must fail to resolve.
fn reject(body: &str) -> String {
    resolve(body).expect_err("schema must be rejected").to_string()
}

#[test]
fn reads_value_and_map_slots_with_leaf_names_and_docs() {
    let schema = resolve(
        r#"
    use core-types.{asset-amount, word};

    /// An example account.
    record storage {
        /// Public key authorized to update the stored asset quantities.
        %owner-public-key: word,
        %asset-qty-map: list<tuple<word, asset-amount>>,
    }
"#,
    )
    .unwrap();

    let leaves = schema.slots().iter().map(|slot| slot.leaf()).collect::<Vec<_>>();
    assert_eq!(leaves, ["owner_public_key", "asset_qty_map"]);

    let owner = schema.slot("owner_public_key").unwrap();
    assert_eq!(
        owner.docs(),
        Some("Public key authorized to update the stored asset quantities.")
    );
    let StorageSlotKind::Value(value) = owner.kind() else {
        panic!("owner-public-key must be a value slot");
    };
    assert_eq!(value.fqn(), Some(WORD_FQN));
    assert_eq!(value.layout().fixed_width(), Some(4));

    let map = schema.slot("asset_qty_map").unwrap();
    assert_eq!(map.docs(), None);
    let StorageSlotKind::Map { key, value } = map.kind() else {
        panic!("asset-qty-map must be a map slot");
    };
    assert_eq!(key.fqn(), Some(WORD_FQN));
    assert_eq!(value.fqn(), Some(ASSET_AMOUNT_FQN));
    assert_eq!(value.layout().fixed_width(), Some(1));

    // The leaf is the snake-case name; the WIT spelling does not match.
    assert!(schema.slot("owner-public-key").is_none());
    assert!(schema.custom_type_fqns().is_empty());
    assert!(schema.wit_text().starts_with("package miden:example-storage-schema;"));
}

#[test]
fn reads_custom_map_keys_and_values() {
    let schema = resolve(
        r#"
    use core-types.{felt};

    record pair {
        left: u32,
        right: u16,
    }

    record balance {
        amount: felt,
        locked: bool,
    }

    record storage {
        balances: list<tuple<pair, balance>>,
    }
"#,
    )
    .unwrap();

    let StorageSlotKind::Map { key, value } = schema.slot("balances").unwrap().kind() else {
        panic!("balances must be a map slot");
    };
    let SchemaTypeKind::Record(fields) = key.kind() else {
        panic!("the key must be a record");
    };
    assert_eq!(fields[1].ty().kind(), &SchemaTypeKind::Primitive(PrimitiveType::U16));
    assert_eq!(key.layout().fixed_width(), Some(2));
    assert_eq!(value.layout().fixed_width(), Some(2));

    let mut fqns = schema.custom_type_fqns().into_iter().collect::<Vec<_>>();
    fqns.sort();
    assert_eq!(
        fqns,
        [
            "miden:example-storage-schema/account-storage.balance",
            "miden:example-storage-schema/account-storage.pair",
        ]
    );
}

#[test]
fn reads_options_in_custom_records() {
    let schema = resolve(
        r#"
    record limit {
        max: option<u32>,
    }

    record storage {
        limit: limit,
        count: u16,
    }
"#,
    )
    .unwrap();

    let StorageSlotKind::Value(limit) = schema.slot("limit").unwrap().kind() else {
        panic!("limit must be a value slot");
    };
    assert_eq!(limit.layout().minimum(), 1);
    assert_eq!(limit.layout().maximum(), 2);
    let StorageSlotKind::Value(count) = schema.slot("count").unwrap().kind() else {
        panic!("count must be a value slot");
    };
    assert_eq!(count.kind(), &SchemaTypeKind::Primitive(PrimitiveType::U16));
}

#[test]
fn rejects_slot_types_wider_than_a_word() {
    let error = reject(
        r#"
    record wide {
        a: u64,
        b: u64,
        c: bool,
    }

    record storage {
        wide: wide,
    }
"#,
    );
    assert!(error.contains("account storage slot `wide`"), "{error}");
    assert!(error.contains("up to 5 felts wide"), "{error}");

    let error = reject(
        r#"
    record storage {
        entries: list<tuple<u32, option<wide-pair>>>,
    }

    record wide-pair {
        a: u64,
        b: u64,
    }
"#,
    );
    assert!(error.contains("map value type"), "{error}");
    assert!(error.contains("up to 5 felts wide"), "{error}");
}

#[test]
fn rejects_lists_and_tuples_outside_a_map_slot() {
    let error = reject(
        r#"
    record holder {
        items: list<tuple<u32, u32>>,
    }

    record storage {
        holder: holder,
    }
"#,
    );
    assert!(error.contains("WIT list"), "{error}");
    assert!(error.contains("account storage slot `holder`"), "{error}");

    let error = reject(
        r#"
    record storage {
        items: list<u32>,
    }
"#,
    );
    assert!(error.contains("is a list, but not a `list<tuple<K, V>>` map"), "{error}");

    let error = reject(
        r#"
    record storage {
        pair: tuple<u32, u32>,
    }
"#,
    );
    assert!(error.contains("WIT tuple"), "{error}");
}

#[test]
fn rejects_map_entries_that_are_not_pairs() {
    let error = reject(
        r#"
    record storage {
        triples: list<tuple<u32, u32, u32>>,
    }
"#,
    );
    assert!(error.contains("has a tuple of 3 types"), "{error}");
}

#[test]
fn rejects_a_versioned_schema_package() {
    let wit = schema_wit(
        "miden:example-storage-schema@1.0.0",
        "    record storage {\n        count: u32,\n    }",
    );
    let error = AccountStorageSchema::from_wit_text(&wit).unwrap_err().to_string();
    assert!(error.contains("must not carry a version"), "{error}");
}

#[test]
fn rejects_an_empty_storage_record() {
    let error = reject("    record storage {}");
    assert!(error.contains("declares at least one slot"), "{error}");
}

#[test]
fn rejects_a_root_that_is_not_a_record() {
    let error = reject("    type storage = u32;");
    assert!(error.contains("must resolve to a record"), "{error}");
}
