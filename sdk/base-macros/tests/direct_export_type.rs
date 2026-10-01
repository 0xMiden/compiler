//! `export_type` remains usable without depending on the `miden` facade crate.

use miden_base_macros::export_type;

#[export_type]
struct DirectRecord {
    present: u32,
    #[cfg(any())]
    absent: MissingType,
}

#[test]
fn direct_macro_import_uses_filtered_type_shape() {
    assert_eq!(DirectRecord { present: 2 }.present, 2);
    let shape = DirectRecord::__MIDEN_EXPORT_TYPE_SHAPE;
    assert!(shape.contains("present"));
    assert!(!shape.contains("absent"));
}
