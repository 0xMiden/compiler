//! Batch kernel note tracker: determines note erasure and binds the batch note lists to the
//! verified per-transaction notes.
//!
//! Mirrors `asm/kernels/batch/lib/note_tracker.masm` from the protocol batch kernel
//! (0xMiden/protocol#2905).

extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{
    Digest, Felt, Word, adv_load_preimage, assert_eq, felt,
    intrinsics::{advice::adv_push_mapvaln, crypto::merge},
};

use crate::memory::{
    BatchMemory, MAX_NOTES_PER_BATCH, NOTE_ENTRY_FELT_LEN, NoteEntry, erasure_erased,
    erasure_expected,
};

// SORTED NOTE LIST LOOKUP
// =================================================================================================

/// Finds `key` in a sorted note list, returning the entry index if present. The Rust counterpart
/// of the MASM kernel's `sorted_array::find_key_value` lookups.
///
/// The binary search is written with [`word_lt`] instead of `Ord`-based `binary_search_by` so
/// that no `core::cmp::Ordering` value is materialized (its `Less` discriminant is -1, which the
/// compiler currently rejects when it round-trips through an i32-checked cast).
fn find_key(entries: &[NoteEntry], key: &Word) -> Option<usize> {
    let mut lo = 0;
    let mut hi = entries.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let mid_key = &entries[mid].key;
        if word_lt(mid_key, key) {
            lo = mid + 1;
        } else if word_lt(key, mid_key) {
            hi = mid;
        } else {
            return Some(mid);
        }
    }
    None
}

/// Compares two words, most-significant felt first. The Rust counterpart of the MASM kernel's
/// `word::lt` (and the same order as `Word`'s host-side `Ord`).
pub fn word_lt(a: &Word, b: &Word) -> bool {
    let mut i = 4;
    while i > 0 {
        i -= 1;
        if a[i] < b[i] {
            return true;
        }
        if b[i] < a[i] {
            return false;
        }
    }
    false
}

/// Asserts two words are equal, felt by felt. The Rust counterpart of `assert_eqw`.
fn assert_eq_word(a: Word, b: Word) {
    assert_eq(a[0], b[0]);
    assert_eq(a[1], b[1]);
    assert_eq(a[2], b[2]);
    assert_eq(a[3], b[3]);
}

/// Pipes a transaction's note tuples (8-felt entries) from the advice map into memory, asserting
/// their sequential hash equals `commitment`, and returns the decoded entries. The Rust
/// counterpart of the per-transaction scratch region loads in `process_tx_input_notes` /
/// `process_tx_output_notes`.
fn load_tx_note_tuples(commitment: Word) -> Vec<NoteEntry> {
    let len_felts = adv_push_mapvaln(commitment).as_canonical_u64() as usize;
    assert!(
        len_felts.is_multiple_of(NOTE_ENTRY_FELT_LEN),
        "a transaction note tuple list length is not a multiple of the note entry length"
    );
    let num_notes = len_felts / NOTE_ENTRY_FELT_LEN;
    assert!(
        num_notes <= MAX_NOTES_PER_BATCH,
        "a transaction's note set contains more notes than the maximum allowed"
    );

    let data = adv_load_preimage(Felt::from((len_felts / 4) as u32), commitment);

    let mut entries = Vec::with_capacity(num_notes);
    for chunk in data.chunks_exact(NOTE_ENTRY_FELT_LEN) {
        entries.push(NoteEntry {
            key: Word::from([chunk[0], chunk[1], chunk[2], chunk[3]]),
            value: Word::from([chunk[4], chunk[5], chunk[6], chunk[7]]),
        });
    }
    entries
}

// ERASURE CROSS-REFERENCE
// =================================================================================================

