//! DEX note with a custom limit-price storage type.

#![no_std]
#![feature(alloc_error_handler)]

use miden::{AccountId, Word, account, active_note, export_type, note};
use miden_field_repr::{FromFeltRepr, ToFeltRepr};

/// A rational limit price.
#[export_type]
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct LimitPrice {
    /// The price numerator.
    pub numerator: u64,
    /// The price denominator.
    pub denominator: u64,
}

/// Storage for one DEX note.
#[note]
struct DexNote {
    /// The account that can consume this note.
    target: AccountId,
    /// The exchange limit price.
    price: LimitPrice,
}

/// Native account interface used by the note script.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

#[note]
impl DexNote {
    /// Sends every note asset to the target account.
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        assert_eq!(account.get_id(), self.target);
        let _limit_price = self.price;

        let assets = active_note::get_initial_assets();
        for asset in assets {
            account.receive_asset(asset);
        }
    }
}
