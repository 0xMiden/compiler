//! Exercises typed note attachments on a P2ID note: the P2ID note constructor builds the
//! recipient and `account.create_note` creates the note.
//!
//! Element 0 of the script argument selects the scenario: round-trip checks (0), four/five
//! attachments (1/2), 512/513 total words (3/4), adding an attachment to a sealed note (5), or an
//! invalid scheme (6). Element 1 carries the attachment scheme as a felt. The whole word is also
//! the P2ID serial number.

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
    // The scheme comes from the script argument so the check is not folded away at compile time;
    // an invalid scheme makes the script panic.
    let scheme = NoteAttachmentScheme::try_from(arg[1]).unwrap();
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
        // The host passes an invalid scheme: the script must already have panicked at the
        // conversion above.
        6 => {}
        _ => panic!("unknown attachment scenario"),
    }
}

/// Checks content, append order, first-match lookup and commitments for all writer paths.
fn check_round_trip(note_idx: NoteIdx, scheme: NoteAttachmentScheme, first: Word) {
    let s1 = NoteAttachmentScheme::new(scheme.as_u16() + 1).unwrap();
    let s2 = NoteAttachmentScheme::new(scheme.as_u16() + 2).unwrap();
    let absent = NoteAttachmentScheme::new(scheme.as_u16() + 3).unwrap();
    let second = Word::from([felt!(5), felt!(6), felt!(7), felt!(8)]);
    let single = [first];
    let multiple = [first, second];
    let advice = [
        Word::from([felt!(9), felt!(10), felt!(11), felt!(12)]),
        Word::from([felt!(13), felt!(14), felt!(15), felt!(16)]),
    ];
    let word_attachment = NoteAttachment::with_word(scheme, first);
    let slice_attachment = NoteAttachment::with_words(s1, &single).unwrap();
    assert_eq!(word_attachment.as_words(), slice_attachment.as_words());
    assert_eq!(word_attachment.num_words(), 1);
    assert_eq!(slice_attachment.attachment_scheme(), s1);
    word_attachment.add_to(note_idx);
    slice_attachment.add_to(note_idx);
    let schemes =
        || note::metadata_into_attachment_schemes(output_note::get_metadata(note_idx).header);
    assert_eq!(schemes(), [Some(scheme), Some(s1), None, None]);
    NoteAttachment::with_words(scheme, &multiple).unwrap().add_to(note_idx);

    // The host supplies this preimage in the advice map, which the advice-map writer consumes.
    let multi_commitment = hash_words(&multiple).inner;
    let advice_commitment = hash_words(&advice).inner;
    output_note::add_attachment(note_idx, s2, advice_commitment);

    assert_eq!(schemes(), [Some(scheme), Some(s1), Some(scheme), Some(s2)]);
    // A duplicate scheme resolves to its first attachment.
    let header = output_note::get_metadata(note_idx).header;
    for (lookup, expected) in [(scheme, Some(0)), (s1, Some(1)), (s2, Some(3)), (absent, None)] {
        assert_eq!(output_note::find_attachment(note_idx, lookup), expected);
        assert_eq!(note::find_attachment_idx(lookup, header), expected);
    }
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 0).as_slice(), &single);
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 1).as_slice(), &single);
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 2).as_slice(), &multiple);
    assert_eq!(output_note::write_attachment_to_memory(note_idx, 3).as_slice(), &advice);
    let commitments = output_note::write_attachment_commitments_to_memory(note_idx);
    assert_eq!(
        commitments.as_slice(),
        &[
            hash_words(&single).inner,
            hash_words(&single).inner,
            multi_commitment,
            advice_commitment,
        ]
    );
}
