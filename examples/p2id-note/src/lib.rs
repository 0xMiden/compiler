// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::Account;

#[note_script]
fn run(_arg: Word, account: &mut Account) {
    let inputs = active_note::get_inputs();
    let target_account_id: AccountId = inputs.as_slice().into();

    let current_account = account.get_id();
    assert_eq!(current_account, target_account_id);

    let assets = active_note::get_assets();
    for asset in assets {
        account.receive_asset(asset);
    }
}
