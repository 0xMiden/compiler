//! Correctness and compiler-cost regression tests for the Rust Miden protocol batch kernel
//! (`tests/fixtures/batch-kernel`).
//!
//! The fixture mirrors the MASM batch kernel of the protocol repository
//! (`asm/kernels/batch` of 0xMiden/protocol#2905, plus the expiration running-minimum of
//! 0xMiden/protocol#3019); this test plays the role of the protocol's `BatchKernel` input
//! builders: it derives batch data from mock transactions the same way
//! `BatchKernel::prepare_inputs` does, executes the compiled kernel on the VM, and checks the
//! outputs against commitments computed with the host hasher.

use std::{collections::BTreeSet, sync::Arc};

use miden_assembly::SourceManager;
use miden_core::{EMPTY_WORD, Felt, Word, crypto::hash::Poseidon2, utils::hash_string_to_word};
use miden_debug::{DebugQuery, Felt as TestFelt};
use miden_mast_package::Package;
use miden_processor::advice::AdviceInputs;
use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest, CompilerTestBuilder,
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

    AdviceInputs::default().with_map(map).with_stack(stack.into())
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

// DIFFERENTIAL SCENARIOS
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

/// A creator followed by a consumer, optionally retaining an authenticated input after erasure.
fn erasure_batch(retain_input: bool) -> Vec<MockTransaction> {
    let mut first = transaction(10);
    let details = word(50, 0, 0, 50);
    let metadata = word(51, 0, 0, 51);
    first.output_notes = vec![(details, metadata)];
    first.expiration_block_num = 900;
    if retain_input {
        first.input_notes = vec![(word(1000, 0, 0, 1), EMPTY_WORD)];
    }
    let mut second = transaction(20);
    second.input_notes = vec![(word(2000, 0, 0, 2), note_id(details, metadata))];
    vec![first, second]
}

/// A larger batch exercising repeated per-transaction loading and deeper sorted-list lookups.
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

/// The valid scenarios as `(name, transactions)`, richest last.
fn valid_scenarios() -> Vec<(&'static str, Vec<MockTransaction>)> {
    vec![
        ("two transactions", two_tx_batch()),
        ("partial erasure", erasure_batch(true)),
        ("all inputs erased", erasure_batch(false)),
        ("no notes", vec![transaction(1)]),
        ("eight transactions, 64 notes", many_notes_batch()),
    ]
}

/// Serializes a strictly key-sorted global note list after inserting a phantom entry.
fn with_extra_note(blob: &[Felt], key: Word) -> Vec<Felt> {
    let mut entries: Vec<Vec<Felt>> =
        blob.as_chunks::<8>().0.iter().map(|entry| entry.to_vec()).collect();
    let mut entry = key.as_elements().to_vec();
    entry.extend_from_slice(EMPTY_WORD.as_elements());
    entries.push(entry);
    entries.sort_by_key(|entry| Word::from(<[Felt; 4]>::try_from(&entry[..4]).unwrap()));
    entries.into_iter().flatten().collect()
}

/// VM trace and cycles on success, or the VM rejection and its cycle on failure.
type ExecutionOutcome = Result<(miden_debug::ExecutionTrace, usize), (String, usize)>;

/// Executes the compiled kernel with a bounded VM cycle budget.
fn execute(
    package: Arc<Package>,
    source_manager: Arc<dyn SourceManager>,
    args: Vec<Felt>,
    advice: AdviceInputs,
) -> ExecutionOutcome {
    let mut exec = executor_with_std(args);
    exec.with_advice_inputs(advice);
    let mut executor = exec.into_debug(package, source_manager);
    while !executor.stopped {
        assert!(executor.cycle < 5_000_000, "batch kernel exceeded the cycle budget");
        if let Err(err) = executor.step() {
            return Err((err.to_string(), executor.cycle));
        }
    }
    let cycles = executor.cycle;
    Ok((executor.into_execution_trace(), cycles))
}

