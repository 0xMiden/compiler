// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::vec::Vec;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use miden::*;

use crate::bindings::miden::basic_wallet::basic_wallet::receive_asset;

fn consume_assets() {
    let assets = note::get_assets();
    for asset in assets {
        receive_asset(asset);
    }
}

fn reclaim_assets(consuming_account: AccountId) {
    let creator_account = note::get_sender();

    if consuming_account == creator_account {
        consume_assets();
    } else {
        panic!();
    }
}

#[note_script]
fn run(_arg: Word) {
    let inputs = note::get_inputs();
    let target_account_id_prefix = inputs[0];
    let target_account_id_suffix = inputs[1];
    
    let timelock_height = inputs[2];
    let reclaim_height = inputs[3];

    // make sure the number of inputs is 4
    assert_eq(inputs.len().into(), Felt::from(4u32));

    // get block number
    let block_number = tx::get_block_number();
    assert!(block_number >= timelock_height);

    // get consuming account id
    let consuming_account_id = account::get_id();

    // target account id
    let target_account_id = AccountId::from(target_account_id_prefix, target_account_id_suffix);

    let is_target = target_account_id == consuming_account_id;
    if is_target {
        consume_assets();
    } else {
        assert!(reclaim_height >= block_number);
        reclaim_assets(consuming_account_id);
    }
}