/// For input-note list entry `idx`: if it is unauthenticated (its note id is non-empty) and that
/// note id appears in the output-note list, marks the input entry erasure-expected and links the
/// matching output entry back to it. This is the static (order-independent) erasure
/// determination; the temporal ordering is enforced during per-transaction processing.
///
/// If two input entries carry the same note id (impossible for real notes, whose nullifier is
/// derived from the note), the second link overwrites the first and the kernel later aborts.
fn cross_reference_one_input(memory: &mut BatchMemory, idx: usize) {
    let note_id = memory.input_notes[idx].value;
    if note_id == Word::empty() {
        return;
    }
    if let Some(j) = find_key(&memory.output_notes, &note_id) {
        memory.input_note_flags[idx].erasure = erasure_expected();
        memory.output_note_flags[j].will_be_erased = felt!(1);
        memory.output_note_flags[j].linked_input_index = idx;
    }
}

/// Cross-references the input and output note lists to determine erasure (see
/// [`cross_reference_one_input`]).
fn cross_reference_erasure(memory: &mut BatchMemory) {
    for idx in 0..memory.input_notes.len() {
        cross_reference_one_input(memory, idx);
    }
}

// PER-TRANSACTION OUTPUT NOTES
// =================================================================================================

/// Marks output-note list entry `j` as created, asserting it was not already created.
fn mark_output_note_created(memory: &mut BatchMemory, j: usize) {
    assert!(
        memory.output_note_flags[j].is_created == felt!(0),
        "a note-id-sorted output-note list entry was created by more than one transaction"
    );
    memory.output_note_flags[j].is_created = felt!(1);
}

/// Advances input-note list entry `idx`'s erasure flag from expected (1) to erased (2) — its
/// creator output note has been processed — asserting it was expected.
fn flip_input_erasure_created(memory: &mut BatchMemory, idx: usize) {
    assert!(
        memory.input_note_flags[idx].erasure == erasure_expected(),
        "an erased input note was consumed before the transaction that creates it"
    );
    memory.input_note_flags[idx].erasure = erasure_erased();
}

/// Binds one per-transaction output note (a `[DETAILS_COMMITMENT, METADATA_COMMITMENT]` tuple) to
/// the note-id-sorted output-note list: derives its note id, looks it up, marks it created, and —
/// if it erases an input note — advances that input note's erasure flag.
fn bind_one_output_note(memory: &mut BatchMemory, note: &NoteEntry) {
    // Derive the output note's id as merge(details_commitment, metadata_commitment).
    let details_commitment = Digest::from_word(note.key);
    let metadata_commitment = Digest::from_word(note.value);
    let note_id = Word::from(merge([details_commitment, metadata_commitment]));

    // Look the note id up in the note-id-sorted output-note list and mark the entry created.
    let j = find_key(&memory.output_notes, &note_id)
        .expect("a transaction output note is missing from the note-id-sorted output-note list");
    mark_output_note_created(memory, j);

    // If this output note erases an input note (cross-referenced earlier), advance that input
    // note's erasure flag from expected to erased (creator processed).
    if memory.output_note_flags[j].will_be_erased != felt!(0) {
        let linked_input_index = memory.output_note_flags[j].linked_input_index;
        flip_input_erasure_created(memory, linked_input_index);
    }
}

/// Verifies transaction `tx_index`'s `OUTPUT_NOTES_COMMITMENT_idx` against its piped
/// `(DETAILS_COMMITMENT, METADATA_COMMITMENT)` tuples, then binds each of those notes to the
/// batch output-note list (see [`bind_per_tx_notes`] for what binding proves).
fn process_tx_output_notes(memory: &mut BatchMemory, tx_index: usize) {
    let commitment = memory.tx_output_notes_commitment(tx_index);
    if commitment == Word::empty() {
        // Empty commitment: the transaction has no output notes, so there is nothing to bind.
        return;
    }
    let notes = load_tx_note_tuples(commitment);
    for note in notes.iter() {
        bind_one_output_note(memory, note);
    }
}

// PER-TRANSACTION INPUT NOTES
// =================================================================================================

