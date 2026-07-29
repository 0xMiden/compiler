// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the `basic-wallet` component methods (e.g.
/// `move_asset_to_note`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
struct Wallet;

/// Arguments of the transaction script, transported via the `TX_SCRIPT_ARGS` word.
///
/// The encoding exceeds one word, so the args word is the hash of the encoded fields and the
/// values travel through the advice provider, verified against the args word (see `ScriptArgs`).
/// Hosts building the transaction encode a struct with the identical field layout.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArg {
    /// The output note's tag.
    pub tag: Tag,
    /// The output note's type.
    pub note_type: NoteType,
    /// The output note's recipient digest.
    pub recipient: Recipient,
    /// The asset to move to the output note.
    pub asset: Asset,
}

#[tx_script]
fn run(inputs: TxScriptArg, account: &mut Wallet) {
    let note_idx = account.create_note(inputs.tag, inputs.note_type, inputs.recipient);
    account.move_asset_to_note(inputs.asset, note_idx);
}
