// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::vec::Vec;

use miden::*;

use crate::bindings::miden::basic_wallet::basic_wallet::receive_asset;

#[note_script]
fn run(_arg: Word) {
    let inputs = active_note::get_inputs();
    let target_account_id_prefix = inputs[0];
    let target_account_id_suffix = inputs[1];

    let target_account = AccountId::from(target_account_id_prefix, target_account_id_suffix);
    let current_account = active_account::get_id();
    assert_eq!(current_account, target_account);

    let assets = active_note::get_assets();
    for asset in assets {
        receive_asset(asset);
    }
}
