//! Off-chain serialization/deserialization tests.
//!
//! These tests verify the correctness of `ToFeltRepr` and `FromFeltRepr` implementations without
//! involving on-chain execution.

use miden_field::{Felt, PrimeField64};
use miden_field_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

/// Serializes `value` off-chain and deserializes it back, asserting equality.
fn assert_roundtrip<T>(value: &T)
where
    T: ToFeltRepr + FromFeltRepr + PartialEq + core::fmt::Debug,
{
    let felts = value.to_felt_repr();
    let mut reader = FeltReader::new(&felts);
    let roundtrip = <T as FromFeltRepr>::from_felt_repr(&mut reader);
    assert_eq!(roundtrip, *value);
}

/// Test struct for off-chain serialization tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TwoFelts {
    a: Felt,
    b: Felt,
}

#[test]
fn test_serialization() {
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
fn test_deserialization() {
    let felts = [Felt::new(12345), Felt::new(67890)];

    let mut reader = FeltReader::new(&felts);
    let value = TwoFelts::from_felt_repr(&mut reader);

    assert_eq!(value.a, Felt::new(12345));
    assert_eq!(value.b, Felt::new(67890));
}

#[test]
fn test_roundtrip() {
    let original = TwoFelts {
        a: Felt::new(12345),
        b: Felt::new(67890),
    };

    assert_roundtrip(&original);
}

/// Test struct containing multiple non-`Felt` fields.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct MixedStruct {
    a: Felt,
    b: u32,
    c: bool,
    d: u8,
}

#[test]
fn test_struct_roundtrip_mixed_types() {
    let original = MixedStruct {
        a: Felt::new(11),
        b: 22,
        c: true,
        d: 33,
    };

    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 4);
    assert_eq!(felts[0], Felt::new(11));
    assert_eq!(felts[1], Felt::new(22));
    assert_eq!(felts[2], Felt::new(1));
    assert_eq!(felts[3], Felt::new(33));

    assert_roundtrip(&original);
}

/// Inner struct used by nested struct/enum tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct Inner {
    x: Felt,
    y: u64,
}

/// Outer struct containing nested `Inner`.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct Outer {
    head: u8,
    inner: Inner,
    tail: bool,
}

#[test]
fn test_struct_roundtrip_nested() {
    let original = Outer {
        head: 1,
        inner: Inner {
            x: Felt::new(2),
            y: 3,
        },
        tail: false,
    };

    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 5);
    assert_eq!(felts[0], Felt::new(1));
    assert_eq!(felts[1], Felt::new(2));
    assert_eq!(felts[2], Felt::new(3));
    assert_eq!(felts[3], Felt::new(0));
    assert_eq!(felts[4], Felt::new(0));

    assert_roundtrip(&original);
}

/// Unit-only enum test type.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
enum SimpleEnum {
    A,
    B,
    C,
}

#[test]
fn test_enum_roundtrip_unit() {
    let original = SimpleEnum::B;
    let felts = original.to_felt_repr();
    assert_eq!(felts, vec![Felt::new(1)]);
    assert_roundtrip(&original);
}

/// Mixed enum with different shapes to exercise tags and payload encoding.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
enum MixedEnum {
    Unit,
    Pair(Felt, u32),
    Struct { n: u64, flag: bool },
    Nested(Inner),
}

#[test]
fn test_enum_roundtrip_tuple_variant() {
    let original = MixedEnum::Pair(Felt::new(7), 8);
    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 3);
    assert_eq!(felts[0], Felt::new(1));
    assert_eq!(felts[1], Felt::new(7));
    assert_eq!(felts[2], Felt::new(8));
    assert_roundtrip(&original);
}

#[test]
fn test_enum_roundtrip_struct_variant() {
    let original = MixedEnum::Struct { n: 9, flag: true };
    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 4);
    assert_eq!(felts[0], Felt::new(2));
    assert_eq!(felts[1], Felt::new(9));
    assert_eq!(felts[2], Felt::new(0));
    assert_eq!(felts[3], Felt::new(1));
    assert_roundtrip(&original);
}

/// Struct with an enum field to exercise struct+enum composition.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct WithEnum {
    prefix: Felt,
    msg: MixedEnum,
    suffix: u32,
}

