// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AccountId, Word, account, active_note, note};

/// Native account of the note: exposes the `basic-wallet` component methods (e.g.
/// `receive_asset`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

#[note]
struct P2idNote {
    target_account_id: AccountId,
}

#[note]
impl P2idNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        let current_account = account.get_id();
        assert_eq!(current_account, self.target_account_id);

        let assets = active_note::get_initial_assets();
        for asset in assets {
            account.receive_asset(asset);
        }
    }
}
