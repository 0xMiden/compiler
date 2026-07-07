//! Batch kernel prologue: loads and verifies the batch's structural commitments from the advice
//! provider.
//!
//! Mirrors `asm/kernels/batch/lib/prologue.masm` from the protocol batch kernel
//! (0xMiden/protocol#2905), extended with the per-transaction expiration running-minimum of
//! 0xMiden/protocol#3019.

extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{
    Felt, Word, adv_load_preimage, felt, intrinsics::advice::adv_push_mapvaln, pipe_words_to_memory,
};

use crate::{
    memory::{
        self, BatchMemory, INPUT_FLAGS_STRIDE, MAX_NOTES_PER_BATCH, MAX_TRANSACTIONS_PER_BATCH,
        NOTE_ENTRY_FELT_LEN, OUTPUT_FLAGS_STRIDE, TX_HEADER_FELT_LEN, TX_TUPLE_FELT_LEN,
    },
    note_tracker,
};

// CONSTANTS
// =================================================================================================

/// Advice-map key under which the nullifier-sorted input-note list is provided. The key is the
/// word hash of the domain message `miden::batch_kernel::input_note_list` (the MASM kernel
/// evaluates `word("miden::batch_kernel::input_note_list")` at assembly time; the value below is
/// `miden_core::utils::hash_string_to_word` of the same message).
#[inline(always)]
fn input_note_list_key() -> Word {
    Word::from([
        felt!(0x643bd4845322a3ce_u64),
        felt!(0x59fe43373dd36f9c_u64),
        felt!(0xb025720fddafbf03_u64),
        felt!(0x5737d0d4e9e438c8_u64),
    ])
}

/// Advice-map key under which the note-id-sorted output-note list is provided: the word hash of
/// the domain message `miden::batch_kernel::output_note_list`.
#[inline(always)]
fn output_note_list_key() -> Word {
    Word::from([
        felt!(0x3526094d3f4b4d20_u64),
        felt!(0x4156c0a181a827b7_u64),
        felt!(0xeca6acdd31b62cbf_u64),
        felt!(0xe0922efc58e94e7d_u64),
    ])
}

// SORTED NOTE LIST LOADING
// =================================================================================================

/// Pipes a sorted note list (8-felt `[KEY, VALUE]` entries) from the advice map into memory and
/// returns it as piped. The list is not hashed against a commitment; its integrity is
/// established later by binding every entry to a verified per-transaction note.
#[inline(always)]
fn load_note_list(key: Word) -> Vec<Felt> {
    let len_felts = adv_push_mapvaln(key).as_canonical_u64() as usize;
    // The MASM kernel derives `num_notes` with field divisions; a length that is not a multiple
    // of 8 felts wraps it to a non-u32 felt, which the range check below rejects.
    assert!(
        len_felts.is_multiple_of(NOTE_ENTRY_FELT_LEN),
        "a batch note list length is not a multiple of the note entry length"
    );
    assert!(
        len_felts / NOTE_ENTRY_FELT_LEN <= MAX_NOTES_PER_BATCH,
        "a batch note list contains more entries than the maximum allowed"
    );

    let (_hash, data) = pipe_words_to_memory(Felt::from((len_felts / 4) as u32));
    data
}

/// Asserts a flat note list is strictly increasing by its KEY word, which also proves there are
/// no duplicate keys.
#[inline(always)]
fn assert_list_strictly_sorted(list: &[Felt]) {
    for index in 1..list.len() / NOTE_ENTRY_FELT_LEN {
        assert!(
            note_tracker::word_lt(memory::note_key(list, index - 1), memory::note_key(list, index)),
            "a batch note list is not strictly sorted by its key"
        );
    }
}

/// Loads and verifies the nullifier-sorted input-note list.
#[inline(always)]
fn prepare_input_note_list() -> Vec<Felt> {
    let entries = load_note_list(input_note_list_key());
    assert_list_strictly_sorted(&entries);
    entries
}

/// Loads and verifies the note-id-sorted output-note list.
#[inline(always)]
fn prepare_output_note_list() -> Vec<Felt> {
    let entries = load_note_list(output_note_list_key());
    assert_list_strictly_sorted(&entries);
    entries
}

// TRANSACTION EXPIRATIONS
// =================================================================================================

/// Reads each transaction's `expiration_block_num` from the advice stack and returns the running
/// minimum over all transactions.
///
/// The MASM implementation of 0xMiden/protocol#3019 pops one felt per transaction; the compiler
/// SDK reads the advice stack with word granularity, so each transaction contributes one
/// `[expiration_block_num, 0, 0, 0]` word instead.
///
/// TODO: assert each `expiration_block_num_i > reference_block_num`.
/// TODO: derive each `expiration_block_num_i` from data committed-to in the verified transaction
///       header rather than from the unverified advice stack.
#[inline(always)]
fn load_tx_expirations(num_transactions: usize) -> Felt {
    let (_hash, data) = pipe_words_to_memory(Felt::from(num_transactions as u32));

    let mut min = felt!(0xffffffff_u64);
    for tx_index in 0..num_transactions {
        let expiration_block_num = data[tx_index * 4];
        if expiration_block_num < min {
            min = expiration_block_num;
        }
    }
    min
}

