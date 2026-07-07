//! Smoke tests for the Rust implementation of the Miden protocol batch kernel
//! (`tests/fixtures/batch-kernel`).
//!
//! The fixture mirrors the MASM batch kernel of the protocol repository
//! (`asm/kernels/batch` of 0xMiden/protocol#2905, plus the expiration running-minimum of
//! 0xMiden/protocol#3019); this test plays the role of the protocol's `BatchKernel` input
//! builders: it derives batch data from mock transactions the same way
//! `BatchKernel::prepare_inputs` does, executes the compiled kernel on the VM, and checks the
//! outputs against commitments computed with the host hasher.

use std::{
    collections::BTreeSet,
    panic::{AssertUnwindSafe, catch_unwind},
};

use miden_core::{EMPTY_WORD, Felt, Word, crypto::hash::Poseidon2, utils::hash_string_to_word};
use miden_debug::{DebugQuery, Felt as TestFelt};
use miden_processor::advice::AdviceInputs;
use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{executor_with_std, stripped_mast_size_str},
};

/// Advice-map keys under which the sorted note lists are provided to the kernel. Must match the
/// key constants baked into the fixture's `prologue.rs`.
const INPUT_NOTE_LIST_KEY_MESSAGE: &str = "miden::batch_kernel::input_note_list";
const OUTPUT_NOTE_LIST_KEY_MESSAGE: &str = "miden::batch_kernel::output_note_list";

/// Byte address (in Rust memory) at which the kernel writes its output words.
const OUT_ADDR: u32 = 20 * 65536;

/// Shorthand for a word of the given canonical felt values.
fn word(a: u64, b: u64, c: u64, d: u64) -> Word {
    Word::from([
        Felt::new_unchecked(a),
        Felt::new_unchecked(b),
        Felt::new_unchecked(c),
        Felt::new_unchecked(d),
    ])
}

// MOCK TRANSACTIONS
// =================================================================================================

/// The witness data of one proven transaction, as far as the batch kernel is concerned.
struct MockTransaction {
    /// The account the transaction executes against, as `[prefix, suffix]`.
    account_id: [Felt; 2],
    /// The account's state commitment before the transaction.
    init_account_commitment: Word,
    /// The account's state commitment after the transaction.
    final_account_commitment: Word,
    /// The consumed notes' `(nullifier, note_id_or_empty)` tuples: the note id is set for notes
    /// consumed unauthenticated and empty for authenticated ones.
    input_notes: Vec<(Word, Word)>,
    /// The produced notes' `(details_commitment, metadata_commitment)` tuples.
    output_notes: Vec<(Word, Word)>,
    /// The block number at which the transaction expires.
    expiration_block_num: u32,
}

impl MockTransaction {
    /// Mirrors `build_input_note_commitment`: the sequential hash of the
    /// `(nullifier, note_id_or_empty)` tuples, or the empty word if there are none.
    fn input_notes_commitment(&self) -> Word {
        if self.input_notes.is_empty() {
            return EMPTY_WORD;
        }
        let elements: Vec<Felt> = self
            .input_notes
            .iter()
            .flat_map(|(nullifier, note_id)| {
                nullifier.as_elements().iter().chain(note_id.as_elements()).copied()
            })
            .collect();
        Poseidon2::hash_elements(&elements)
    }

    /// Mirrors `OutputNotes::commitment`: the sequential hash of the
    /// `(details_commitment, metadata_commitment)` tuples, or the empty word if there are none.
    fn output_notes_commitment(&self) -> Word {
        if self.output_notes.is_empty() {
            return EMPTY_WORD;
        }
        let elements: Vec<Felt> = self
            .output_notes
            .iter()
            .flat_map(|(details, metadata)| {
                details.as_elements().iter().chain(metadata.as_elements()).copied()
            })
            .collect();
        Poseidon2::hash_elements(&elements)
    }