#[test]
fn test_struct_with_enum_roundtrip() {
    let original = WithEnum {
        prefix: Felt::new(10),
        msg: MixedEnum::Nested(Inner {
            x: Felt::new(11),
            y: 12,
        }),
        suffix: 13,
    };

    // prefix (1) + msg(tag=3 + Inner(3)) + suffix (1) = 6 felts
    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 6);
    assert_eq!(felts[0], Felt::new(10));
    assert_eq!(felts[1], Felt::new(3));
    assert_eq!(felts[2], Felt::new(11));
    assert_eq!(felts[3], Felt::new(12));
    assert_eq!(felts[4], Felt::new(0));
    assert_eq!(felts[5], Felt::new(13));

    assert_roundtrip(&original);
}

/// Nested enum shape which wraps a struct that itself contains an enum.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
enum Top {
    None,
    Some(WithEnum),
}

#[test]
fn test_enum_nested_with_struct_roundtrip() {
    let original = Top::Some(WithEnum {
        prefix: Felt::new(21),
        msg: MixedEnum::Struct { n: 22, flag: false },
        suffix: 23,
    });

    // tag (1) + WithEnum(prefix 1 + msg 4 + suffix 1) = 7 felts
    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 7);
    assert_roundtrip(&original);
}

/// Test struct containing an `Option` field.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct WithOption {
    prefix: Felt,
    maybe: Option<u32>,
    suffix: bool,
}

#[test]
fn test_struct_roundtrip_option_some() {
    let original = WithOption {
        prefix: Felt::new(5),
        maybe: Some(42),
        suffix: true,
    };

    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 4);
    assert_eq!(felts, vec![Felt::new(5), Felt::new(1), Felt::new(42), Felt::new(1)]);

    assert_roundtrip(&original);
}

#[test]
fn test_struct_roundtrip_option_none() {
    let original = WithOption {
        prefix: Felt::new(7),
        maybe: None,
        suffix: false,
    };

    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 3);
    assert_eq!(felts, vec![Felt::new(7), Felt::new(0), Felt::new(0)]);

    assert_roundtrip(&original);
}

/// Test struct containing a `Vec` field.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct WithVec {
    prefix: Felt,
    items: Vec<u8>,
    suffix: bool,
}

#[test]
fn test_struct_roundtrip_vec_non_empty() {
    let original = WithVec {
        prefix: Felt::new(9),
        items: vec![1, 2, 3],
        suffix: true,
    };

    // prefix (1) + Vec<u8> (len 1 + 3 elems) + suffix (1)
    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 6);
    assert_eq!(
        felts,
        vec![
            Felt::new(9),
            Felt::new(3),
            Felt::new(1),
            Felt::new(2),
            Felt::new(3),
            Felt::new(1),
        ]
    );

    assert_roundtrip(&original);
}

#[test]
fn test_struct_roundtrip_vec_empty() {
    let original = WithVec {
        prefix: Felt::new(10),
        items: vec![],
        suffix: false,
    };

    let felts = original.to_felt_repr();
    assert_eq!(felts.len(), 3);
    assert_eq!(felts, vec![Felt::new(10), Felt::new(0), Felt::new(0)]);

    assert_roundtrip(&original);
}

/// Test tuple struct serialization/round-trip.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TupleStruct(u32, bool, Felt);

#[test]
fn test_tuple_struct_roundtrip() {
    let original = TupleStruct(22, true, Felt::new(33));
    let felts = original.to_felt_repr();

    assert_eq!(felts, vec![Felt::new(22), Felt::new(1), Felt::new(33)]);
    assert_roundtrip(&original);
}

#[test]
fn test_u64_roundtrip_uses_u32_limbs() {
    let test_cases: [u64; 6] =
        [0, 1, u32::MAX as u64, (u32::MAX as u64) << 32, 0x1122_3344_5566_7788, u64::MAX];

    for value in test_cases {
        let felts = value.to_felt_repr();
        assert_eq!(felts.len(), 2);

        let expected_lo = value & 0xffff_ffff;
        let expected_hi = value >> 32;
        assert_eq!(felts[0].as_canonical_u64(), expected_lo);
        assert_eq!(felts[1].as_canonical_u64(), expected_hi);

        let mut reader = FeltReader::new(&felts);
        let roundtripped = u64::from_felt_repr(&mut reader);
        assert_eq!(roundtripped, value);
    }
}
