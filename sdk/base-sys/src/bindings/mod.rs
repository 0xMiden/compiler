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
pub mod faucet;
pub mod input_note;
pub mod native_account;
pub mod note;
pub mod output_note;
pub mod storage;
pub mod tx;
mod types;

pub use miden_field_repr::{FromFeltRepr, ToFeltRepr};
pub use types::*;

/// Maximum number of attachments per note, defined by the protocol MASM source at
/// `asm/kernels/transaction-core/src/output_note.masm`.
pub(super) const MAX_ATTACHMENTS_PER_NOTE: usize = 4;

/// Maximum words per attachment, defined by the protocol MASM source at
/// `asm/protocol_utils/src/note.masm`.
pub(super) const MAX_ATTACHMENT_WORDS: usize = 256;

/// Asserts that a note attachment count is within the protocol limit.
pub(super) fn assert_attachment_count(num_attachments: usize) {
    assert!(
        num_attachments <= MAX_ATTACHMENTS_PER_NOTE,
        "note cannot contain more than {MAX_ATTACHMENTS_PER_NOTE} attachments"
    );
}

/// Asserts that an attachment word count is within the protocol limit.
pub(super) fn assert_attachment_word_count(num_words: usize) {
    assert!(
        num_words <= MAX_ATTACHMENT_WORDS,
        "note attachment cannot contain more than {MAX_ATTACHMENT_WORDS} words"
    );
}
