//! Tests for macro expansion.

use miden_account_storage_schema::AccountStorageSchema;
use miden_protocol::{
    account::{
        AccountComponentMetadata, StorageSlotName,
        component::{SchemaType, StorageSchema, StorageSlotSchema},
    },
    utils::serde::Serializable,
};
use midenc_expect_test::expect_file;
use syn::LitStr;

use crate::{expand_from_wit_text, scope_ident, storage};

const P2ID_SCHEMA: &str = r#"
package example:p2id-schema;

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
package example:dex-schema;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id};

    /// A ratio used as an order limit.
    record limit-price {
        numerator: u64,
        denominator: u64,
    }

    /// Selects order execution.
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

/// The account storage schema of `examples/storage-example`, with the core types it uses.
const STORAGE_EXAMPLE_SCHEMA: &str = r#"
package miden:storage-example-storage-schema;

use miden:base/core-types@1.0.0;

interface account-storage {
    use core-types.{asset-amount, word};

    /// An example account demonstrating storage value and map usage.
    record storage {
        /// Public key authorized to update the stored asset quantities.
        %owner-public-key: word,
        /// A map from asset id word to quantity held by the account.
        %asset-qty-map: list<tuple<word, asset-amount>>,
    }
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record word { a: felt, b: felt, c: felt, d: felt }
        record asset-amount { inner: felt }
    }
}
"#;

/// Formats one WIT-text macro expansion as Rust source.
fn expand(wit: &str) -> String {
    let literal = LitStr::new(wit, proc_macro2::Span::call_site());
    let tokens = expand_from_wit_text(&literal).unwrap();
    unparse(tokens)
}

/// Formats generated tokens as Rust source.
fn unparse(tokens: proc_macro2::TokenStream) -> String {
    let file: syn::File = syn::parse2(tokens).unwrap();
    prettyplease::unparse(&file)
}

/// Returns protocol metadata with the storage-example slots under the component's namespace.
fn storage_example_metadata() -> AccountComponentMetadata {
    let namespace = "miden::storage_example::foo";
    let slots = [
        (
            StorageSlotName::new(format!("{namespace}::owner_public_key")).unwrap(),
            StorageSlotSchema::value("owner public key", SchemaType::native_word()),
        ),
        (
            StorageSlotName::new(format!("{namespace}::asset_qty_map")).unwrap(),
            StorageSlotSchema::map(
                "asset quantity map",
                SchemaType::native_word(),
                SchemaType::native_felt(),
            ),
        ),
    ];
    AccountComponentMetadata::new("storage-example")
        .with_storage_schema(StorageSchema::new(slots).unwrap())
}

/// Formats the storage bindings of a schema read with `metadata` as Rust source.
fn expand_storage(wit: &str, metadata: &AccountComponentMetadata) -> syn::Result<String> {
    let schema = AccountStorageSchema::from_wit_text_and_metadata(wit, metadata).unwrap();
    let span = proc_macro2::Span::call_site();
    storage::expand_storage_schema(
        &schema,
        &metadata.to_bytes(),
        &syn::parse_quote!(::miden_bindings),
        span,
        &scope_ident("storage-example"),
    )
    .map(unparse)
}

#[test]
fn expands_p2id_schema_golden() {
    expect_file!["../expected/p2id.rs"].assert_eq(&expand(P2ID_SCHEMA));
}

#[test]
fn expands_custom_schema_golden() {
    expect_file!["../expected/custom.rs"].assert_eq(&expand(CUSTOM_SCHEMA));
}

#[test]
fn expands_storage_example_schema_golden() {
    let expanded = expand_storage(STORAGE_EXAMPLE_SCHEMA, &storage_example_metadata()).unwrap();
    expect_file!["../expected/storage.rs"].assert_eq(&expanded);
}

#[test]
fn rejects_storage_names_that_collide_with_the_root() {
    let slot = STORAGE_EXAMPLE_SCHEMA.replace("%owner-public-key", "storage");
    let namespace = "miden::storage_example::foo";
    let metadata = AccountComponentMetadata::new("storage-example").with_storage_schema(
        StorageSchema::new([
            (
                StorageSlotName::new(format!("{namespace}::storage")).unwrap(),
                StorageSlotSchema::value("", SchemaType::native_word()),
            ),
            (
                StorageSlotName::new(format!("{namespace}::asset_qty_map")).unwrap(),
                StorageSlotSchema::map("", SchemaType::native_word(), SchemaType::native_felt()),
            ),
        ])
        .unwrap(),
    );
    let error = expand_storage(&slot, &metadata).unwrap_err().to_string();
    assert!(error.contains("slot `storage` collides"), "{error}");

    // A custom type of another interface may have the root's name.
    let colliding = STORAGE_EXAMPLE_SCHEMA
        .replace(
            "interface account-storage {",
            "interface types {\n    record storage { inner: u32 }\n}\n\ninterface account-storage \
             {\n    use types.{storage as owner};",
        )
        .replace("%owner-public-key: word", "%owner-public-key: owner");
    let error = expand_storage(&colliding, &storage_example_metadata()).unwrap_err().to_string();
    assert!(error.contains("maps to Rust type `Storage`"), "{error}");
}

#[test]
fn wit_text_bindings_reject_an_account_storage_schema() {
    let literal = LitStr::new(STORAGE_EXAMPLE_SCHEMA, proc_macro2::Span::call_site());
    let error = expand_from_wit_text(&literal).unwrap_err().to_string();
    assert!(
        error.contains("needs the slot names of its package's protocol metadata"),
        "{error}"
    );
}
