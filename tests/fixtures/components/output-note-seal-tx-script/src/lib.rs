//! Transaction script which checks the output-note sealing bindings against the transaction kernel.
//!
//! It creates an output note, checks that the note is not sealed, seals it twice and checks that
//! it reports as sealed after each seal. When the first felt of the script argument is non-zero,
//! it then tries to add an attachment to the sealed note, which the kernel must reject.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the `basic-wallet` component methods (e.g.
/// `create_note`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
struct Wallet;

#[tx_script]
fn run(arg: Word, account: &mut Wallet) {
    let tag = Tag::from(felt!(0));
    // Private note type (0b00).
    let note_type = NoteType::from(felt!(0));
    let recipient = Recipient::from([felt!(1), felt!(2), felt!(3), felt!(4)]);
    let note_idx = account.create_note(tag, note_type, recipient);
    assert!(!output_note::is_sealed(note_idx), "a new output note must not be sealed");

    output_note::seal(note_idx);
    assert!(output_note::is_sealed(note_idx), "a sealed output note must report as sealed");

    // Sealing is idempotent.
    output_note::seal(note_idx);
    assert!(output_note::is_sealed(note_idx), "a sealed output note must stay sealed");

    if arg[0] != felt!(0) {
        // Scheme 0 is reserved by the protocol to signal an absent attachment.
        let attachment_scheme = felt!(1);
        let attachment = Word::from([felt!(5), felt!(6), felt!(7), felt!(8)]);
        output_note::add_word_attachment(note_idx, attachment_scheme, attachment);
    }
}
