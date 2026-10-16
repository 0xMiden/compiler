//! Batch state shared between the kernel phases.
//!
//! Mirrors `asm/kernels/batch/lib/memory.masm` from the protocol batch kernel
//! (0xMiden/protocol#2905). The MASM module lays the batch state out as flat felt regions
//! (transaction tuples, transaction headers, sorted note lists and their parallel flag arrays)
//! and exposes accessors over them; the Rust equivalent keeps the same flat felt layout in
//! [`BatchMemory`]'s buffers — the advice data is stored exactly as piped, with word accessors
//! over it — which avoids re-decoding the piped data into per-entry structures. The
//! per-transaction note scratch region has no counterpart here: each transaction's notes are
//! decoded into a temporary that is dropped when the transaction has been processed.

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

/// Number of felts in a word.
pub const WORD_FELT_LEN: usize = 4;

/// Number of felts each transaction tuple occupies in the Layer 1 piped data.
pub const TX_TUPLE_FELT_LEN: usize = 8;

/// Number of words each transaction tuple occupies in the Layer 1 piped data.
pub const TX_TUPLE_WORD_LEN: usize = TX_TUPLE_FELT_LEN / WORD_FELT_LEN;

/// Number of felts each transaction header occupies in the Layer 2 piped data. This must match
/// the felt-sequence layout of `TransactionId::new`.
pub const TX_HEADER_FELT_LEN: usize = 16;

/// Number of words each transaction header occupies in the Layer 2 piped data.
pub const TX_HEADER_WORD_LEN: usize = TX_HEADER_FELT_LEN / WORD_FELT_LEN;

/// Number of felts each note occupies in a sorted note list entry (KEY word + VALUE word).
pub const NOTE_ENTRY_FELT_LEN: usize = 8;

/// A sorted note list entry: the KEY word followed by the VALUE word.
pub type NoteEntry = [Felt; NOTE_ENTRY_FELT_LEN];

/// An input-note list entry's flags: `[erasure, consumed]`.
pub type InputFlags = [Felt; 2];

/// An output-note list entry's flags: `[will_be_erased, is_created, linked_input_index]`.
pub type OutputFlags = [Felt; 3];

// ERASURE FLAG VALUES
// =================================================================================================

// The flags are `Felt`-valued like the felts of the MASM flag words. This also keeps every flag
// test a VM felt comparison: integer-typed flags invite LLVM to merge the flag tests into a
// branch table, a shape the Miden backend does not currently lower correctly.

/// `erasure` flag value: not erased / external.
#[inline]
pub fn erasure_not_erased() -> Felt {
    felt!(0)
}

/// `erasure` flag value: expected to be erased: the input note's id matches an output note
/// created by another transaction within this batch -- its "creator" -- which has not been
/// processed yet.
#[inline]
pub fn erasure_expected() -> Felt {
    felt!(1)
}

/// `erasure` flag value: erased: the creator output note has been processed.
#[inline]
pub fn erasure_erased() -> Felt {
    felt!(2)
}

// FLAT LIST ACCESSORS
// =================================================================================================

// The accessors view flat buffers as fixed-size arrays (`as_chunks`) and index those: one bounds
// check per entry, and the words inside an entry are reached at constant offsets with no checks
// at all, where slicing `buffer[start..start + 4]` costs an overflow check, two bounds compares
// and a length check per word.

/// Views a flat note list as its entries. The list length is a multiple of the entry length (the
/// loaders assert it), so no felts are left over.
#[inline(always)]
pub fn note_entries(list: &[Felt]) -> &[NoteEntry] {
    list.as_chunks().0
}

/// Returns the KEY word (the first 4 felts) of a note entry.
#[inline(always)]
pub fn entry_key(entry: &NoteEntry) -> &[Felt; 4] {
    entry.first_chunk().unwrap()
}

/// Returns the VALUE word (the last 4 felts) of a note entry.
#[inline(always)]
pub fn entry_value(entry: &NoteEntry) -> &[Felt; 4] {
    entry.last_chunk().unwrap()
}

/// Returns the KEY word of entry `index` of a flat note list.
#[inline(always)]
pub fn note_key(list: &[Felt], index: usize) -> &[Felt; 4] {
    entry_key(&note_entries(list)[index])
}

/// Returns the VALUE word of entry `index` of a flat note list.
#[inline(always)]
pub fn note_value(list: &[Felt], index: usize) -> &[Felt; 4] {
    entry_value(&note_entries(list)[index])
}

/// Returns word `index` of a flat felt buffer whose length is a multiple of the word length.
#[inline(always)]
pub fn nth_word(buffer: &[Felt], index: usize) -> &[Felt; 4] {
    &buffer.as_chunks().0[index]
}

/// Copies a 4-felt word into a [`Word`] (needed where an intrinsic takes a `Word` by value).
#[inline(always)]
pub fn to_word(felts: &[Felt; 4]) -> Word {
    Word::from(*felts)
}

/// Returns whether the 4-felt word `word` is the empty word.
#[inline(always)]
pub fn is_empty_word(word: &[Felt; 4]) -> bool {
    word[0] == felt!(0) && word[1] == felt!(0) && word[2] == felt!(0) && word[3] == felt!(0)
}

// BATCH STATE
// =================================================================================================

