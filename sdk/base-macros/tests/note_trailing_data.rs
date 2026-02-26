use core::convert::TryFrom;
use miden_base_macros::note;

extern crate self as miden;

pub use miden_field::Felt;

pub mod felt_repr {
    pub use miden_field_repr::{FeltReader, FeltReprError, FromFeltRepr};
}

#[derive(Debug)]
#[note]
struct UnitNote;

#[derive(Debug)]
#[note]
struct OneFeltNote {
    #[allow(dead_code)]
    a: miden::Felt,
}

#[test]
fn unit_note_rejects_trailing_data() {
    let felts = [miden::Felt::from_u64_unchecked(0)];

    let err = UnitNote::try_from(felts.as_slice()).unwrap_err();
    assert_eq!(
        err,
        miden::felt_repr::FeltReprError::TrailingData { pos: 0, len: 1 }
    );
}

#[test]
fn note_struct_rejects_trailing_data() {
    let felts = [
        miden::Felt::from_u64_unchecked(1),
        miden::Felt::from_u64_unchecked(2),
    ];

    let err = OneFeltNote::try_from(felts.as_slice()).unwrap_err();
    assert_eq!(
        err,
        miden::felt_repr::FeltReprError::TrailingData { pos: 1, len: 2 }
    );
}
