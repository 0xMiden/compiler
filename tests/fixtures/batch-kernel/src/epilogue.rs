//! Batch kernel epilogue: verifies the note-tracking results and computes the batch's note
//! commitments.
//!
//! Mirrors `asm/kernels/batch/lib/epilogue.masm` from the protocol batch kernel
//! (0xMiden/protocol#2905).

extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, felt, hash_elements};

use crate::memory::{
    BatchMemory, INPUT_FLAGS_STRIDE, NOTE_ENTRY_FELT_LEN, OUTPUT_FLAGS_STRIDE, erasure_erased,
    erasure_expected,
};

// ASSERTIONS
// =================================================================================================

/// Asserts every output-note list entry was created by exactly one transaction (with the per-tx
/// binding, this proves the list is exactly the union of the per-transaction output notes, so a
/// host cannot fabricate an erasure with a note no transaction creates).
///
/// The matching input-note checks (every entry consumed exactly once, none left pending-erasure)
/// are folded into the single pass in [`compute_input_notes_commitment`].
#[inline(always)]
fn assert_all_output_notes_created(memory: &BatchMemory) {
    for flags in memory.output_note_flags().chunks_exact(OUTPUT_FLAGS_STRIDE) {
        assert!(
            flags[1] != felt!(0),
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
/// The pass over the input entries also enforces the epilogue invariants on each entry: it was
/// consumed exactly once and is not left expected-to-be-erased (i.e. any created-and-consumed
/// note had its creator processed).
///
/// The MASM kernel absorbs the entries into an incremental hasher state persisted in memory;
/// here the sorted note list buffer is handed to `hash_elements` whole in the common no-erasure
/// case (which is why this takes `BatchMemory` by value), and only batches with erased notes
/// collect the surviving entries into a fresh buffer first. Both produce the same sequential
/// hash (matching `Hasher::hash_elements`).
#[inline(always)]
fn compute_input_notes_commitment(memory: BatchMemory) -> Word {
    let num_notes = memory.num_input_notes();
    let mut num_erased = 0;

    for flags in memory.input_note_flags().chunks_exact(INPUT_FLAGS_STRIDE) {
        // Assert this entry was consumed exactly once and is not left expected-to-be-erased.
        assert!(
            flags[1] != felt!(0),
            "an input-note list entry was not consumed by any transaction"
        );
        let erasure = flags[0];
        assert!(
            erasure != erasure_expected(),
            "an erased input note was consumed before the transaction that creates it"
        );
        if erasure == erasure_erased() {
            num_erased += 1;
        }
    }

    // With no non-erased entries the commitment is the empty word, matching the early return in
    // `build_input_note_commitment` (this is not the hash of zero elements).
    if num_erased == num_notes {
        return Word::empty();
    }

    // Common case: nothing erased, hash the sorted note list buffer as-is.
    if num_erased == 0 {
        return Word::from(hash_elements(memory.input_notes));
    }

    // Some entries were erased: collect the surviving entries and hash those.
    let mut elements: Vec<Felt> =
        Vec::with_capacity((num_notes - num_erased) * NOTE_ENTRY_FELT_LEN);
    for (entry, flags) in memory
        .input_notes
        .chunks_exact(NOTE_ENTRY_FELT_LEN)
        .zip(memory.input_note_flags().chunks_exact(INPUT_FLAGS_STRIDE))
    {
        if flags[0] != erasure_erased() {
            elements.extend_from_slice(entry);
        }
    }
    Word::from(hash_elements(elements))
}

// OUTPUT NOTES COMMITMENT
// =================================================================================================

/// Computes the batch's output-notes commitment (the batch note tree root).
///
/// Placeholder: returns the empty word until the batch note tree is wired up.
///
/// TODO: hash the batch's output notes into the batch note tree (SMT) root.
#[inline(always)]
fn compute_output_notes_commitment(_memory: &BatchMemory) -> Word {
    Word::empty()
}

// EPILOGUE
// =================================================================================================

/// Verifies the note-tracking results and computes the batch's note commitments, returned as
/// `(INPUT_NOTES_COMMITMENT, OUTPUT_NOTES_COMMITMENT)`. Consumes the batch state (the input-note
/// buffer becomes the hash input in the no-erasure case).
///
/// Asserts every output-note list entry was created, then computes the input- and output-notes
/// commitments. The per-input-entry invariants (consumed exactly once, no pending erasure) are
/// enforced inside the input-commitment pass.
///
/// TODO: authenticate unauthenticated, non-erased input notes against BLOCK_COMMITMENT's chain
///       MMR.
#[inline(always)]
pub fn finalize(memory: BatchMemory) -> (Word, Word) {
    assert_all_output_notes_created(&memory);
    let output_notes_commitment = compute_output_notes_commitment(&memory);
    let input_notes_commitment = compute_input_notes_commitment(memory);
    (input_notes_commitment, output_notes_commitment)
}
