#![no_std]
#![feature(alloc_error_handler)]

use miden::{Asset, AssetAmount, Word, felt, tx_script};

use crate::bindings::miden::storage_example::foo;

#[tx_script]
fn run(_arg: Word) {
    let asset = Asset::new(
        Word::new([felt!(2), felt!(3), felt!(4), felt!(5)]),
        Word::new([felt!(6), felt!(7), felt!(8), felt!(9)]),
    );
    let quantity = AssetAmount::new(7).unwrap();
    foo::set_asset_qty(
        Word::new([felt!(1), felt!(0), felt!(0), felt!(0)]),
        asset,
        quantity,
    );
    assert_eq!(foo::get_asset_qty(asset), quantity);
}