/// Checks the protocol's kernel success/rejection scenarios and snapshots compiler costs.
///
/// Scenarios follow protocol PR #2905 at `dfb246fb2e42284280891776d44095639031987c`.
/// Host-only executor and prover tests are outside this compiler fixture's scope. Extra cases
/// cover empty batches, malformed tuple lengths, complete erasure and larger sorted lists.
/// Cycles include the entrypoint/ABI and the expiration minimum extension from #3019.
#[test]
fn batch_kernel() {
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/batch-kernel",
        WasmTranslationConfig::default(),
        ["--entrypoint".to_string(), "batch_kernel::entrypoint".to_string()],
    );
    let package = test.compile_package();
    let source_manager = test.session.source_manager.clone();
    let block_commitment = word(101, 102, 103, 104);
    let mut metrics = format!("MAST bytes: {}\nVM cycles:\n", stripped_mast_size_str(&package));
    let mut check = |name: &str,
                     transactions: &[MockTransaction],
                     advice: AdviceInputs,
                     valid: bool| {
        let id = batch_id(transactions);
        let rust = execute(
            package.clone(),
            source_manager.clone(),
            build_args(block_commitment, id),
            advice,
        );
        let cycles = if valid {
            let (rust, rust_cycles) =
                rust.unwrap_or_else(|err| panic!("{name}: Rust rejected: {err:?}"));
            let expected = expected_input_notes_commitment(transactions);
            assert_eq!(read_word(&rust, OUT_ADDR), expected, "{name}: Rust input commitment");
            assert_eq!(read_word(&rust, OUT_ADDR + 16), EMPTY_WORD, "{name}: Rust output root");
            let expiration: Felt = rust.parse_result().expect("expiration output");
            assert_eq!(
                expiration,
                Felt::from(transactions.iter().map(|tx| tx.expiration_block_num).min().unwrap()),
                "{name}: expiration"
            );
            rust_cycles
        } else {
            let (rust_error, rust_cycles) =
                rust.err().unwrap_or_else(|| panic!("{name}: Rust accepted invalid advice"));
            // Missing advice, stack underflow, and host errors must not masquerade as validation.
            assert!(
                rust_error.contains("assertion"),
                "{name}: unexpected Rust error: {rust_error}"
            );
            rust_cycles
        };
        use std::fmt::Write;
        writeln!(metrics, "{name}: {cycles}").unwrap();
    };

    for (name, transactions) in valid_scenarios() {
        check(name, &transactions, build_advice_inputs(&transactions), true);
    }

    let transactions = two_tx_batch();
    let advice = build_advice_inputs(&transactions);
    let tx = &transactions[0];
    let batch_key = batch_id(&transactions);
    let input_key = hash_string_to_word(INPUT_NOTE_LIST_KEY_MESSAGE);
    let output_key = hash_string_to_word(OUTPUT_NOTE_LIST_KEY_MESSAGE);
    for (name, key) in [
        ("tampered batch", batch_key),
        ("tampered header", tx.id()),
        ("tampered input preimage", tx.input_notes_commitment()),
        ("tampered output preimage", tx.output_notes_commitment()),
    ] {
        let mut blob = advice.map().get(&key).unwrap().to_vec();
        blob[0] += Felt::ONE;
        check(name, &transactions, advice.clone().with_map([(key, blob)]), false);
    }
    for (name, key, blob) in [
        ("empty batch", batch_key, vec![]),
        ("too many transactions", batch_key, vec![Felt::ZERO; 1025 * 8]),
        ("unaligned transactions", batch_key, vec![Felt::ZERO; 9]),
        ("short header", tx.id(), vec![Felt::ZERO; 8]),
        ("long header", tx.id(), vec![Felt::ZERO; 32]),
        ("too many input notes", tx.input_notes_commitment(), vec![Felt::ZERO; 1025 * 8]),
        (
            "too many output notes",
            tx.output_notes_commitment(),
            vec![Felt::ZERO; 1025 * 8],
        ),
        ("unaligned input preimage", tx.input_notes_commitment(), vec![Felt::ZERO; 9]),
        ("unaligned output preimage", tx.output_notes_commitment(), vec![Felt::ZERO; 9]),
    ] {
        check(name, &transactions, advice.clone().with_map([(key, blob)]), false);
    }
    for (kind, key) in [("input", input_key), ("output", output_key)] {
        let blob = advice.map().get(&key).unwrap().to_vec();
        let mut duplicated = blob[..8].to_vec();
        duplicated.extend_from_slice(&blob);
        let mut descending = blob[8..16].to_vec();
        descending.extend_from_slice(&blob[..8]);
        descending.extend_from_slice(&blob[16..]);
        for (name, data) in [
            ("missing", blob[..blob.len() - 8].to_vec()),
            ("duplicated", duplicated),
            ("descending", descending),
            ("phantom", with_extra_note(&blob, word(u32::MAX as u64, 0, 0, 0))),
            ("oversized", vec![Felt::ZERO; 1025 * 8]),
            ("unaligned", vec![Felt::ZERO; 9]),
        ] {
            check(
                &format!("{name} {kind} list"),
                &transactions,
                advice.clone().with_map([(key, data)]),
                false,
            );
        }
    }
    let mut mismatch = advice.map().get(&input_key).unwrap().to_vec();
    mismatch[4] += Felt::ONE;
    check(
        "input note id mismatch",
        &transactions,
        advice.clone().with_map([(input_key, mismatch)]),
        false,
    );
    let phantom_creation =
        with_extra_note(advice.map().get(&output_key).unwrap(), transactions[1].input_notes[0].1);
    check(
        "phantom erasure",
        &transactions,
        advice.with_map([(output_key, phantom_creation)]),
        false,
    );

    let mut reversed = erasure_batch(false);
    reversed.reverse();
    check("consume before create", &reversed, build_advice_inputs(&reversed), false);
    let mut same_tx = erasure_batch(false);
    same_tx[0].input_notes = core::mem::take(&mut same_tx[1].input_notes);
    same_tx.truncate(1);
    check("same transaction erasure", &same_tx, build_advice_inputs(&same_tx), false);

    expect![[r#"
        MAST bytes: 104572
        VM cycles:
        two transactions: 28544
        partial erasure: 25062
        all inputs erased: 20071
        no notes: 11541
        eight transactions, 64 notes: 251204
        tampered batch: 1089
        tampered header: 2321
        tampered input preimage: 14066
        tampered output preimage: 16030
        empty batch: 343
        too many transactions: 326
        unaligned transactions: 302
        short header: 1911
        long header: 1911
        too many input notes: 13678
        too many output notes: 15634
        unaligned input preimage: 13654
        unaligned output preimage: 15623
        missing input list: 14107
        duplicated input list: 5963
        descending input list: 5785
        phantom input list: 25993
        oversized input list: 3871
        unaligned input list: 3847
        missing output list: 23348
        duplicated output list: 8034
        descending output list: 7856
        phantom output list: 25483
        oversized output list: 5930
        unaligned output list: 5906
        input note id mismatch: 19559
        phantom erasure: 21113
        consume before create: 13643
        same transaction erasure: 12484
    "#]]
    .assert_eq(&metrics);
}

/// Reads a word the kernel wrote to Rust memory at `byte_addr`.
fn read_word(trace: &miden_debug::ExecutionTrace, byte_addr: u32) -> Word {
    let felts: [TestFelt; 4] = trace
        .read_from_rust_memory(byte_addr)
        .unwrap_or_else(|| panic!("failed to read output word at {byte_addr:#x}"));
    Word::from([felts[0].0, felts[1].0, felts[2].0, felts[3].0])
}

/// Cycle profiler for one valid scenario (default: the richest). A tool for compiler optimization
/// work, not a regression test; run it with
/// `cargo make test-rust -- --run-ignored only --no-capture batch_kernel_profile`. Select the
/// scenario with `BATCH_KERNEL_PROFILE_SCENARIO=<name>`; set
/// `CARGO_PROFILE_PROFILING_DEBUG=line-tables-only` to resolve Rust source lines (this perturbs
/// the generated code slightly).
///
/// Builds with the fixture's `profiling` profile (release code plus the package debug-info
/// section, which the `release` profile omits) and writes to `target/batch_kernel_profile/`:
/// - `folded.txt`: flamegraph folded stacks (`outer;inner <cycles>`),
/// - `procedures.txt`: self cycles per innermost procedure,
/// - `locations.txt`: self cycles per innermost resolved source location,
/// - `ops.txt`: the executed VM operations in order, one `<op>\t<location>` per line, for
///   pattern counting.
#[test]
#[ignore = "profiling tool, run on demand"]
fn batch_kernel_profile() {
    use std::{collections::BTreeMap, fmt::Write as _};

    let mut scenarios = valid_scenarios();
    let (name, transactions) = match std::env::var("BATCH_KERNEL_PROFILE_SCENARIO") {
        Ok(wanted) => scenarios
            .into_iter()
            .find(|(name, _)| *name == wanted)
            .unwrap_or_else(|| panic!("unknown scenario {wanted:?}")),
        Err(_) => scenarios.pop().unwrap(),
    };

    // `--profile` and the harness's default `--release` exclude each other.
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/batch-kernel",
        WasmTranslationConfig::default(),
        ["--entrypoint", "batch_kernel::entrypoint", "--profile", "profiling"].map(String::from),
    );
    builder.with_release(false);
    let mut test = builder.build();
    let package = test.compile_package();
    let source_manager = test.session.source_manager.clone();
    let mut exec = executor_with_std(build_args(word(101, 102, 103, 104), batch_id(&transactions)));
    exec.with_advice_inputs(build_advice_inputs(&transactions));
    let mut executor = exec.into_debug(package, source_manager.clone());

    let mut folded: BTreeMap<String, usize> = BTreeMap::new();
    let mut procedures: BTreeMap<String, usize> = BTreeMap::new();
    let mut locations: BTreeMap<String, usize> = BTreeMap::new();
    let mut ops = String::new();
    let mut total = 0;
    while !executor.stopped {
        let before = executor.cycle;
        executor.step().unwrap_or_else(|err| panic!("{name}: {err}"));
        let cycles = executor.cycle.saturating_sub(before);
        if cycles == 0 {
            continue;
        }
        total += cycles;
        let frames = executor.callstack.frames();
        let names: Vec<String> = frames
            .iter()
            .map(|frame| frame.procedure("").map_or_else(|| "?".into(), |p| p.to_string()))
            .collect();
        *folded.entry(names.join(";")).or_default() += cycles;
        *procedures.entry(names.last().cloned().unwrap_or_default()).or_default() += cycles;
        if let Some(frame) = frames.last() {
            let location = frame
                .last_resolved(source_manager.as_ref())
                .map_or_else(|| "<unresolved>".into(), |l| l.to_string());
            if let Some(op) = frame.recent().back() {
                writeln!(ops, "{}\t{location}", op.display()).unwrap();
            }
            *locations.entry(location).or_default() += cycles;
        }
    }

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/batch_kernel_profile");
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = String::new();
    for (stack, cycles) in &folded {
        writeln!(text, "{stack} {cycles}").unwrap();
    }
    std::fs::write(dir.join("folded.txt"), text).unwrap();
    std::fs::write(dir.join("ops.txt"), ops).unwrap();
    for (file, counts) in [("procedures.txt", procedures), ("locations.txt", locations)] {
        let mut rows: Vec<_> = counts.into_iter().collect();
        rows.sort_by_key(|(_, cycles)| std::cmp::Reverse(*cycles));
        let mut text = format!("{name}: {total} cycles\n");
        for (key, cycles) in rows {
            let share = cycles as f64 * 100.0 / total as f64;
            writeln!(text, "{cycles:>8} {share:5.1}% {key}").unwrap();
        }
        std::fs::write(dir.join(file), text).unwrap();
    }
    eprintln!("{name}: {total} cycles, profile written to {}", dir.display());
}