/// The batch state produced by the prologue and updated by the note tracker.
///
/// Each buffer corresponds to a memory region of `memory.masm` and holds the felts exactly as
/// piped from the advice provider; the region's entry count (`num_transactions`,
/// `num_input_notes`, `num_output_notes`) is the buffer length divided by the entry stride.
pub struct BatchMemory {
    /// Layer 1 piped data: the verified `(tx_id, account_id)` tuples, in batch order
    /// ([`TX_TUPLE_FELT_LEN`] felts per transaction). The account pair is verified as part of
    /// the Layer 1 pre-image but not otherwise used until account updates are aggregated.
    pub tx_tuples: Vec<Felt>,
    /// Layer 2 piped data: the verified per-transaction headers, in batch order, each kept as
    /// piped ([`TX_HEADER_FELT_LEN`] felts: `[INIT, FINAL, INPUT_NOTES_COMMITMENT,
    /// OUTPUT_NOTES_COMMITMENT]`). The account commitments are verified as part of the pre-image
    /// but not otherwise used until account updates are aggregated.
    pub tx_headers: Vec<Vec<Felt>>,
    /// The nullifier-sorted input-note list: 8-felt `[KEY, VALUE]` entries where KEY is the
    /// nullifier and VALUE is the note id (for unauthenticated notes) or the empty word. The
    /// VALUE word doubles as the second half of the `(nullifier, note_id_or_empty)` tuple hashed
    /// into `INPUT_NOTES_COMMITMENT`.
    pub input_notes: Vec<Felt>,
    /// The note-id-sorted output-note list: 8-felt `[KEY, VALUE]` entries where KEY is the note
    /// id; the VALUE word is unused.
    pub output_notes: Vec<Felt>,
    /// The input-note flags, parallel to [`Self::input_notes`].
    pub input_flags: Vec<InputFlags>,
    /// The output-note flags, parallel to [`Self::output_notes`].
    pub output_flags: Vec<OutputFlags>,
    /// The running minimum of the transactions' expiration block numbers.
    pub batch_expiration_block_num: Felt,
}

impl BatchMemory {
    /// Returns `num_transactions`.
    #[inline(always)]
    pub fn num_transactions(&self) -> usize {
        self.tx_tuples.len() / TX_TUPLE_FELT_LEN
    }

    /// Returns the number of entries in the nullifier-sorted input-note list.
    #[inline(always)]
    pub fn num_input_notes(&self) -> usize {
        self.input_notes.len() / NOTE_ENTRY_FELT_LEN
    }

    /// Returns the verified `TransactionId` of transaction `tx_index`.
    #[inline(always)]
    pub fn tx_id(&self, tx_index: usize) -> Word {
        to_word(nth_word(&self.tx_tuples, tx_index * TX_TUPLE_WORD_LEN))
    }

    /// Returns the verified per-transaction `INPUT_NOTES_COMMITMENT` for transaction `tx_index`.
    #[inline(always)]
    pub fn tx_input_notes_commitment(&self, tx_index: usize) -> &[Felt; 4] {
        nth_word(&self.tx_headers[tx_index], 2)
    }

    /// Returns the verified per-transaction `OUTPUT_NOTES_COMMITMENT` for transaction `tx_index`.
    #[inline(always)]
    pub fn tx_output_notes_commitment(&self, tx_index: usize) -> &[Felt; 4] {
        nth_word(&self.tx_headers[tx_index], 3)
    }

    /// Returns input-note entry `index`'s `erasure` flag.
    #[inline(always)]
    pub fn input_note_erasure(&self, index: usize) -> Felt {
        self.input_flags[index][0]
    }

    /// Sets input-note entry `index`'s `erasure` flag.
    #[inline(always)]
    pub fn set_input_note_erasure(&mut self, index: usize, value: Felt) {
        self.input_flags[index][0] = value;
    }

    /// Returns input-note entry `index`'s `consumed` flag.
    #[inline(always)]
    pub fn input_note_consumed(&self, index: usize) -> Felt {
        self.input_flags[index][1]
    }

    /// Sets input-note entry `index`'s `consumed` flag.
    #[inline(always)]
    pub fn set_input_note_consumed(&mut self, index: usize, value: Felt) {
        self.input_flags[index][1] = value;
    }

    /// Returns output-note entry `index`'s `will_be_erased` flag.
    #[inline(always)]
    pub fn output_note_will_be_erased(&self, index: usize) -> Felt {
        self.output_flags[index][0]
    }

    /// Sets output-note entry `index`'s `will_be_erased` flag.
    #[inline(always)]
    pub fn set_output_note_will_be_erased(&mut self, index: usize, value: Felt) {
        self.output_flags[index][0] = value;
    }

    /// Returns output-note entry `index`'s `is_created` flag.
    #[inline(always)]
    pub fn output_note_created(&self, index: usize) -> Felt {
        self.output_flags[index][1]
    }

    /// Sets output-note entry `index`'s `is_created` flag.
    #[inline(always)]
    pub fn set_output_note_created(&mut self, index: usize, value: Felt) {
        self.output_flags[index][1] = value;
    }

    /// Returns the input-note list index linked to output-note entry `index`. Meaningful only
    /// when the entry's `will_be_erased` flag is set (which implies the link was written).
    #[inline(always)]
    pub fn output_note_linked_input(&self, index: usize) -> usize {
        self.output_flags[index][2].as_canonical_u64() as usize
    }

    /// Links output-note entry `index` to input-note list entry `input_index`.
    #[inline(always)]
    pub fn set_output_note_linked_input(&mut self, index: usize, input_index: usize) {
        self.output_flags[index][2] = Felt::from(input_index as u32);
    }
}
