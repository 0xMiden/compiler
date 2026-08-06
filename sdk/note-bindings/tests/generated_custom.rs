//! Type-checks and exercises bindings generated from custom WIT types.

use std::collections::BTreeMap;

use miden_field::Felt;
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_note_schema::CodecRegistry;

miden_note_bindings::from_wit_text!(
    r#"
package example:custom-schema@1.0.0;

interface note-storage {
    record limit-price {
        numerator: u64,
        denominator: u64,
    }

    variant order-kind {
        market,
        limit(limit-price),
    }

    record custom-note {
        price: limit-price,
        kind: order-kind,
    }

    type storage = custom-note;
}
"#
);

/// Requires a type to carry both native felt-repr traits.
fn assert_native_felt_repr<T: ToFeltRepr + FromFeltRepr>() {}

#[test]
fn custom_types_have_native_repr_and_typed_round_trips() {
    assert_native_felt_repr::<LimitPrice>();
    assert_native_felt_repr::<OrderKind>();
    assert_native_felt_repr::<CustomNote>();

    let value = CustomNote {
        price: LimitPrice {
            numerator: 3,
            denominator: 2,
        },
        kind: OrderKind::Limit(LimitPrice {
            numerator: 5,
            denominator: 4,
        }),
    };
    let storage = value.to_note_storage().unwrap();
    assert_eq!(
        storage.items(),
        &[
            Felt::from_u32(3),
            Felt::ZERO,
            Felt::from_u32(2),
            Felt::ZERO,
            Felt::ONE,
            Felt::from_u32(5),
            Felt::ZERO,
            Felt::from_u32(4),
            Felt::ZERO,
        ]
    );
    assert_eq!(CustomNote::from_note_storage(&storage).unwrap(), value);

    let codecs = CodecRegistry::default();
    value.validate_with(&codecs).unwrap();
    assert_eq!(
        value.display_with(&codecs).unwrap(),
        "{price: {numerator: 3, denominator: 2}, kind: limit({numerator: 5, denominator: 4})}"
    );
}

#[test]
fn custom_record_string_paths_use_the_registry_parameter() {
    let mut values = BTreeMap::new();
    values.insert("price.numerator".to_owned(), "7".to_owned());
    values.insert("price.denominator".to_owned(), "6".to_owned());
    values.insert("kind".to_owned(), "market".to_owned());

    let value = CustomNote::from_str_values(&values, &CodecRegistry::default()).unwrap();
    assert_eq!(
        value,
        CustomNote {
            price: LimitPrice {
                numerator: 7,
                denominator: 6,
            },
            kind: OrderKind::Market,
        }
    );
}
