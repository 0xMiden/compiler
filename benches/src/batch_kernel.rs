//! Benchmark scenarios for the Rust batch kernel fixture (`tests/fixtures/batch-kernel`).
//!
//! The input builders are a copy of those in `tests/integration/src/end_to_end/batch_kernel.rs`,
//! which owns the kernel's correctness checks; the benchmark inputs need not stay equal to the
//! test's. The kernel hashes the advice-map entries back to the batch id and checks the sorted
//! note lists against them, so a builder that drifts from that layout fails the benchmark run.
//! The advice-stack expiration words are not authenticated (a fixture TODO) and only feed the
//! expiration minimum.
//!
//! The inputs are compiled into the runner, so they follow the runner's revision rather than
//! the `--workspace-root` checkout.

use miden_core::{EMPTY_WORD, Felt, Word, crypto::hash::Poseidon2, utils::hash_string_to_word};
use miden_debug::ExecutionConfig;
use miden_processor::{StackInputs, advice::AdviceInputs};

/// The fixture project, relative to the workspace root.
pub(super) const PROJECT_DIR: &str = "tests/fixtures/batch-kernel";

/// The benchmark name prefix shared by the kernel's scenario rows.
pub(super) const NAME: &str = "batch-kernel";

/// Compiler arguments the fixture needs on top of the runner's standard build flags.
pub(super) const COMPILE_ARGS: &[&str] = &["--entrypoint", "batch_kernel::entrypoint"];

/// The fixture's build profile for flamegraphs: `release` plus the package debug info, which the
/// fixture's `release` profile leaves out.
pub(super) const DEBUG_PROFILE: &str = "profiling";

/// Advice-map key message of the sorted input note list. Must match the key constants baked into
/// the fixture's `prologue.rs`.
const INPUT_NOTE_LIST_KEY_MESSAGE: &str = "miden::batch_kernel::input_note_list";

/// Advice-map key message of the sorted output note list. Must match the key constants baked
/// into the fixture's `prologue.rs`.
const OUTPUT_NOTE_LIST_KEY_MESSAGE: &str = "miden::batch_kernel::output_note_list";

/// Byte address (in Rust memory) at which the kernel writes its output words.
const OUT_ADDR: u32 = 20 * 65536;

/// A measured kernel execution.
pub(super) struct Scenario {
    /// The scenario's label, appended to [`NAME`] in the report.
    pub(super) label: &'static str,
    /// A short file-name-safe identifier of the scenario, unique among the scenarios.
    pub(super) slug: &'static str,
    /// The kernel's operand stack and advice inputs.
    pub(super) config: ExecutionConfig,
}

/// The report row name of `scenario`.
pub(super) fn row_name(scenario: &Scenario) -> String {
    format!("{NAME} ({})", scenario.label)
}

/// The file stem of the artifacts written for `scenario`.
pub(super) fn artifact_stem(scenario: &Scenario) -> String {
    format!("{NAME}-{}", scenario.slug)
}

/// The benchmarked scenarios, in report order.
pub(super) fn scenarios() -> Vec<Scenario> {
    let block_commitment = word(101, 102, 103, 104);
    [
        ("two transactions", "two-transactions", two_tx_batch()),
        ("eight transactions, 64 notes", "eight-transactions", many_notes_batch()),
    ]
    .into_iter()
    .map(|(label, slug, transactions)| Scenario {
        label,
        slug,
        config: ExecutionConfig {
            inputs: StackInputs::new(&build_args(block_commitment, batch_id(&transactions)))
                .expect("kernel arguments fit the operand stack"),
            advice_inputs: build_advice_inputs(&transactions),
            ..Default::default()
        },
    })
    .collect()
}

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
    /// Mirrors `build_input_note_commitment`.
    fn input_notes_commitment(&self) -> Word {
        sequential_hash(&self.input_notes)
    }

    /// Mirrors `OutputNotes::commitment`.
    fn output_notes_commitment(&self) -> Word {
        sequential_hash(&self.output_notes)
    }

    /// Mirrors `TransactionCommitments::elements`: the felt sequence hashed into the transaction id.
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

/// The sequential hash of word-pair tuples, or the empty word if there are none.
fn sequential_hash(tuples: &[(Word, Word)]) -> Word {
    if tuples.is_empty() {
        return EMPTY_WORD;
    }
    Poseidon2::hash_elements(&tuple_elements(tuples))
}

/// Flattens word-pair tuples into their felt sequence.
fn tuple_elements(tuples: &[(Word, Word)]) -> Vec<Felt> {
    tuples
        .iter()
        .flat_map(|(first, second)| first.as_elements().iter().chain(second.as_elements()))
        .copied()
        .collect()
}

/// Mirrors `NoteId`: the merge of the note's details and metadata commitments.
fn note_id(details_commitment: Word, metadata_commitment: Word) -> Word {
    Poseidon2::merge(&[details_commitment, metadata_commitment])
}

// SCENARIOS
// =================================================================================================

