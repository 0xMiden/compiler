//! Off-chain serialization/deserialization tests.
//!
//! These tests verify the correctness of the off-chain `ToFeltRepr` and `FromFeltRepr`
//! implementations without involving on-chain execution.

use miden_core::Felt;
use miden_felt_repr_offchain::{FeltReader, FromFeltRepr, ToFeltRepr};

/// Test struct for off-chain serialization tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TwoFelts {
    a: Felt,
    b: Felt,
}

#[test]
fn serialization() {
    let value = TwoFelts {
        a: Felt::new(12345),
        b: Felt::new(67890),
    };

    let felts = value.to_felt_repr();

    assert_eq!(felts.len(), 2);
    assert_eq!(felts[0], Felt::new(12345));
    assert_eq!(felts[1], Felt::new(67890));
}

#[test]
fn deserialization() {
    let felts = [Felt::new(12345), Felt::new(67890)];

    let mut reader = FeltReader::new(&felts);
    let value = TwoFelts::from_felt_repr(&mut reader);

    assert_eq!(value.a, Felt::new(12345));
    assert_eq!(value.b, Felt::new(67890));
}

#[test]
fn roundtrip() {
    let original = TwoFelts {
        a: Felt::new(12345),
        b: Felt::new(67890),
    };

    let felts = original.to_felt_repr();
    let mut reader = FeltReader::new(&felts);
    let result = TwoFelts::from_felt_repr(&mut reader);

    assert_eq!(result, original);
}