    /// Mirrors `TransactionId::input_elements`: the felt sequence hashed into the transaction id.
    fn header_elements(&self) -> Vec<Felt> {
        let mut elements = Vec::with_capacity(16);
        elements.extend_from_slice(self.init_account_commitment.as_elements());
        elements.extend_from_slice(self.final_account_commitment.as_elements());
        elements.extend_from_slice(self.input_notes_commitment().as_elements());
        elements.extend_from_slice(self.output_notes_commitment().as_elements());
        elements
    }

    /// Mirrors `TransactionId::new`.
    fn id(&self) -> Word {
        Poseidon2::hash_elements(&self.header_elements())
    }
}

/// Mirrors `NoteId`: the merge of the note's details and metadata commitments.
fn note_id(details_commitment: Word, metadata_commitment: Word) -> Word {
    Poseidon2::merge(&[details_commitment, metadata_commitment])
}

// KERNEL INPUT BUILDERS
// =================================================================================================

/// Mirrors `BatchId::from_ids`: the sequential hash of the `(tx_id, account_id)` tuples.
fn batch_id(transactions: &[MockTransaction]) -> Word {
    Poseidon2::hash_elements(&layer1_elements(transactions))
}

/// Mirrors `BatchId::hash_input_elements`: for each transaction,
/// `[transaction_id[4], account_id_prefix, account_id_suffix, 0, 0]`.
fn layer1_elements(transactions: &[MockTransaction]) -> Vec<Felt> {
    let mut elements = Vec::with_capacity(transactions.len() * 8);
    for tx in transactions {
        elements.extend_from_slice(tx.id().as_elements());
        elements.extend_from_slice(&[
            tx.account_id[0],
            tx.account_id[1],
            Felt::new_unchecked(0),
            Felt::new_unchecked(0),
        ]);
    }
    elements
}

/// Builds the advice inputs consumed by the batch kernel, mirroring
/// `BatchKernel::build_advice_inputs` (plus the expiration advice stack of
/// 0xMiden/protocol#3019, adapted to one `[expiration_block_num, 0, 0, 0]` word per transaction).
fn build_advice_inputs(transactions: &[MockTransaction]) -> AdviceInputs {
    let mut map: Vec<(Word, Vec<Felt>)> = Vec::new();

    // Layer 1: BATCH_ID -> [(tx_id, account_id) tuples].
    map.push((batch_id(transactions), layer1_elements(transactions)));

    // Pre-erasure union of every transaction's notes, sorted below: input notes by nullifier,
    // output notes by note id.
    let mut input_list: Vec<(Word, Word)> = Vec::new();
    let mut output_list: Vec<Word> = Vec::new();

    for tx in transactions {
        // Layer 2: tx_id -> the felt sequence TransactionId::new hashes.
        map.push((tx.id(), tx.header_elements()));

        // Layer 3a: per-tx INPUT_NOTES_COMMITMENT -> [NULLIFIER, NOTE_ID_OR_EMPTY] tuples.
        if !tx.input_notes.is_empty() {
            let mut preimage = Vec::with_capacity(tx.input_notes.len() * 8);
            for (nullifier, note_id_or_empty) in &tx.input_notes {
                preimage.extend_from_slice(nullifier.as_elements());
                preimage.extend_from_slice(note_id_or_empty.as_elements());
                input_list.push((*nullifier, *note_id_or_empty));
            }
            map.push((tx.input_notes_commitment(), preimage));
        }

        // Layer 3b: per-tx OUTPUT_NOTES_COMMITMENT -> [DETAILS_COMMITMENT, METADATA_COMMITMENT]
        // tuples.
        if !tx.output_notes.is_empty() {
            let mut preimage = Vec::with_capacity(tx.output_notes.len() * 8);
            for (details, metadata) in &tx.output_notes {
                preimage.extend_from_slice(details.as_elements());
                preimage.extend_from_slice(metadata.as_elements());
                output_list.push(note_id(*details, *metadata));
            }
            map.push((tx.output_notes_commitment(), preimage));
        }
    }

    // Sort the input-note list by nullifier and the output-note list by note id, ascending.
    input_list.sort_by_key(|a| a.0);
    output_list.sort_unstable();

    // INPUT_NOTE_LIST_KEY -> [NULLIFIER, NOTE_ID_OR_EMPTY] (8 felts per note).
    let mut input_blob = Vec::with_capacity(input_list.len() * 8);
    for (nullifier, note_id_or_empty) in &input_list {
        input_blob.extend_from_slice(nullifier.as_elements());
        input_blob.extend_from_slice(note_id_or_empty.as_elements());
    }
    map.push((hash_string_to_word(INPUT_NOTE_LIST_KEY_MESSAGE), input_blob));

    // OUTPUT_NOTE_LIST_KEY -> [NOTE_ID, 0, 0, 0, 0] (8 felts per note; the VALUE word is unused).
    let mut output_blob = Vec::with_capacity(output_list.len() * 8);
    for id in &output_list {
        output_blob.extend_from_slice(id.as_elements());
        output_blob.extend_from_slice(EMPTY_WORD.as_elements());
    }
    map.push((hash_string_to_word(OUTPUT_NOTE_LIST_KEY_MESSAGE), output_blob));

    // Advice stack: each transaction's expiration_block_num, in transaction order, one word per
    // transaction.
    let mut stack = Vec::with_capacity(transactions.len() * 4);
    for tx in transactions {
        stack.push(Felt::new_unchecked(tx.expiration_block_num as u64));
        stack.extend_from_slice(&[Felt::new_unchecked(0); 3]);
    }

    AdviceInputs::default().with_map(map).with_stack(stack)
}

