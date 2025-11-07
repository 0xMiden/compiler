#![no_std]

use miden::*;

use crate::bindings::miden::component_macros_account::component_macros_account::{
    test_custom_types, StructA, StructB,
};

#[note_script]
fn run(_arg: Word) {
    let foo_val = Word::from([felt!(11), felt!(22), felt!(33), felt!(44)]);
    let asset = Asset::new([felt!(99), felt!(88), felt!(77), felt!(66)]);
    let value = StructA {
        foo: foo_val,
        asset,
    };
    let result = test_custom_types(value, asset);
    let expected = StructB {
        bar: foo_val.inner.0,
        baz: asset.inner.inner.0,
    };

    assert_eq!(result.bar, expected.bar);
    assert_eq!(result.baz, expected.baz);
}
