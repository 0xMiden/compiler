#![no_std]
#![feature(alloc_error_handler)]

#[macro_use]
extern crate alloc;

use crate::bindings::Account;
use alloc::vec;
use miden::{intrinsics::advice::adv_push_mapvaln, *};

#[tx_script]
fn run(arg: Word, account: &mut Account) {
    let num_felts = adv_push_mapvaln(arg.clone());
    let num_felts_u64 = num_felts.as_u64();

    let num_assets = num_felts_u64 / 4;
    let num_words = Felt::from_u64_unchecked(num_felts_u64 / 4);

    // Load all words at once, verified against the commitment (RPO hash)
    let data = adv_load_preimage(num_words, arg);

    // Receive assets
    for i in 0..num_assets {
        let off = (i * 4) as usize;
        let asset_word = Word::new([data[off], data[off + 1], data[off + 2], data[off + 3]]);

        account.receive_asset(Asset::new(asset_word));
    }
}