/// Returns the operand stack arguments for the kernel entrypoint:
/// `[out_ptr, BLOCK_COMMITMENT, BATCH_ID]`.
fn build_args(block_commitment: Word, batch_id: Word) -> Vec<Felt> {
    let mut args = vec![Felt::new_unchecked(OUT_ADDR as u64)];
    args.extend_from_slice(block_commitment.as_elements());
    args.extend_from_slice(batch_id.as_elements());
    args
}

// EXPECTED OUTPUTS
// =================================================================================================

/// Computes the expected `INPUT_NOTES_COMMITMENT` the way `ProposedBatch` does: the sequential
/// hash over the nullifier-sorted, post-erasure `(nullifier, note_id_or_empty)` tuples, where a
/// note is erased when it is consumed unauthenticated and its note id is created by a
/// transaction of the same batch.
fn expected_input_notes_commitment(transactions: &[MockTransaction]) -> Word {
    let created: BTreeSet<Word> = transactions
        .iter()
        .flat_map(|tx| tx.output_notes.iter())
        .map(|(details, metadata)| note_id(*details, *metadata))
        .collect();

    let mut entries: Vec<(Word, Word)> =
        transactions.iter().flat_map(|tx| tx.input_notes.iter().copied()).collect();
    entries.sort_by_key(|a| a.0);
    entries.retain(|(_, note_id)| *note_id == EMPTY_WORD || !created.contains(note_id));

    if entries.is_empty() {
        return EMPTY_WORD;
    }
    let elements: Vec<Felt> = entries
        .iter()
        .flat_map(|(nullifier, note_id)| {
            nullifier.as_elements().iter().chain(note_id.as_elements()).copied()
        })
        .collect();
    Poseidon2::hash_elements(&elements)
}

// SMOKE TESTS
// =================================================================================================

