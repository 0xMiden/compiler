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
    self, BatchMemory, MAX_NOTES_PER_BATCH, NOTE_ENTRY_FELT_LEN, erasure_erased, erasure_expected,
};

// SORTED NOTE LIST LOOKUP
// =================================================================================================

/// Finds `key` in a flat sorted note list, returning the entry index if present. The Rust
/// counterpart of the MASM kernel's `sorted_array::find_key_value` lookups.
///
/// The binary search is written with [`word_lt`] instead of `Ord`-based `binary_search_by` so
/// that no `core::cmp::Ordering` value is materialized (its `Less` discriminant is -1, which the
/// compiler currently rejects when it round-trips through an i32-checked cast).
///
/// Inlined into its callers: as an outlined procedure its search state spills into
/// memory-backed VM locals, which costs more than the lookup itself.
#[inline(always)]
fn find_key(list: &[Felt], key: &[Felt; 4]) -> Option<usize> {
    let mut lo = 0;
    let mut hi = list.len() / NOTE_ENTRY_FELT_LEN;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let mid_key = memory::note_key(list, mid);
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
///
/// Written as straight-line code over 4-felt arrays: constant indices need no bounds checks,
/// while an index loop compiles to bounds-checked dynamic indexing and memory-backed loop state,
/// which costs an order of magnitude more VM cycles than these felt comparisons themselves.
#[inline(always)]
pub fn word_lt(a: &[Felt; 4], b: &[Felt; 4]) -> bool {
    if a[3] != b[3] {
        return a[3] < b[3];
    }
    if a[2] != b[2] {
        return a[2] < b[2];
    }
    if a[1] != b[1] {
        return a[1] < b[1];
    }
    a[0] < b[0]
}

/// Asserts two words are equal, felt by felt. The Rust counterpart of `assert_eqw`.
#[inline(always)]
fn assert_eq_word(a: &[Felt; 4], b: &[Felt; 4]) {
    assert_eq(a[0], b[0]);
    assert_eq(a[1], b[1]);
    assert_eq(a[2], b[2]);
    assert_eq(a[3], b[3]);
}

/// Pipes a transaction's note tuples (8-felt entries) from the advice map into memory, asserting
/// their sequential hash equals `commitment`, and returns them as piped. The Rust counterpart of
/// the per-transaction scratch region loads in `process_tx_input_notes` /
/// `process_tx_output_notes`.
#[inline(always)]
fn load_tx_note_tuples(commitment: Word) -> Vec<Felt> {
    let len_felts = adv_push_mapvaln(commitment).as_canonical_u64() as usize;
    assert!(
        len_felts.is_multiple_of(NOTE_ENTRY_FELT_LEN),
        "a transaction note tuple list length is not a multiple of the note entry length"
    );
    assert!(
        len_felts / NOTE_ENTRY_FELT_LEN <= MAX_NOTES_PER_BATCH,
        "a transaction's note set contains more notes than the maximum allowed"
    );

    adv_load_preimage(Felt::from((len_felts / 4) as u32), commitment)
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
#[inline(always)]
fn cross_reference_one_input(memory: &mut BatchMemory, idx: usize) {
    let note_id = memory::note_value(&memory.input_notes, idx);
    if memory::is_empty_word(note_id) {
        return;
    }
    if let Some(j) = find_key(&memory.output_notes, note_id) {
        memory.set_input_note_erasure(idx, erasure_expected());
        memory.set_output_note_will_be_erased(j, felt!(1));
        memory.set_output_note_linked_input(j, idx);
    }
}

/// Cross-references the input and output note lists to determine erasure (see
/// [`cross_reference_one_input`]).
#[inline(always)]
fn cross_reference_erasure(memory: &mut BatchMemory) {
    for idx in 0..memory.num_input_notes() {
        cross_reference_one_input(memory, idx);
    }
}

// PER-TRANSACTION OUTPUT NOTES
// =================================================================================================

/// Marks output-note list entry `j` as created, asserting it was not already created.
#[inline(always)]
fn mark_output_note_created(memory: &mut BatchMemory, j: usize) {
    assert!(
        memory.output_note_created(j) == felt!(0),
        "a note-id-sorted output-note list entry was created by more than one transaction"
    );
    memory.set_output_note_created(j, felt!(1));
}

/// Advances input-note list entry `idx`'s erasure flag from expected (1) to erased (2) — its
/// creator output note has been processed — asserting it was expected.
#[inline(always)]
fn flip_input_erasure_created(memory: &mut BatchMemory, idx: usize) {
    assert!(
        memory.input_note_erasure(idx) == erasure_expected(),
        "an erased input note was consumed before the transaction that creates it"
    );
    memory.set_input_note_erasure(idx, erasure_erased());
}

/// Binds one per-transaction output note (a `[DETAILS_COMMITMENT, METADATA_COMMITMENT]` tuple) to
/// the note-id-sorted output-note list: derives its note id, looks it up, marks it created, and —
/// if it erases an input note — advances that input note's erasure flag.
#[inline(always)]
fn bind_one_output_note(memory: &mut BatchMemory, note: &[Felt]) {
    // Derive the output note's id as merge(details_commitment, metadata_commitment).
    let details_commitment = Digest::from_word(memory::load_word(note, 0));
    let metadata_commitment = Digest::from_word(memory::load_word(note, 4));
    let note_id = Word::from(merge([details_commitment, metadata_commitment]));
    let note_id = [note_id[0], note_id[1], note_id[2], note_id[3]];

    // Look the note id up in the note-id-sorted output-note list and mark the entry created.
    let j = find_key(&memory.output_notes, &note_id)
        .expect("a transaction output note is missing from the note-id-sorted output-note list");
    mark_output_note_created(memory, j);

    // If this output note erases an input note (cross-referenced earlier), advance that input
    // note's erasure flag from expected to erased (creator processed).
    if memory.output_note_will_be_erased(j) != felt!(0) {
        let linked_input_index = memory.output_note_linked_input(j);
        flip_input_erasure_created(memory, linked_input_index);
    }
}

/// Verifies transaction `tx_index`'s `OUTPUT_NOTES_COMMITMENT_idx` against its piped
/// `(DETAILS_COMMITMENT, METADATA_COMMITMENT)` tuples, then binds each of those notes to the
/// batch output-note list (see [`bind_per_tx_notes`] for what binding proves).
#[inline(always)]
fn process_tx_output_notes(memory: &mut BatchMemory, tx_index: usize) {
    if memory::is_empty_word(memory.tx_output_notes_commitment(tx_index)) {
        // Empty commitment: the transaction has no output notes, so there is nothing to bind.
        return;
    }
    let commitment =
        memory::load_word(&memory.tx_headers, tx_index * crate::memory::TX_HEADER_FELT_LEN + 12);
    let notes = load_tx_note_tuples(commitment);
    for note in notes.chunks_exact(NOTE_ENTRY_FELT_LEN) {
        bind_one_output_note(memory, note);
    }
}

// PER-TRANSACTION INPUT NOTES
// =================================================================================================

/// Marks input-note list entry `idx` as consumed, asserting it was not already consumed.
#[inline(always)]
fn mark_input_note_consumed(memory: &mut BatchMemory, idx: usize) {
    assert!(
        memory.input_note_consumed(idx) == felt!(0),
        "a nullifier-sorted input-note list entry was consumed by more than one transaction"
    );
    memory.set_input_note_consumed(idx, felt!(1));
}

/// Asserts input-note list entry `idx` is not expected-to-be-erased at the point it is consumed:
/// if it is created in this batch, its creator has already been processed. Rejects
/// consume-before-create / circular dependencies.
#[inline(always)]
fn assert_input_not_consumed_before_created(memory: &BatchMemory, idx: usize) {
    assert!(
        memory.input_note_erasure(idx) != erasure_expected(),
        "an erased input note was consumed before the transaction that creates it"
    );
}

/// Binds one per-transaction input note (a `[NULLIFIER, NOTE_ID_OR_EMPTY]` tuple) to the
/// nullifier-sorted input-note list: looks it up by nullifier, asserts it is present and that its
/// note id matches, enforces the erasure ordering gate, then marks it consumed.
#[inline(always)]
fn bind_one_input_note(memory: &mut BatchMemory, note: &[Felt]) {
    // Look the per-transaction note up in the nullifier-sorted input-note list by its nullifier.
    let idx = find_key(&memory.input_notes, memory::word_at(note, 0))
        .expect("a transaction input note is missing from the nullifier-sorted input-note list");

    // Assert the list entry's note id equals the per-transaction note's, binding the id as well
    // as the nullifier (so an unauthenticated note cannot be matched to the wrong list entry).
    assert_eq_word(memory::note_value(&memory.input_notes, idx), memory::word_at(note, 4));

    // Enforce the erasure ordering gate (reject consume-before-create), then mark the entry
    // consumed.
    assert_input_not_consumed_before_created(memory, idx);
    mark_input_note_consumed(memory, idx);
}

/// Verifies transaction `tx_index`'s `INPUT_NOTES_COMMITMENT_idx` against its piped
/// `(NULLIFIER, NOTE_ID_OR_EMPTY)` tuples, then binds each of those notes to the batch
/// input-note list (see [`bind_per_tx_notes`] for what binding proves).
#[inline(always)]
fn process_tx_input_notes(memory: &mut BatchMemory, tx_index: usize) {
    if memory::is_empty_word(memory.tx_input_notes_commitment(tx_index)) {
        // Empty commitment: the transaction has no input notes, so there is nothing to bind.
        return;
    }
    let commitment =
        memory::load_word(&memory.tx_headers, tx_index * crate::memory::TX_HEADER_FELT_LEN + 8);
    let notes = load_tx_note_tuples(commitment);
    for note in notes.chunks_exact(NOTE_ENTRY_FELT_LEN) {
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
#[inline(always)]
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
#[inline(always)]
pub fn track_notes(memory: &mut BatchMemory) {
    cross_reference_erasure(memory);
    bind_per_tx_notes(memory);
}
