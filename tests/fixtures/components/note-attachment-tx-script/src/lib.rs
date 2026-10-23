//! Exercises typed note attachments on a P2ID note: the P2ID note constructor builds the
//! recipient and `account.create_note` creates the note.
//!
//! Element 0 of the script argument selects the scenario: round-trip checks (0), four/five
//! attachments (1/2), 512/513 total words (3/4), adding an attachment to a sealed note (5), or
//! reporting the outcome of converting element 1 to a scheme as a one-word attachment (6:
//! `[0, value]` ok, `[1, value]` invalid scheme, `[2]` exceeds `u16`, `[3]` other (unreachable
//! today)), reporting the felt-repr decoding of element 1 as a scheme and of element 2 as an
//! attachment index as two one-word attachments, scheme first (7: `[0, value]` ok,
//! `[1, value, max, ty]` out of range, `[3]` other (unreachable for one felt)), or reading
//! attachment index 3 of a note with one attachment (8). Element 1 carries the attachment scheme
//! and element 2 the attachment index as felts. The whole word is also the P2ID serial number.

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
    // scenario 6 reports the result, the others require a valid scheme.
    let scheme = NoteAttachmentScheme::try_from(arg[1]);
    if scenario == 6 {
        // Report which result the compiled conversion produced, so the host can tell the
        // rejecting branches apart instead of matching a generic guest panic.
        let outcome = match scheme {
            Ok(scheme) => Word::from([felt!(0), Felt::from(scheme.as_u16()), felt!(0), felt!(0)]),
            Err(NoteAttachmentError::InvalidScheme(value)) => {
                Word::from([felt!(1), Felt::from(value), felt!(0), felt!(0)])
            }
            Err(NoteAttachmentError::SchemeExceedsU16) => {
                Word::from([felt!(2), felt!(0), felt!(0), felt!(0)])
            }
            // Required by the non-exhaustive error enum; try_from never produces it.
            Err(_) => Word::from([felt!(3), felt!(0), felt!(0), felt!(0)]),
        };
        NoteAttachment::with_word(NoteAttachmentScheme::none(), outcome).add_to(note_idx);
        return;
    }
    if scenario == 7 {
        check_felt_decoders(note_idx, arg[1], arg[2]);
        return;
    }

    let scheme = scheme.unwrap();
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
            let attachment = NoteAttachment::with_words(scheme, alloc::vec![first; 256]).unwrap();
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
        8 => {
            NoteAttachment::with_word(scheme, first).add_to(note_idx);
            // Index 3 passes the SDK's bound but not the protocol library's: the note has one
            // attachment.
            let _ = output_note::write_attachment_to_memory(note_idx, AttachmentIdx::MAX);
        }
        _ => panic!("unknown attachment scenario"),
    }
}

/// Reports the felt-repr decoding of `scheme` as a scheme and of `index` as an attachment index as
/// two one-word attachments, in that order, and checks that the index decoder round-trips
/// [`AttachmentIdx::MAX`].
fn check_felt_decoders(note_idx: NoteIdx, scheme: Felt, index: Felt) {
    let scheme = NoteAttachmentScheme::try_from(&[scheme][..]).map(|s| Felt::from(s.as_u16()));
    let index = AttachmentIdx::try_from(&[index][..]).map(|i| Felt::from(i.as_u8()));
    for outcome in [scheme, index] {
        NoteAttachment::with_word(NoteAttachmentScheme::none(), decode_outcome(outcome))
            .add_to(note_idx);
    }

    let idx = AttachmentIdx::MAX;
    let back = AttachmentIdx::try_from(&idx.to_felt_repr()[..]).unwrap();
    assert_eq!(back, idx);
}

/// Encodes a felt-repr decoding result as `[0, value, 0, 0]` on success and
/// `[1, value, max, ty]` for an out-of-range value, where `ty` is `0` for `NoteAttachmentScheme`,
/// `1` for `AttachmentIdx` and `2` for any other type.
fn decode_outcome(outcome: Result<Felt, felt_repr::FeltReprError>) -> Word {
    match outcome {
        Ok(value) => Word::from([felt!(0), value, felt!(0), felt!(0)]),
        Err(felt_repr::FeltReprError::ValueOutOfRange { value, max, ty, .. }) => {
            let ty = if is_type_name(ty, "NoteAttachmentScheme") {
                felt!(0)
            } else if is_type_name(ty, "AttachmentIdx") {
                felt!(1)
            } else {
                felt!(2)
            };
            // Both values are canonical felt values, so the conversions cannot fail.
            Word::from([felt!(1), Felt::new(value).unwrap(), Felt::new(max).unwrap(), ty])
        }
        // A one-felt slice decodes or fails the range check; it never runs out of felts or has
        // trailing ones.
        Err(_) => Word::from([felt!(3), felt!(0), felt!(0), felt!(0)]),
    }
}

