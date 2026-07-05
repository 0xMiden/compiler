//! Batch state shared between the kernel phases.
//!
//! Mirrors `asm/kernels/batch/lib/memory.masm` from the protocol batch kernel
//! (0xMiden/protocol#2905). The MASM module lays the batch state out as flat memory regions
//! (transaction tuples, transaction headers, sorted note lists and their parallel flag arrays)
//! and exposes accessors over them; the Rust equivalent holds the same data as typed collections
//! in [`BatchMemory`]. The per-transaction note scratch region has no counterpart here: each
//! transaction's notes are decoded into a temporary that is dropped when the transaction has been
//! processed.

extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, felt};

// CAPACITY LIMITS
// =================================================================================================

/// Maximum number of transactions in a batch.
pub const MAX_TRANSACTIONS_PER_BATCH: usize = 1024;

/// Maximum number of entries in a sorted note list and in a single transaction's note set.
/// Equals `MAX_INPUT_NOTES_PER_BATCH` = `MAX_OUTPUT_NOTES_PER_BATCH` in the protocol.
pub const MAX_NOTES_PER_BATCH: usize = 1024;

/// Number of felts each transaction tuple occupies in the Layer 1 piped data.
pub const TX_TUPLE_FELT_LEN: usize = 8;

/// Number of felts each transaction header occupies in the Layer 2 piped data. This must match
/// the felt-sequence layout of `TransactionId::new`.
pub const TX_HEADER_FELT_LEN: usize = 16;

/// Number of felts each note occupies in a sorted note list entry (KEY word + VALUE word).
pub const NOTE_ENTRY_FELT_LEN: usize = 8;

// BATCH STATE
// =================================================================================================

/// A verified Layer 1 `(tx_id, account_id)` tuple: `[TX_ID, account_id_prefix, account_id_suffix,
/// 0, 0]`.
pub struct TxTuple {
    /// The transaction's `TransactionId` commitment.
    pub tx_id: Word,
    /// The account the transaction executes against, as `[prefix, suffix, 0, 0]`. Verified as
    /// part of the Layer 1 pre-image but not otherwise used until account updates are aggregated.
    #[allow(dead_code)]
    pub account_id: Word,
}

/// A verified Layer 2 transaction header: the felt sequence hashed by `TransactionId::new`,
/// `[INIT, FINAL, INPUT_NOTES_COMMITMENT, OUTPUT_NOTES_COMMITMENT]`.
pub struct TxHeader {
    /// The account's state commitment before the transaction. Verified as part of the Layer 2
    /// pre-image but not otherwise used until account updates are aggregated.
    #[allow(dead_code)]
    pub init_account_commitment: Word,
    /// The account's state commitment after the transaction. Verified as part of the Layer 2
    /// pre-image but not otherwise used until account updates are aggregated.
    #[allow(dead_code)]
    pub final_account_commitment: Word,
    /// The transaction's input notes commitment.
    pub input_notes_commitment: Word,
    /// The transaction's output notes commitment.
    pub output_notes_commitment: Word,
}

/// An 8-felt sorted note list entry `[KEY, VALUE]`.
///
/// For the nullifier-sorted input-note list the KEY word is the nullifier (the lookup key) and
/// the VALUE word is the note id (for unauthenticated notes) or the empty word. The VALUE word
/// doubles as the second half of the `(nullifier, note_id_or_empty)` tuple hashed into
/// `INPUT_NOTES_COMMITMENT`.
///
/// For the note-id-sorted output-note list the KEY word is the note id; the VALUE word is unused.
pub struct NoteEntry {
    /// The sorted-list lookup key.
    pub key: Word,
    /// The value associated with the key.
    pub value: Word,
}

// The flag fields below are `Felt`-typed like the felts of the MASM flag words. This also keeps
// every flag test a VM felt comparison: integer-typed flags invite LLVM to merge the flag tests
// into a branch table, a shape the Miden backend does not currently lower correctly.

/// `erasure` flag value: not erased / external.
pub fn erasure_not_erased() -> Felt {
    felt!(0)
}

/// `erasure` flag value: expected to be erased: the input note's id matches an output note
/// created by another transaction within this batch -- its "creator" -- which has not been
/// processed yet.
pub fn erasure_expected() -> Felt {
    felt!(1)
}

/// `erasure` flag value: erased: the creator output note has been processed.
pub fn erasure_erased() -> Felt {
    felt!(2)
}

/// Parallel input-note flags: the `[erasure, consumption, 0, 0]` flag word.
pub struct InputNoteFlags {
    /// The entry's erasure state: one of the `erasure_*` values.
    pub erasure: Felt,
    /// Whether the entry has been consumed by a transaction (0 or 1).
    pub consumed: Felt,
}

/// Parallel output-note flags: the `[will_be_erased, is_created, linked_input_index, 0]` flag
/// word.
pub struct OutputNoteFlags {
    /// Whether this output note erases an input note of the batch (0 or 1).
    pub will_be_erased: Felt,
    /// Whether the entry has been created by a transaction (0 or 1).
    pub is_created: Felt,
    /// Index of the input-note list entry this output note erases (meaningful only when
    /// `will_be_erased` is set).
    pub linked_input_index: usize,
}

/// The batch state produced by the prologue and updated by the note tracker.
///
/// Each field corresponds to a memory region of `memory.masm`; the region's entry count
/// (`num_transactions`, `num_input_notes`, `num_output_notes`) is the collection length.
pub struct BatchMemory {
    /// Layer 1 piped data: the verified `(tx_id, account_id)` tuples, in batch order.
    pub tx_tuples: Vec<TxTuple>,
    /// Layer 2 piped data: the verified per-transaction headers, in batch order.
    pub tx_headers: Vec<TxHeader>,
    /// The nullifier-sorted input-note list.
    pub input_notes: Vec<NoteEntry>,
    /// Parallel flags for `input_notes`.
    pub input_note_flags: Vec<InputNoteFlags>,
    /// The note-id-sorted output-note list.
    pub output_notes: Vec<NoteEntry>,
    /// Parallel flags for `output_notes`.
    pub output_note_flags: Vec<OutputNoteFlags>,
    /// The running minimum of the transactions' expiration block numbers.
    pub batch_expiration_block_num: Felt,
}

impl BatchMemory {
    /// Returns `num_transactions`.
    pub fn num_transactions(&self) -> usize {
        self.tx_tuples.len()
    }

    /// Returns the verified per-transaction `INPUT_NOTES_COMMITMENT` for transaction `tx_index`.
    pub fn tx_input_notes_commitment(&self, tx_index: usize) -> Word {
        self.tx_headers[tx_index].input_notes_commitment
    }

    /// Returns the verified per-transaction `OUTPUT_NOTES_COMMITMENT` for transaction `tx_index`.
    pub fn tx_output_notes_commitment(&self, tx_index: usize) -> Word {
        self.tx_headers[tx_index].output_notes_commitment
    }
}
