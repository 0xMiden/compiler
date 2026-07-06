// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::{intrinsics::advice::adv_push_mapvaln, *};

use crate::bindings::miden::p2id::miden_p2id;

/// Native account of the transaction script: exposes the `basic-wallet` component methods (e.g.
/// `move_asset_to_note`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
struct Wallet;

// Input layout constants
const TAG_INDEX: usize = 0;
const NOTE_TYPE_INDEX: usize = 1;
const TARGET_PREFIX_INDEX: usize = 2;
const TARGET_SUFFIX_INDEX: usize = 3;
const SERIAL_NUM_START: usize = 4;
const SERIAL_NUM_END: usize = 8;
const ASSET_START: usize = 8;
const ASSET_END: usize = 16;

/// Creates a P2ID output note via the note package's exported constructor and moves the given
/// asset into it.
///
/// Unlike `basic-wallet-tx-script`, which receives a pre-computed note recipient, this script
/// only receives the note parameters (target account, tag, note type, serial number): the
/// recipient — including the note script root — is computed by the `p2id` note package itself.
#[tx_script]
fn run(arg: Word, account: &mut Wallet) {
    let num_felts = adv_push_mapvaln(arg);
    let num_felts_u64 = num_felts.as_canonical_u64();
    assert_eq(Felt::from_u32((num_felts_u64 % 4) as u32), felt!(0));
    let num_words = Felt::new(num_felts_u64 / 4).unwrap();
    let commitment = arg;
    let input = adv_load_preimage(num_words, commitment);
    let tag: Tag = input[TAG_INDEX].into();
    let note_type: NoteType = input[NOTE_TYPE_INDEX].into();
    let target = AccountId::new(input[TARGET_PREFIX_INDEX], input[TARGET_SUFFIX_INDEX]);
    let serial_num: [Felt; 4] = input[SERIAL_NUM_START..SERIAL_NUM_END].try_into().unwrap();
    let note_idx = miden_p2id::create(target, tag, note_type, serial_num.into());
    let asset: [Felt; 8] = input[ASSET_START..ASSET_END].try_into().unwrap();
    let asset_key: [Felt; 4] = asset[..4].try_into().unwrap();
    let asset_value: [Felt; 4] = asset[4..].try_into().unwrap();
    let asset = Asset::new(asset_key, asset_value);
    account.move_asset_to_note(asset, note_idx);
}
