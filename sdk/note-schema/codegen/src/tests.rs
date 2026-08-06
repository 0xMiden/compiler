//! Tests for host-profile type generation.

use miden_note_schema::NoteStorageSchema;

use crate::generate_host_types;

const P2ID_SCHEMA: &str = r#"
package example:p2id-schema@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id};

    record p2id-note {
        target-account-id: account-id,
    }

    type storage = p2id-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record account-id { prefix: felt, suffix: felt }
    }
}
"#;

const CUSTOM_SCHEMA: &str = r#"
package example:dex-schema@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id};

    record limit-price {
        numerator: u64,
        denominator: u64,
    }

    variant order-kind {
        market,
        limit(limit-price),
    }

    record dex-note {
        target: account-id,
        kind: order-kind,
    }

    type storage = dex-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record account-id { prefix: felt, suffix: felt }
    }
}
"#;

/// Formats generated tokens as Rust source.
fn generate(wit: &str) -> (String, bool, String) {
    let schema = NoteStorageSchema::from_wit_text(wit).unwrap();
    let generated = generate_host_types(&schema).unwrap();
    let file: syn::File = syn::parse2(generated.tokens().clone()).unwrap();
    (
        prettyplease::unparse(&file),
        generated.has_custom_types(),
        generated.root_ident().to_string(),
    )
}

#[test]
fn maps_protocol_leaf_and_root_type() {
    let (source, has_custom_types, root) = generate(P2ID_SCHEMA);
    assert_eq!(root, "P2idNote");
    assert!(!has_custom_types);
    assert!(source.contains("pub target_account_id: ::miden_protocol::account::AccountId"));
    assert!(source.contains("pub const WIT_FQN"));
    assert!(source.contains("self.prefix().as_felt()"));
}

#[test]
fn derives_native_repr_for_custom_records_and_variants() {
    let (source, has_custom_types, root) = generate(CUSTOM_SCHEMA);
    assert_eq!(root, "DexNote");
    assert!(has_custom_types);
    assert!(source.contains("pub struct LimitPrice"));
    assert!(source.contains("pub enum OrderKind"));
    assert!(source.matches("::miden_field_repr::ToFeltRepr").count() >= 2);
    assert!(source.contains("example:dex-schema/note-storage@1.0.0.limit-price"));
}
