//! Batch kernel epilogue: verifies the note-tracking results and computes the batch's note
//! commitments.
//!
//! Mirrors `asm/kernels/batch/lib/epilogue.masm` from the protocol batch kernel
//! (0xMiden/protocol#2905).

extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, felt, hash_elements};

use crate::memory::{BatchMemory, NOTE_ENTRY_FELT_LEN, erasure_erased, erasure_expected};

// ASSERTIONS
// =================================================================================================

/// Asserts every output-note list entry was created by exactly one transaction (with the per-tx
/// binding, this proves the list is exactly the union of the per-transaction output notes, so a
/// host cannot fabricate an erasure with a note no transaction creates).
///
/// The matching input-note checks (every entry consumed exactly once, none left pending-erasure)
/// are folded into the single pass in [`compute_input_notes_commitment`].
fn assert_all_output_notes_created(memory: &BatchMemory) {
    for flags in memory.output_note_flags.iter() {
        assert!(
            flags.is_created != felt!(0),
            "an output-note list entry was not created by any transaction"
        );
    }
}

// INPUT NOTES COMMITMENT
// =================================================================================================

/// Computes `INPUT_NOTES_COMMITMENT` as the sequential hash of the non-erased
/// `(NULLIFIER, NOTE_ID_OR_EMPTY)` entries of the nullifier-sorted input-note list (entries with
/// erasure flag `Erased` are skipped).
///
/// The single pass over the input entries also enforces the epilogue invariants on each entry: it
/// was consumed exactly once and is not left expected-to-be-erased (i.e. any created-and-consumed
/// note had its creator processed).
///
/// The MASM kernel absorbs the entries into an incremental hasher state persisted in memory; here
/// the non-erased entries are collected and hashed in one call, producing the same sequential
/// hash (matching `Hasher::hash_elements`).
fn compute_input_notes_commitment(memory: &BatchMemory) -> Word {
    let mut elements: Vec<Felt> =
        Vec::with_capacity(memory.input_notes.len() * NOTE_ENTRY_FELT_LEN);
    let mut absorbed_count = 0;

    for (entry, flags) in memory.input_notes.iter().zip(memory.input_note_flags.iter()) {
        // Assert this entry was consumed exactly once and is not left expected-to-be-erased.
        assert!(
            flags.consumed != felt!(0),
            "an input-note list entry was not consumed by any transaction"
        );
        assert!(
            flags.erasure != erasure_expected(),
            "an erased input note was consumed before the transaction that creates it"
        );

        // Absorb the entry unless it was erased (created-and-consumed in this batch).
        if flags.erasure != erasure_erased() {
            elements.extend_from_slice(entry.key.as_elements());
            elements.extend_from_slice(entry.value.as_elements());
            absorbed_count += 1;
        }
    }

    // With no non-erased entries the commitment is the empty word, matching the early return in
    // `build_input_note_commitment` (this is not the hash of zero elements).
    if absorbed_count == 0 {
        Word::empty()
    } else {
        Word::from(hash_elements(elements))
    }
}

// OUTPUT NOTES COMMITMENT
// =================================================================================================

/// Computes the batch's output-notes commitment (the batch note tree root).
///
/// Placeholder: returns the empty word until the batch note tree is wired up.
///
/// TODO: hash the batch's output notes into the batch note tree (SMT) root.
fn compute_output_notes_commitment(_memory: &BatchMemory) -> Word {
    Word::empty()
}

// EPILOGUE
// =================================================================================================

/// Verifies the note-tracking results and computes the batch's note commitments, returned as
/// `(INPUT_NOTES_COMMITMENT, OUTPUT_NOTES_COMMITMENT)`.
///
/// Asserts every output-note list entry was created, then computes the input- and output-notes
/// commitments. The per-input-entry invariants (consumed exactly once, no pending erasure) are
/// enforced inside the input-commitment pass.
///
/// TODO: authenticate unauthenticated, non-erased input notes against BLOCK_COMMITMENT's chain
///       MMR.
pub fn finalize(memory: &BatchMemory) -> (Word, Word) {
    assert_all_output_notes_created(memory);
    let output_notes_commitment = compute_output_notes_commitment(memory);
    let input_notes_commitment = compute_input_notes_commitment(memory);
    (input_notes_commitment, output_notes_commitment)
}