/// Marks input-note list entry `idx` as consumed, asserting it was not already consumed.
fn mark_input_note_consumed(memory: &mut BatchMemory, idx: usize) {
    assert!(
        memory.input_note_flags[idx].consumed == felt!(0),
        "a nullifier-sorted input-note list entry was consumed by more than one transaction"
    );
    memory.input_note_flags[idx].consumed = felt!(1);
}

/// Asserts input-note list entry `idx` is not expected-to-be-erased at the point it is consumed:
/// if it is created in this batch, its creator has already been processed. Rejects
/// consume-before-create / circular dependencies.
fn assert_input_not_consumed_before_created(memory: &BatchMemory, idx: usize) {
    assert!(
        memory.input_note_flags[idx].erasure != erasure_expected(),
        "an erased input note was consumed before the transaction that creates it"
    );
}

/// Binds one per-transaction input note (a `[NULLIFIER, NOTE_ID_OR_EMPTY]` tuple) to the
/// nullifier-sorted input-note list: looks it up by nullifier, asserts it is present and that its
/// note id matches, enforces the erasure ordering gate, then marks it consumed.
fn bind_one_input_note(memory: &mut BatchMemory, note: &NoteEntry) {
    // Look the per-transaction note up in the nullifier-sorted input-note list by its nullifier.
    let idx = find_key(&memory.input_notes, &note.key)
        .expect("a transaction input note is missing from the nullifier-sorted input-note list");

    // Assert the list entry's note id equals the per-transaction note's, binding the id as well
    // as the nullifier (so an unauthenticated note cannot be matched to the wrong list entry).
    assert_eq_word(memory.input_notes[idx].value, note.value);

    // Enforce the erasure ordering gate (reject consume-before-create), then mark the entry
    // consumed.
    assert_input_not_consumed_before_created(memory, idx);
    mark_input_note_consumed(memory, idx);
}

/// Verifies transaction `tx_index`'s `INPUT_NOTES_COMMITMENT_idx` against its piped
/// `(NULLIFIER, NOTE_ID_OR_EMPTY)` tuples, then binds each of those notes to the batch
/// input-note list (see [`bind_per_tx_notes`] for what binding proves).
fn process_tx_input_notes(memory: &mut BatchMemory, tx_index: usize) {
    let commitment = memory.tx_input_notes_commitment(tx_index);
    if commitment == Word::empty() {
        // Empty commitment: the transaction has no input notes, so there is nothing to bind.
        return;
    }
    let notes = load_tx_note_tuples(commitment);
    for note in notes.iter() {
        bind_one_input_note(memory, note);
    }
}

/// Processes every transaction in batch order, binding its input notes then its output notes.
///
/// Binding a per-transaction note means locating it in the batch's sorted note list — by
/// nullifier for inputs, by note id for outputs — and marking the matching entry (consumed for
/// inputs, created for outputs). Because every per-transaction note must be found and each entry
/// may be marked at most once, binding proves the host-provided list is exactly the multiset of
/// per-transaction notes: the host cannot inject, omit, or duplicate entries.
///
/// Inputs are bound before outputs within a transaction so a note created and consumed by the
/// same transaction is rejected — at consume time its erasure flag is still `Expected` (its
/// creating output has not been processed), tripping the gate in
/// [`assert_input_not_consumed_before_created`].
fn bind_per_tx_notes(memory: &mut BatchMemory) {
    for tx_index in 0..memory.num_transactions() {
        process_tx_input_notes(memory, tx_index);
        process_tx_output_notes(memory, tx_index);
    }
}

// NOTE TRACKING
// =================================================================================================

/// Tracks the batch's notes: determines erasure by cross-referencing the prepared batch input-
/// and output-note lists, then binds both lists to the verified per-transaction notes while
/// enforcing the creator-before-consumer ordering gate. Assumes the prologue has loaded and
/// strict-sorted the two sorted note lists into memory.
pub fn track_notes(memory: &mut BatchMemory) {
    cross_reference_erasure(memory);
    bind_per_tx_notes(memory);
}
