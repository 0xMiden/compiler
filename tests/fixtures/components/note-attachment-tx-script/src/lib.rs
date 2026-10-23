//! Exercises typed note attachments after creating a note through its exported constructor.
//!
//! The first script argument selects round-trip checks (0), four/five attachments (1/2),
//! 512/513 total words (3/4), or adding an attachment to a sealed note (5).

#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::*;

use crate::bindings::miden::p2id::p2id;

/// Native wallet that creates notes from the note constructor's recipient.
#[account(basic_wallet::BasicWallet)]
struct Wallet;

/// Creates a P2ID note and exercises the selected attachment behavior.
#[tx_script]
fn run(arg: Word, account: &mut Wallet) {
    let target = account.get_id();
    let recipient = p2id::build_recipient(target, arg);
    let note_idx = account.create_note(Tag::from(0u32), NoteType::from(0u8), recipient);
    let scenario = arg[0].as_canonical_u64();
    // Keep the scheme supplied at runtime to exercise validation in compiled guest code.
    let scheme = NoteAttachmentScheme::new(arg[1].as_canonical_u64() as u16).unwrap();
    let first = Word::from([felt!(1), felt!(2), felt!(3), felt!(4)]);

    match scenario {
        0 => check_round_trip(note_idx, scheme, first),
        1 | 2 => {
            let attachment = NoteAttachment::with_word(scheme, first);
            let count = if scenario == 1 { 4 } else { 5 };
            for _ in 0..count {
                attachment.add_to(note_idx);
            }
        }
        3 | 4 => {
            let words = alloc::vec![first; 256];
            let attachment = NoteAttachment::with_words(scheme, &words).unwrap();
            attachment.add_to(note_idx);
            attachment.add_to(note_idx);
            if scenario == 4 {
                NoteAttachment::with_word(scheme, first).add_to(note_idx);
            }
        }
        5 => {
            output_note::seal(note_idx);
            NoteAttachment::with_word(scheme, first).add_to(note_idx);
        }
        _ => panic!("unknown attachment scenario"),
    }
}

/// Checks content, append order, first-match lookup and commitments for all writer paths.
fn check_round_trip(note_idx: NoteIdx, scheme: NoteAttachmentScheme, first: Word) {
    let second = Word::from([felt!(5), felt!(6), felt!(7), felt!(8)]);
    let single = [first];
    let multiple = [first, second];
    let legacy = [
        Word::from([felt!(9), felt!(10), felt!(11), felt!(12)]),
        Word::from([felt!(13), felt!(14), felt!(15), felt!(16)]),
    ];
    let word_attachment = NoteAttachment::with_word(scheme, first);
    let slice_attachment = NoteAttachment::with_words(scheme, &single).unwrap();
    assert_eq!(word_attachment.as_words(), slice_attachment.as_words());
    assert_eq!(word_attachment.num_words(), 1);
    assert_eq!(slice_attachment.attachment_scheme(), scheme);
    word_attachment.add_to(note_idx);
    slice_attachment.add_to(note_idx);
    NoteAttachment::with_words(scheme, &multiple).unwrap().add_to(note_idx);

    // The host supplies this preimage in the advice map; the legacy writer still consumes it.
    let multi_commitment = hash_words(&multiple).inner;
    let legacy_commitment = hash_words(&legacy).inner;
    output_note::add_attachment(note_idx, scheme, legacy_commitment);

    assert_eq!(output_note::find_attachment(note_idx, scheme), Some(0));
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 0).as_slice(), &single);
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 1).as_slice(), &single);
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 2).as_slice(), &multiple);
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 3).as_slice(), &legacy);
    let commitments = output_note::write_attachment_commitments_to_memory(note_idx);
    assert_eq!(
        commitments.as_slice(),
        &[
            hash_words(&single).inner,
            hash_words(&single).inner,
            multi_commitment,
            legacy_commitment,
        ]
    );
}
