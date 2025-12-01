// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::vec::Vec;

use miden::*;

use crate::bindings::Account;

fn consume_assets(account: &Account) {
    let assets = active_note::get_assets();
    for asset in assets {
        // TODO: `receieve_asset` should require `account` to be &mut
        // Only when account is Wasm CM resource? Otherwise its a bunch of free functions
        // (WIT interface has no notion of borrowing)
        account.receive_asset(asset);
    }
}

fn reclaim_assets(account: &Account, consuming_account: AccountId) {
    let creator_account = active_note::get_sender();

    if consuming_account == creator_account {
        consume_assets(account);
    } else {
        panic!();
    }
}

#[note_script]
fn run(_arg: Word, account: Account) {
    let inputs = active_note::get_inputs();

    // make sure the number of inputs is 4
    assert_eq((inputs.len() as u32).into(), felt!(4));

    let target_account_id_prefix = inputs[0];
    let target_account_id_suffix = inputs[1];

    let timelock_height = inputs[2];
    let reclaim_height = inputs[3];

    // get block number
    let block_number = tx::get_block_number();
    assert!(block_number >= timelock_height);

    // get consuming account id
    let consuming_account_id = account.get_id();

    // target account id
    let target_account_id = AccountId::from(target_account_id_prefix, target_account_id_suffix);

    let is_target = target_account_id == consuming_account_id;
    if is_target {
        consume_assets(&account);
    } else {
        assert!(reclaim_height >= block_number);
        reclaim_assets(&account, consuming_account_id);
    }
}