/// Returns `true` if `ty` has the length and first byte of `name`.
///
/// `str` equality lowers to a `memcmp` call, which the guest does not link; the length and first
/// byte tell the two type names apart (20 bytes starting with `N`, 13 bytes starting with `A`).
fn is_type_name(ty: &str, name: &str) -> bool {
    ty.len() == name.len() && ty.as_bytes().first() == name.as_bytes().first()
}

/// Checks content, append order, first-match lookup, read-back and commitments for all writer
/// paths.
fn check_round_trip(note_idx: NoteIdx, scheme: NoteAttachmentScheme, first: Word) {
    // The host keeps the scheme at most 65531, so `+ 3` stays a valid scheme.
    let s1 = NoteAttachmentScheme::new(scheme.as_u16() + 1).unwrap();
    let s2 = NoteAttachmentScheme::new(scheme.as_u16() + 2).unwrap();
    let absent = NoteAttachmentScheme::new(scheme.as_u16() + 3).unwrap();
    let second = Word::from([felt!(5), felt!(6), felt!(7), felt!(8)]);
    let advice = [
        Word::from([felt!(9), felt!(10), felt!(11), felt!(12)]),
        Word::from([felt!(13), felt!(14), felt!(15), felt!(16)]),
    ];
    let attachment_0 = NoteAttachment::with_word(scheme, first);
    let attachment_1 = NoteAttachment::with_words(s1, alloc::vec![second]).unwrap();
    let attachment_2 = NoteAttachment::with_words(scheme, alloc::vec![first, second]).unwrap();
    assert_eq!(attachment_0.num_words(), 1);
    assert_eq!(attachment_1.attachment_scheme(), s1);
    attachment_0.add_to(note_idx);
    attachment_1.add_to(note_idx);
    let schemes =
        || note::metadata_into_attachment_schemes(output_note::get_metadata(note_idx).header);
    assert_eq!(schemes(), [Some(scheme), Some(s1), None, None]);
    attachment_2.add_to(note_idx);

    // The host supplies this preimage in the advice map, which the advice-map writer consumes.
    let advice_commitment = hash_words(&advice).inner;
    output_note::add_attachment(note_idx, s2, advice_commitment);

    assert_eq!(schemes(), [Some(scheme), Some(s1), Some(scheme), Some(s2)]);
    // A duplicate scheme resolves to its first attachment.
    let header = output_note::get_metadata(note_idx).header;
    let idx = |n| AttachmentIdx::new(n).unwrap();
    for (lookup, expected) in
        [(scheme, Some(idx(0))), (s1, Some(idx(1))), (s2, Some(idx(3))), (absent, None)]
    {
        assert_eq!(output_note::find_attachment(note_idx, lookup), expected);
        assert_eq!(note::find_attachment_idx(lookup, header), expected);
    }
    // Every attachment has distinct content, so the read-back tells the indexes apart; the first
    // match for `scheme` is attachment 0, not attachment 2.
    assert_eq!(output_note::read_attachment(note_idx, scheme), Some(attachment_0));
    assert_eq!(output_note::read_attachment(note_idx, s1), Some(attachment_1));
    assert_eq!(
        output_note::read_attachment(note_idx, s2),
        Some(NoteAttachment::with_words(s2, advice.to_vec()).unwrap())
    );
    assert_eq!(output_note::read_attachment(note_idx, absent), None);
    let expected_words: [&[Word]; 4] = [&[first], &[second], &[first, second], &advice];
    for (n, words) in (0..).zip(expected_words) {
        assert_eq!(output_note::write_attachment_to_memory(note_idx, idx(n)).as_slice(), words);
    }
    let commitments = output_note::write_attachment_commitments_to_memory(note_idx);
    assert_eq!(
        commitments.as_slice(),
        &[
            hash_words(&[first]).inner,
            hash_words(&[second]).inner,
            hash_words(&[first, second]).inner,
            advice_commitment,
        ]
    );
}
