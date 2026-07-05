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
        BatchMemory, InputNoteFlags, MAX_NOTES_PER_BATCH, MAX_TRANSACTIONS_PER_BATCH,
        NOTE_ENTRY_FELT_LEN, NoteEntry, OutputNoteFlags, TX_HEADER_FELT_LEN, TX_TUPLE_FELT_LEN,
        TxHeader, TxTuple, erasure_not_erased,
    },
    note_tracker,
};

// CONSTANTS
// =================================================================================================

/// Maximum value of `batch_expiration_block_num`, used as the running-min initialiser.
const MAX_BLOCK_NUM: u64 = 0xffffffff;

/// Advice-map key under which the nullifier-sorted input-note list is provided. The key is the
/// word hash of the domain message `miden::batch_kernel::input_note_list` (the MASM kernel
/// evaluates `word("miden::batch_kernel::input_note_list")` at assembly time; the value below is
/// `miden_core::utils::hash_string_to_word` of the same message).
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
/// returns the entries. The list is not hashed against a commitment; its integrity is established
/// later by binding every entry to a verified per-transaction note.
fn load_note_list(key: Word) -> Vec<NoteEntry> {
    let len_felts = adv_push_mapvaln(key).as_canonical_u64() as usize;
    // The MASM kernel derives `num_notes` with field divisions; a length that is not a multiple
    // of 8 felts wraps it to a non-u32 felt, which the range check below rejects.
    assert!(
        len_felts.is_multiple_of(NOTE_ENTRY_FELT_LEN),
        "a batch note list length is not a multiple of the note entry length"
    );
    let num_notes = len_felts / NOTE_ENTRY_FELT_LEN;
    assert!(
        num_notes <= MAX_NOTES_PER_BATCH,
        "a batch note list contains more entries than the maximum allowed"
    );

    let num_words = len_felts / 4;
    let (_hash, data) = pipe_words_to_memory(Felt::from(num_words as u32));

    let mut entries = Vec::with_capacity(num_notes);
    for chunk in data.chunks_exact(NOTE_ENTRY_FELT_LEN) {
        entries.push(NoteEntry {
            key: Word::from([chunk[0], chunk[1], chunk[2], chunk[3]]),
            value: Word::from([chunk[4], chunk[5], chunk[6], chunk[7]]),
        });
    }
    entries
}

/// Asserts a note list is strictly increasing by its KEY word, which also proves there are no
/// duplicate keys.
fn assert_list_strictly_sorted(entries: &[NoteEntry]) {
    for pair in entries.windows(2) {
        assert!(
            note_tracker::word_lt(&pair[0].key, &pair[1].key),
            "a batch note list is not strictly sorted by its key"
        );
    }
}

/// Loads and verifies the nullifier-sorted input-note list, along with its parallel flags
/// initialized to `[erasure = 0, consumption = 0]`.
fn prepare_input_note_list() -> (Vec<NoteEntry>, Vec<InputNoteFlags>) {
    let entries = load_note_list(input_note_list_key());
    assert_list_strictly_sorted(&entries);
    let mut flags = Vec::with_capacity(entries.len());
    for _ in 0..entries.len() {
        flags.push(InputNoteFlags {
            erasure: erasure_not_erased(),
            consumed: felt!(0),
        });
    }
    (entries, flags)
}

/// Loads and verifies the note-id-sorted output-note list, along with its parallel flags
/// initialized to `[will_be_erased = 0, is_created = 0, linked_input_index = 0]`.
fn prepare_output_note_list() -> (Vec<NoteEntry>, Vec<OutputNoteFlags>) {
    let entries = load_note_list(output_note_list_key());
    assert_list_strictly_sorted(&entries);
    let mut flags = Vec::with_capacity(entries.len());
    for _ in 0..entries.len() {
        flags.push(OutputNoteFlags {
            will_be_erased: felt!(0),
            is_created: felt!(0),
            linked_input_index: 0,
        });
    }
    (entries, flags)
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
fn load_tx_expirations(num_transactions: usize) -> Felt {
    let (_hash, data) = pipe_words_to_memory(Felt::from(num_transactions as u32));

    let mut min = Felt::new(MAX_BLOCK_NUM).unwrap();
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

    // Pipe the tuples into memory, asserting their sequential hash equals BATCH_ID.
    let tuple_data = adv_load_preimage(Felt::from((len_felts / 4) as u32), batch_id);

    let mut tx_tuples = Vec::with_capacity(num_transactions);
    for chunk in tuple_data.chunks_exact(TX_TUPLE_FELT_LEN) {
        tx_tuples.push(TxTuple {
            tx_id: Word::from([chunk[0], chunk[1], chunk[2], chunk[3]]),
            account_id: Word::from([chunk[4], chunk[5], chunk[6], chunk[7]]),
        });
    }

    // Layer 2: for each transaction, pipe + verify its header.
    // ---------------------------------------------------------------------------------------------

    let mut tx_headers = Vec::with_capacity(num_transactions);
    for tuple in tx_tuples.iter() {
        let len_felts = adv_push_mapvaln(tuple.tx_id).as_canonical_u64() as usize;
        assert!(
            len_felts.is_multiple_of(4),
            "a transaction header length is not a multiple of the word length"
        );

        // Pipe the header into memory, asserting its sequential hash equals TX_ID.
        let header_data = adv_load_preimage(Felt::from((len_felts / 4) as u32), tuple.tx_id);
        // The MASM kernel stores each header in a fixed 16-felt slot; the length is implicitly
        // pinned by the hash check against the `TransactionId::new` pre-image.
        assert!(
            header_data.len() == TX_HEADER_FELT_LEN,
            "a transaction header does not match the TransactionId pre-image layout"
        );

        tx_headers.push(TxHeader {
            init_account_commitment: Word::from([
                header_data[0],
                header_data[1],
                header_data[2],
                header_data[3],
            ]),
            final_account_commitment: Word::from([
                header_data[4],
                header_data[5],
                header_data[6],
                header_data[7],
            ]),
            input_notes_commitment: Word::from([
                header_data[8],
                header_data[9],
                header_data[10],
                header_data[11],
            ]),
            output_notes_commitment: Word::from([
                header_data[12],
                header_data[13],
                header_data[14],
                header_data[15],
            ]),
        });
    }

    // Note lists: load the sorted note lists and assert each is strictly sorted by its key.
    // ---------------------------------------------------------------------------------------------

    let (input_notes, input_note_flags) = prepare_input_note_list();
    let (output_notes, output_note_flags) = prepare_output_note_list();

    // Expirations: accumulate the running minimum of the transactions' expiration block numbers.
    // ---------------------------------------------------------------------------------------------

    let batch_expiration_block_num = load_tx_expirations(num_transactions);

    BatchMemory {
        tx_tuples,
        tx_headers,
        input_notes,
        input_note_flags,
        output_notes,
        output_note_flags,
        batch_expiration_block_num,
    }
}