// PROLOGUE
// =================================================================================================

/// Loads to memory and verifies the batch's structural commitments from the advice provider.
///
/// Performs the following steps:
/// - Layer 1: pipes the `(tx_id, account_id)` tuples from the advice map keyed by `BATCH_ID`,
///   asserting that the sequential hash of the piped data matches `BATCH_ID`. The number of
///   transactions is derived from the piped length.
/// - Layer 2: for each transaction, pipes its pre-image from the advice map keyed by the verified
///   `tx_id`, asserting that the sequential hash of the piped data matches the `tx_id`.
/// - Note lists: loads the input-note list (sorted by nullifier) and the output-note list (sorted
///   by note id) from the advice map and asserts each is strictly sorted by its key. Their
///   integrity is established later, when the note tracker binds every entry to a verified
///   per-transaction note.
/// - Expirations: reads each transaction's `expiration_block_num` from the advice stack and
///   accumulates the running minimum.
///
/// Panics if:
/// - the `(tx_id, account_id)` tuple list piped from the advice map does not hash to `BATCH_ID`.
/// - a transaction's `(INIT, FINAL, INPUT_NOTES_COMMITMENT, OUTPUT_NOTES_COMMITMENT)` data piped
///   from the advice map does not hash to its `tx_id`.
///
/// TODO: verify that each transaction's reference block is contained in the chain MMR rooted at
///       BLOCK_COMMITMENT.
/// TODO: verify that the partial-blockchain peaks hash matches the block header's chain
///       commitment.
#[inline(always)]
pub fn prepare_batch(batch_id: Word) -> BatchMemory {
    // Layer 1: pipe BATCH_ID's mapped value + verify.
    // ---------------------------------------------------------------------------------------------

    let len_felts = adv_push_mapvaln(batch_id).as_canonical_u64() as usize;
    // Each tx contributes 2 words (8 felts): tx_id + account_id_pair.
    assert!(
        len_felts.is_multiple_of(TX_TUPLE_FELT_LEN),
        "the batch transaction tuple list length is not a multiple of the tuple length"
    );
    let num_transactions = len_felts / TX_TUPLE_FELT_LEN;
    assert!(
        num_transactions <= MAX_TRANSACTIONS_PER_BATCH,
        "the batch contains more transactions than the maximum allowed"
    );

    // Pipe the tuples into memory, asserting their sequential hash equals BATCH_ID; the piped
    // buffer is kept as-is as the tuple region.
    let tx_tuples = adv_load_preimage(Felt::from((len_felts / 4) as u32), batch_id);

    // Layer 2: for each transaction, pipe + verify its header into the flat header buffer.
    // ---------------------------------------------------------------------------------------------

    let mut tx_headers: Vec<Felt> = Vec::with_capacity(num_transactions * TX_HEADER_FELT_LEN);
    for tx_index in 0..num_transactions {
        let tx_id = memory::load_word(&tx_tuples, tx_index * TX_TUPLE_FELT_LEN);
        let len_felts = adv_push_mapvaln(tx_id).as_canonical_u64() as usize;
        // The MASM kernel stores each header in a fixed 16-felt slot; the length is implicitly
        // pinned by the hash check against the `TransactionId::new` pre-image.
        assert!(
            len_felts == TX_HEADER_FELT_LEN,
            "a transaction header does not match the TransactionId pre-image layout"
        );

        // Pipe the header into memory, asserting its sequential hash equals TX_ID.
        let header_data = adv_load_preimage(Felt::from((TX_HEADER_FELT_LEN / 4) as u32), tx_id);
        tx_headers.extend_from_slice(&header_data);
    }

    // Note lists: load the sorted note lists and assert each is strictly sorted by its key.
    // ---------------------------------------------------------------------------------------------

    let input_notes = prepare_input_note_list();
    let output_notes = prepare_output_note_list();
    // One shared buffer for both parallel flag arrays: the input-note flags
    // (`[erasure = 0, consumed = 0]` per entry) followed by the output-note flags
    // (`[will_be_erased = 0, is_created = 0, linked_input_index = 0]` per entry).
    let note_flags = alloc::vec![
        felt!(0);
        (input_notes.len() / NOTE_ENTRY_FELT_LEN) * INPUT_FLAGS_STRIDE
            + (output_notes.len() / NOTE_ENTRY_FELT_LEN) * OUTPUT_FLAGS_STRIDE
    ];

    // Expirations: accumulate the running minimum of the transactions' expiration block numbers.
    // ---------------------------------------------------------------------------------------------

    let batch_expiration_block_num = load_tx_expirations(num_transactions);

    BatchMemory {
        tx_tuples,
        tx_headers,
        input_notes,
        output_notes,
        note_flags,
        batch_expiration_block_num,
    }
}
