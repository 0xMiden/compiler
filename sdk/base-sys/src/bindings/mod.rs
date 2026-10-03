//! Bindings for Miden protocol
//!
//! # Word Field Ordering
//!
//! The Miden protocol MASM procedures expect and/or return Word on the stack with the least
//! significant felt on top of the stack.
//!
//! - In Rust: Word fields are stored as [e0, e1, e2, e3]
//! - In MASM procedures: These are pushed/popped from the stack in reverse order [e3, e2, e1, e0]

pub mod active_account;
pub mod active_note;
pub mod asset;
pub mod faucet;
pub mod input_note;
pub mod native_account;
pub mod note;
pub mod output_note;
pub mod storage;
pub mod tx;
mod types;

pub use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_stdlib_sys::Felt;
pub use types::*;

/// Maximum number of attachments per note, defined by the protocol MASM source at
/// `asm/kernels/transaction-core/src/output_note.masm`.
const MAX_ATTACHMENTS_PER_NOTE: usize = 4;

/// Maximum words per attachment, defined by the protocol MASM source at
/// `asm/protocol_utils/src/note.masm`.
const MAX_ATTACHMENT_WORDS: usize = 256;

/// Asserts that a note attachment count is within the protocol limit.
fn assert_attachment_count(num_attachments: usize) {
    assert!(
        num_attachments <= MAX_ATTACHMENTS_PER_NOTE,
        "note cannot contain more than {MAX_ATTACHMENTS_PER_NOTE} attachments"
    );
}

/// Asserts that an attachment word count is within the protocol limit.
fn assert_attachment_word_count(num_words: usize) {
    assert!(
        num_words <= MAX_ATTACHMENT_WORDS,
        "note attachment cannot contain more than {MAX_ATTACHMENT_WORDS} words"
    );
}

/// Converts an attachment scheme to the `u16` the protocol's attachment procedures take.
///
/// # Panics
///
/// If the scheme does not fit in a `u16`; the protocol's largest scheme is `u16::MAX - 1`.
fn attachment_scheme_u16(attachment_scheme: Felt) -> u16 {
    assert!(
        types::felt_at_most(attachment_scheme, u16::MAX.into()),
        "attachment scheme exceeds u16"
    );
    // The bound makes the truncation exact.
    types::felt_low_u32(attachment_scheme) as u16
}

/// Converts a note index and an attachment scheme to the `u16`s the protocol's attachment
/// procedures take, checking both bounds before converting either (see
/// [`types::felt_low_u32`]).
///
/// # Panics
///
/// If either does not fit in a `u16`; the note index is checked first.
fn note_index_and_scheme_u16(note_index: NoteIdx, attachment_scheme: Felt) -> (u16, u16) {
    assert!(types::felt_at_most(note_index.inner, u16::MAX.into()), "note index exceeds u16");
    assert!(
        types::felt_at_most(attachment_scheme, u16::MAX.into()),
        "attachment scheme exceeds u16"
    );
    // The bounds make the truncations exact.
    (
        types::felt_low_u32(note_index.inner) as u16,
        types::felt_low_u32(attachment_scheme) as u16,
    )
}

/// Converts an attachment index to the `u8` the protocol's attachment procedures take.
///
/// # Panics
///
/// If the index does not fit in a `u8`; no note has that many attachments.
fn attachment_index_u8(attachment_idx: u32) -> u8 {
    u8::try_from(attachment_idx).expect("attachment index exceeds u8")
}

// On the host the protocol procedures are unimplemented and panic when called, so these tests
// cover only the checks a lookup makes before calling one; the expected messages tell those
// panics apart.
#[cfg(test)]
mod tests {
    use miden_stdlib_sys::{Felt, Word, felt};

    use super::{NoteIdx, active_note, input_note, note, output_note};

    /// The smallest attachment scheme above `u16::MAX`.
    fn scheme_above_u16() -> Felt {
        Felt::from_u32(u32::from(u16::MAX) + 1)
    }

    /// The smallest note index above `u16::MAX`.
    fn note_index_above_u16() -> NoteIdx {
        NoteIdx {
            inner: Felt::from_u32(u32::from(u16::MAX) + 1),
        }
    }

    /// Ensures a metadata lookup rejects a scheme the protocol cannot store.
    #[test]
    #[should_panic(expected = "attachment scheme exceeds u16")]
    fn metadata_attachment_lookup_rejects_a_scheme_above_u16() {
        note::find_attachment_idx(scheme_above_u16(), Word::new([felt!(0); 4]));
    }

    /// Ensures an active-note lookup rejects a scheme the protocol cannot store.
    #[test]
    #[should_panic(expected = "attachment scheme exceeds u16")]
    fn active_note_attachment_lookup_rejects_a_scheme_above_u16() {
        active_note::find_attachment(scheme_above_u16());
    }

    /// Ensures an input-note lookup rejects a scheme the protocol cannot store.
    #[test]
    #[should_panic(expected = "attachment scheme exceeds u16")]
    fn input_note_attachment_lookup_rejects_a_scheme_above_u16() {
        input_note::find_attachment(NoteIdx::from(0u16), scheme_above_u16());
    }

    /// Ensures an output-note lookup rejects a scheme the protocol cannot store.
    #[test]
    #[should_panic(expected = "attachment scheme exceeds u16")]
    fn output_note_attachment_lookup_rejects_a_scheme_above_u16() {
        output_note::find_attachment(NoteIdx::from(0u16), scheme_above_u16());
    }

    /// Ensures an input-note lookup rejects a note index above `u16::MAX`, as the kernel does,
    /// checking it before the scheme.
    #[test]
    #[should_panic(expected = "note index exceeds u16")]
    fn input_note_attachment_lookup_rejects_a_note_index_above_u16() {
        input_note::find_attachment(note_index_above_u16(), scheme_above_u16());
    }

    /// Ensures an output-note lookup rejects a note index above `u16::MAX`, as the kernel does,
    /// checking it before the scheme.
    #[test]
    #[should_panic(expected = "note index exceeds u16")]
    fn output_note_attachment_lookup_rejects_a_note_index_above_u16() {
        output_note::find_attachment(note_index_above_u16(), scheme_above_u16());
    }
}
