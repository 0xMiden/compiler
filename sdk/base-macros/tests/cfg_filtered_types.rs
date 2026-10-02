//! `cfg` and `cfg_attr` must be evaluated before the macros inspect type fields.
#![allow(clippy::non_minimal_cfg)]

use core::convert::TryFrom;

use miden_base_macros::{export_type, note};

extern crate self as miden;

pub use miden_field::Felt;

pub mod felt_repr {
    pub use miden_field_repr::{FeltReader, FeltReprError, FeltWriter, FromFeltRepr, ToFeltRepr};
}

pub mod active_note {
    pub trait ActiveNote {}
}

#[export_type]
struct Filtered {
    present: u32,
    #[cfg(any())]
    absent: MissingType,
    #[cfg_attr(all(), cfg(any()))]
    also_absent: MissingType,
}

#[derive(Debug, PartialEq, Eq, miden_field_repr::FromFeltRepr, miden_field_repr::ToFeltRepr)]
#[export_type]
enum Choice {
    #[cfg(any())]
    Absent(u32),
    #[cfg(all())]
    Present,
    #[cfg_attr(all(), cfg(any()))]
    AlsoAbsent(u32),
}

#[derive(Debug)]
#[note]
struct NoteInputs {
    choice: Choice,
    present: u32,
    #[cfg(any())]
    absent: MissingType,
    #[cfg_attr(all(), cfg(any()))]
    also_absent: MissingType,
}

#[test]
fn conditional_fields_are_absent_from_shapes_schema_and_encoding() {
    assert_eq!(Filtered { present: 3 }.present, 3);
    let encoded = miden_field_repr::ToFeltRepr::to_felt_repr(&Choice::Present);
    assert_eq!(encoded, vec![Felt::new(0).unwrap()]);
    let record_shape = Filtered::__MIDEN_EXPORT_TYPE_SHAPE;
    let enum_shape = Choice::__MIDEN_EXPORT_TYPE_SHAPE;
    assert!(record_shape.contains("present"));
    assert!(!record_shape.contains("absent"));
    assert!(enum_shape.contains("present"));
    assert!(!enum_shape.contains("absent"));

    let schema = core::str::from_utf8(&__MIDEN_NOTE_STORAGE_SCHEMA_BYTES).unwrap();
    assert!(schema.contains("variant choice {\n        %present,"));
    assert!(schema.contains("present: u32"));
    assert!(!schema.contains("absent"));

    let value = NoteInputs::try_from(&[Felt::new(0).unwrap(), Felt::new(7).unwrap()][..]).unwrap();
    assert_eq!(value.choice, Choice::Present);
    assert_eq!(value.present, 7);
}