/// A small transaction with distinct account states and no notes.
fn transaction(seed: u64) -> MockTransaction {
    MockTransaction {
        account_id: [Felt::new_unchecked(seed), Felt::new_unchecked(seed + 1)],
        init_account_commitment: word(seed, 1, 1, 1),
        final_account_commitment: word(seed, 2, 2, 2),
        input_notes: vec![],
        output_notes: vec![],
        expiration_block_num: 1234,
    }
}

/// The protocol's two-transaction scenario, with authenticated and external unauthenticated
/// inputs and three outputs. Transaction order deliberately differs from nullifier order.
fn two_tx_batch() -> Vec<MockTransaction> {
    let mut first = transaction(10);
    first.input_notes = vec![(word(2000, 0, 0, 2), EMPTY_WORD)];
    first.output_notes = vec![(word(80, 0, 0, 80), word(81, 0, 0, 81))];
    let mut second = transaction(20);
    second.input_notes = vec![(word(1000, 0, 0, 1), word(90, 0, 0, 90))];
    second.output_notes = vec![
        (word(82, 0, 0, 82), word(83, 0, 0, 83)),
        (word(84, 0, 0, 84), word(85, 0, 0, 85)),
    ];
    second.expiration_block_num = 800;
    vec![first, second]
}

/// Eight transactions with four input and four output notes each.
fn many_notes_batch() -> Vec<MockTransaction> {
    (0..8)
        .map(|i| {
            let mut tx = transaction(i + 1);
            for j in 0..4 {
                let seed = i * 4 + j + 1;
                tx.input_notes.push((word(seed, 0, 0, 0), EMPTY_WORD));
                tx.output_notes.push((word(seed, 1, 2, 3), word(seed, 4, 5, 6)));
            }
            tx
        })
        .collect()
}

// KERNEL INPUT BUILDERS
// =================================================================================================

/// Mirrors `BatchId::from_ids`: the sequential hash of the `(tx_id, account_id)` tuples.
fn batch_id(transactions: &[MockTransaction]) -> Word {
    Poseidon2::hash_elements(&layer1_elements(transactions))
}

/// Mirrors `OrderedTransactionHeaders::hash_input_elements`: for each transaction,
/// `[transaction_id[4], account_id_suffix, account_id_prefix, 0, 0]`.
fn layer1_elements(transactions: &[MockTransaction]) -> Vec<Felt> {
    let mut elements = Vec::with_capacity(transactions.len() * 8);
    for tx in transactions {
        elements.extend_from_slice(tx.id().as_elements());
        elements.extend_from_slice(&[
            tx.account_id[1],
            tx.account_id[0],
            Felt::new_unchecked(0),
            Felt::new_unchecked(0),
        ]);
    }
    elements
}

/// Builds the advice inputs consumed by the batch kernel, mirroring
/// `BatchKernel::build_advice_inputs` plus one `[expiration_block_num, 0, 0, 0]` advice-stack
/// word per transaction.
fn build_advice_inputs(transactions: &[MockTransaction]) -> AdviceInputs {
    let mut map: Vec<(Word, Vec<Felt>)> =
        vec![(batch_id(transactions), layer1_elements(transactions))];
    let mut input_list: Vec<(Word, Word)> = Vec::new();
    let mut output_list: Vec<Word> = Vec::new();
    for tx in transactions {
        map.push((tx.id(), tx.header_elements()));
        if !tx.input_notes.is_empty() {
            map.push((tx.input_notes_commitment(), tuple_elements(&tx.input_notes)));
            input_list.extend_from_slice(&tx.input_notes);
        }
        if !tx.output_notes.is_empty() {
            map.push((tx.output_notes_commitment(), tuple_elements(&tx.output_notes)));
            output_list.extend(
                tx.output_notes.iter().map(|&(details, metadata)| note_id(details, metadata)),
            );
        }
    }

    // The global note lists: input notes keyed by nullifier, output notes by note id (with an
    // unused empty value word), both ascending.
    input_list.sort_by_key(|(nullifier, _)| *nullifier);
    output_list.sort_unstable();
    let output_list: Vec<(Word, Word)> =
        output_list.into_iter().map(|id| (id, EMPTY_WORD)).collect();
    map.push((hash_string_to_word(INPUT_NOTE_LIST_KEY_MESSAGE), tuple_elements(&input_list)));
    map.push((hash_string_to_word(OUTPUT_NOTE_LIST_KEY_MESSAGE), tuple_elements(&output_list)));

    let stack: Vec<Felt> = transactions
        .iter()
        .flat_map(|tx| {
            let mut word = [Felt::new_unchecked(0); 4];
            word[0] = Felt::from(tx.expiration_block_num);
            word
        })
        .collect();

    AdviceInputs::default().with_map(map).with_stack(stack.into())
}

/// The kernel entrypoint's operand stack arguments: `[out_ptr, BLOCK_COMMITMENT, BATCH_ID]`.
fn build_args(block_commitment: Word, batch_id: Word) -> Vec<Felt> {
    let mut args = vec![Felt::from(OUT_ADDR)];
    args.extend_from_slice(block_commitment.as_elements());
    args.extend_from_slice(batch_id.as_elements());
    args
}
