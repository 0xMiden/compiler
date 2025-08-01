use core::panic;
use std::collections::VecDeque;

use miden_core::{utils::group_slice_elements, FieldElement};
use miden_processor::AdviceInputs;
use midenc_debug::{Executor, FromMidenRepr, TestFelt, ToMidenRepr};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use midenc_session::Emit;
use proptest::{
    arbitrary::any,
    prelude::TestCaseError,
    prop_assert_eq,
    test_runner::{TestError, TestRunner},
};

use crate::{
    testing::{eval_package, Initializer},
    CompilerTest,
};

#[test]
fn test_adv_load_preimage() {
    let main_fn = r#"
    (key: Word) -> alloc::vec::Vec<Word> {
        let num_felts = adv_push_mapvaln(key);
        assert_eq(Felt::from_u32(num_felts % 4), felt!(0));
        let num_words = num_felts / 4;
        let commitment = key;
        adv_load_preimage(num_words, commitment)
    }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "adv_load_preimage",
        &main_fn,
        config,
        ["--test-harness".into()],
    );

    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/adv_load_preimage.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/adv_load_preimage.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/adv_load_preimage.masm")]);

    let package = test.compiled_package();

    // Create test data: 4 words (16 felts)
    let test_words: Vec<[Felt; 4]> = vec![
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        [Felt::new(5), Felt::new(6), Felt::new(7), Felt::new(8)],
        [Felt::new(9), Felt::new(10), Felt::new(11), Felt::new(12)],
        [Felt::new(13), Felt::new(14), Felt::new(15), Felt::new(16)],
    ];

    // Flatten words to felts for hashing
    let felts: Vec<Felt> = test_words.iter().flat_map(|w| w.iter().copied()).collect();

    let commitment = miden_core::crypto::hash::Rpo256::hash_elements(&felts);
    let mut advice_map = std::collections::BTreeMap::new();
    advice_map.insert(commitment, felts.clone());

    let args = [commitment[0], commitment[1], commitment[2], commitment[3]];

    let mut exec = Executor::for_package(&package, args.to_vec(), &test.session)
        .expect("Failed to create executor");
    exec.with_advice_inputs(AdviceInputs::default().with_map(advice_map));
    let trace = exec.execute(&package.unwrap_program(), &test.session);

    // The function returns a Vec<Word> which is a fat pointer
    // We'll need to read from memory to get the actual words
    let result_ptr: Felt = trace.parse_result().expect("Failed to parse result");

    // Read the Vec metadata from memory (capacity, ptr, len, padding)
    let vec_metadata: [TestFelt; 4] = trace
        .read_from_rust_memory((result_ptr.as_int() as u32) / 4)
        .expect("Failed to read vec metadata");

    let capacity = vec_metadata[0].0.as_int() as usize;
    let data_ptr = vec_metadata[1].0.as_int() as u32;
    let vec_len = vec_metadata[2].0.as_int() as usize;

    // Reconstruct the Vec<[Felt; 4]> by reading all words from memory
    let mut result_words: Vec<[Felt; 4]> = Vec::with_capacity(capacity);
    for i in 0..vec_len {
        let word_addr = (data_ptr / 4) + (i * 4) as u32;
        let actual_word: [TestFelt; 4] = trace
            .read_from_rust_memory(word_addr)
            .unwrap_or_else(|| panic!("Failed to read word at index {}", i));
        let actual_felt_word: [Felt; 4] =
            [actual_word[0].0, actual_word[1].0, actual_word[2].0, actual_word[3].0];
        result_words.push(actual_felt_word);
    }

    // Compare the reconstructed Vec exactly with test_words
    assert_eq!(result_words, test_words, "Vec<Word> mismatch");
}
