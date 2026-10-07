// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the methods of the standard (MASM) basic
/// wallet component (e.g. `move_asset_to_note`), whose interface is derived from the
/// `miden-standards-wallets-basic-wallet` package manifest.
#[account(miden_standards_wallets_basic_wallet::BasicWallet)]
struct Wallet;

/// Arguments of the transaction script, transported via the `TX_SCRIPT_ARGS` word.
///
/// The encoding exceeds one word, so the args word is the hash of the encoded fields and the
/// values travel through the advice provider, verified against the args word (see `ScriptArgs`).
/// Hosts building the transaction encode a struct with the identical field layout.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArgs {
    /// The output note's tag.
    pub tag: Tag,
    /// The output note's type.
    pub note_type: NoteType,
    /// The output note's recipient digest.
    pub recipient: Recipient,
    /// The asset to move to the output note.
    pub asset: Asset,
}

/// Creates an output note and moves `args.asset` into it.
///
/// The standard wallet's interface is derived from its package manifest, which carries no type
/// aliases: the tag is a plain `u32`, the recipient a `Word`, and the note index a `u16`.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut Wallet) {
    let note_idx = account.create_note(args.tag.inner, args.note_type, args.recipient.inner);
    account.move_asset_to_note(args.asset, note_idx);
}
