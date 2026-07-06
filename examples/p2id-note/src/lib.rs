// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AccountId, Recipient, Word, account, active_note, felt_repr::ToFeltRepr, note};

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
    /// Computes the recipient digest of a P2ID note targeted at `target`.
    ///
    /// The recipient commits to this crate's note script, the provided serial number, and the
    /// serialized note inputs, so a note created from it is consumable with the note script
    /// exported by this package.
    ///
    /// This constructor is exported from the compiled note package and is intended to be called
    /// by other Miden code (e.g. a transaction script). The caller turns the recipient into an
    /// output note through an account procedure (e.g. the basic wallet's `create-note`), because
    /// `output_note::create` requires the account-component context.
    #[note_constructor]
    pub fn build_recipient(target: AccountId, serial_num: Word) -> Recipient {
        let inputs = P2idNote {
            target_account_id: target,
        };
        let note_script_root = note::get_entrypoint_root();
        note::build_recipient(serial_num, note_script_root, inputs.to_felt_repr())
    }

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
