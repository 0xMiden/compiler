// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::{
    AccountId, NoteIdx, NoteType, Recipient, Tag, Word, account, active_note,
    felt_repr::ToFeltRepr, note, output_note,
};

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
    /// Creates a P2ID output note targeted at `target` and returns its index.
    ///
    /// The note recipient commits to this crate's note script, the provided serial number, and
    /// the serialized note inputs, so the created note is consumable with the note script
    /// exported by this package.
    ///
    /// This constructor is exported from the compiled note package and is intended to be called
    /// by other Miden code (e.g. a transaction script) that creates this note.
    #[note_constructor]
    pub fn create(target: AccountId, tag: Tag, note_type: NoteType, serial_num: Word) -> NoteIdx {
        let inputs = P2idNote {
            target_account_id: target,
        };
        let recipient = build_recipient(inputs, serial_num);
        output_note::create(tag, note_type, recipient)
    }

    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        let current_account = account.get_id();
        assert_eq!(current_account, self.target_account_id);

        let assets = active_note::get_assets();
        for asset in assets {
            account.receive_asset(asset);
        }
    }
}

/// Builds the note recipient committing to the note script root, serial number, and note inputs.
fn build_recipient(inputs: P2idNote, serial_num: Word) -> Recipient {
    let note_script_root = P2idNote::get_entrypoint_root();
    note::build_recipient(serial_num, note_script_root, inputs.to_felt_repr())
}
