//! Tests trailing-data rejection for unit note structs.

use core::convert::TryFrom;

use miden_base_macros::note;

extern crate self as miden;

pub use miden_field::Felt;

pub mod felt_repr {
    pub use miden_field_repr::{FeltReader, FeltReprError, FeltWriter, FromFeltRepr, ToFeltRepr};
}

#[derive(Debug)]
#[note]
struct UnitNote;

#[test]
fn unit_note_rejects_trailing_data() {
    let felts = [miden::Felt::new(0).unwrap()];

    let err = UnitNote::try_from(felts.as_slice()).unwrap_err();
    assert_eq!(err, miden::felt_repr::FeltReprError::TrailingData { pos: 0, len: 1 });
}