/// Compiles the batch kernel fixture and executes it against mock batches:
///
/// 1. a two-transaction batch without intra-batch notes, checking `INPUT_NOTES_COMMITMENT`,
///    `BATCH_NOTE_TREE_ROOT` and `batch_expiration_block_num`;
/// 2. a batch where one transaction creates a note a later transaction consumes, checking the
///    note is erased from `INPUT_NOTES_COMMITMENT`;
/// 3. a batch with a tampered `BATCH_ID` pre-image, checking the kernel rejects it;
/// 4. a batch where a note is consumed before the transaction that creates it, checking the
///    kernel rejects it.
#[test]
fn batch_kernel() {
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/batch-kernel",
        WasmTranslationConfig::default(),
        ["--entrypoint".to_string(), "batch_kernel::entrypoint".to_string()],
    );
    let package = test.compile_package();
    let program = package.unwrap_program();

    // The serialized size of the compiled kernel's MAST forest, with debug info stripped.
    expect!["160697"].assert_eq(stripped_mast_size_str(&package));

    // The reference block commitment is dropped by the kernel (verification is still a TODO
    // there), so any word will do.
    let block_commitment = word(101, 102, 103, 104);

    // Executes the kernel and returns the resulting trace along with the consumed VM cycles;
    // this is `Executor::execute` unrolled through the debug executor to observe the cycle
    // counter.
    let execute = |transactions: &[MockTransaction], advice: AdviceInputs| {
        let mut exec =
            executor_with_std(build_args(block_commitment, batch_id(transactions)), Some(&package));
        exec.with_advice_inputs(advice);
        let mut executor = exec.into_debug(&program, test.session.source_manager.clone());
        while !executor.stopped {
            if let Err(err) = executor.step() {
                panic!("batch kernel execution failed: {err}");
            }
        }
        let cycles = executor.cycle;
        (executor.into_execution_trace(), cycles)
    };

    // Scenario 1: two transactions, no intra-batch note relationships.
    // - tx1 consumes one authenticated note (empty note id) and creates one note.
    // - tx2 consumes one unauthenticated note whose note id is not created in this batch, and
    //   creates two notes.
    {
        let transactions = [
            MockTransaction {
                account_id: [Felt::new_unchecked(10), Felt::new_unchecked(11)],
                init_account_commitment: word(1, 1, 1, 1),
                final_account_commitment: word(2, 2, 2, 2),
                input_notes: vec![(word(1000, 0, 0, 1), EMPTY_WORD)],
                output_notes: vec![(word(80, 0, 0, 80), word(81, 0, 0, 81))],
                expiration_block_num: 1234,
            },
            MockTransaction {
                account_id: [Felt::new_unchecked(20), Felt::new_unchecked(21)],
                init_account_commitment: word(3, 3, 3, 3),
                final_account_commitment: word(4, 4, 4, 4),
                input_notes: vec![(word(2000, 0, 0, 2), word(90, 0, 0, 90))],
                output_notes: vec![
                    (word(82, 0, 0, 82), word(83, 0, 0, 83)),
                    (word(84, 0, 0, 84), word(85, 0, 0, 85)),
                ],
                expiration_block_num: 800,
            },
        ];

        let (trace, cycles) = execute(&transactions, build_advice_inputs(&transactions));

        // The VM cycles consumed by the kernel for this two-transaction batch.
        expect!["86894"].assert_eq(&cycles.to_string());

        let input_notes_commitment = read_word(&trace, OUT_ADDR);
        assert_eq!(
            input_notes_commitment,
            expected_input_notes_commitment(&transactions),
            "kernel INPUT_NOTES_COMMITMENT should match the commitment derived on the host"
        );

        let batch_note_tree_root = read_word(&trace, OUT_ADDR + 16);
        assert_eq!(
            batch_note_tree_root, EMPTY_WORD,
            "BATCH_NOTE_TREE_ROOT is not wired up yet and should be the empty word"
        );

        let expiration: miden_core::Felt =
            trace.parse_result().expect("kernel should return batch_expiration_block_num");
        assert_eq!(
            expiration,
            Felt::new_unchecked(800),
            "batch_expiration_block_num should be the minimum over the transactions"
        );
    }

    // Scenario 2: tx1 creates a note that tx2 consumes unauthenticated; the note is erased and
    // only tx1's authenticated input note remains in the commitment.
    {
        let details = word(50, 0, 0, 50);
        let metadata = word(51, 0, 0, 51);
        let transactions = [
            MockTransaction {
                account_id: [Felt::new_unchecked(10), Felt::new_unchecked(11)],
                init_account_commitment: word(1, 1, 1, 1),
                final_account_commitment: word(2, 2, 2, 2),
                input_notes: vec![(word(1000, 0, 0, 1), EMPTY_WORD)],
                output_notes: vec![(details, metadata)],
                expiration_block_num: 900,
            },
            MockTransaction {
                account_id: [Felt::new_unchecked(20), Felt::new_unchecked(21)],
                init_account_commitment: word(3, 3, 3, 3),
                final_account_commitment: word(4, 4, 4, 4),
                input_notes: vec![(word(2000, 0, 0, 2), note_id(details, metadata))],
                output_notes: vec![],
                expiration_block_num: 1000,
            },
        ];

        let (trace, _) = execute(&transactions, build_advice_inputs(&transactions));

        let expected = expected_input_notes_commitment(&transactions);
        assert_ne!(expected, EMPTY_WORD, "the authenticated note should remain post-erasure");
        assert_eq!(
            read_word(&trace, OUT_ADDR),
            expected,
            "the erased note should be excluded from INPUT_NOTES_COMMITMENT"
        );

        let expiration: miden_core::Felt =
            trace.parse_result().expect("kernel should return batch_expiration_block_num");
        assert_eq!(expiration, Felt::new_unchecked(900));
    }

    // Scenario 3: a tampered BATCH_ID pre-image must be rejected by the Layer 1 hash check.
    {
        let transactions = [MockTransaction {
            account_id: [Felt::new_unchecked(10), Felt::new_unchecked(11)],
            init_account_commitment: word(1, 1, 1, 1),
            final_account_commitment: word(2, 2, 2, 2),
            input_notes: vec![(word(1000, 0, 0, 1), EMPTY_WORD)],
            output_notes: vec![],
            expiration_block_num: 900,
        }];

        let mut advice = build_advice_inputs(&transactions);
        let key = batch_id(&transactions);
        let mut tampered: Vec<Felt> = advice.map.get(&key).expect("layer 1 advice entry").to_vec();
        tampered[0] += Felt::new_unchecked(1);
        advice.map.insert(key, tampered);

        let result = catch_unwind(AssertUnwindSafe(|| {
            execute(&transactions, advice);
        }));
        assert!(result.is_err(), "kernel should reject a tampered BATCH_ID pre-image");
    }

    // Scenario 4: tx1 consumes a note that only tx2 creates; the consume-before-create ordering
    // gate must reject the batch.
    {
        let details = word(50, 0, 0, 50);
        let metadata = word(51, 0, 0, 51);
        let transactions = [
            MockTransaction {
                account_id: [Felt::new_unchecked(10), Felt::new_unchecked(11)],
                init_account_commitment: word(1, 1, 1, 1),
                final_account_commitment: word(2, 2, 2, 2),
                input_notes: vec![(word(2000, 0, 0, 2), note_id(details, metadata))],
                output_notes: vec![],
                expiration_block_num: 900,
            },
            MockTransaction {
                account_id: [Felt::new_unchecked(20), Felt::new_unchecked(21)],
                init_account_commitment: word(3, 3, 3, 3),
                final_account_commitment: word(4, 4, 4, 4),
                input_notes: vec![],
                output_notes: vec![(details, metadata)],
                expiration_block_num: 1000,
            },
        ];

        let advice = build_advice_inputs(&transactions);
        let result = catch_unwind(AssertUnwindSafe(|| {
            execute(&transactions, advice);
        }));
        assert!(result.is_err(), "kernel should reject a note consumed before it is created");
    }
}

/// Reads a word the kernel wrote to Rust memory at `byte_addr`.
fn read_word(trace: &miden_debug::ExecutionTrace, byte_addr: u32) -> Word {
    let felts: [TestFelt; 4] = trace
        .read_from_rust_memory(byte_addr)
        .unwrap_or_else(|| panic!("failed to read output word at {byte_addr:#x}"));
    Word::from([felts[0].0, felts[1].0, felts[2].0, felts[3].0])
}
