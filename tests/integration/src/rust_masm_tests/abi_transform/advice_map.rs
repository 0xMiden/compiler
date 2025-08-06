use core::panic;
use std::collections::VecDeque;

use miden_core::{utils::group_slice_elements, FieldElement, StarkField};
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
    (k0: Felt, k1: Felt, k2: Felt, k3: Felt) -> alloc::vec::Vec<Felt> {
        let key = Word::from([k0, k1, k2, k3]);

        let num_felts = intrinsics::advice::adv_push_mapvaln(key.clone());

        let num_felts_u64 = num_felts.as_u64();
        assert_eq(Felt::from_u32((num_felts_u64 % 4) as u32), felt!(0));
        let num_words = Felt::from_u64_unchecked(num_felts_u64 / 4);

        let commitment = key;
        adv_load_preimage(num_words, commitment)
    }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("adv_load_preimage", &main_fn, config, []);

    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/adv_load_preimage.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/adv_load_preimage.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/adv_load_preimage.masm")]);

    let package = test.compiled_package();

    // Create test data: 4 words (16 felts)
    let input: Vec<Felt> = vec![
        Felt::new(1),
        Felt::new(2),
        Felt::new(3),
        Felt::new(4),
        Felt::new(5),
        Felt::new(6),
        Felt::new(7),
        Felt::new(8),
        Felt::new(9),
        Felt::new(10),
        Felt::new(11),
        Felt::new(12),
        Felt::new(13),
        Felt::new(14),
        Felt::new(15),
        Felt::new(Felt::MODULUS - 1),
    ];

    let commitment = miden_core::crypto::hash::Rpo256::hash_elements(&input);
    dbg!(&commitment.to_hex());
    let mut advice_map = std::collections::BTreeMap::new();
    advice_map.insert(commitment, input.clone());

    let out_addr = 20u32 * 65536;
    let args = [
        commitment[3],
        commitment[2],
        commitment[1],
        commitment[0],
        Felt::new(out_addr as u64),
    ];

    let mut exec = Executor::for_package(&package, args.to_vec(), &test.session)
        .expect("Failed to create executor");
    exec.with_advice_inputs(AdviceInputs::default().with_map(advice_map));
    let trace = exec.execute(&package.unwrap_program(), &test.session);

    let result_ptr = out_addr;
    // Read the Vec metadata from memory (capacity, ptr, len, padding)
    let vec_metadata: [TestFelt; 4] =
        trace.read_from_rust_memory(result_ptr).expect("Failed to read vec metadata");

    let capacity = vec_metadata[0].0.as_int() as usize;
    let data_ptr = vec_metadata[1].0.as_int() as u32;
    let vec_len = vec_metadata[2].0.as_int() as usize;
    dbg!(capacity, data_ptr, vec_len);

    // Reconstruct the Vec<Felt> by reading all words from memory
    let mut loaded: Vec<Felt> = Vec::with_capacity(capacity);
    for i in 0..(vec_len / 4) {
        let word_addr = data_ptr + (i * 4 * 4) as u32;
        let w: [TestFelt; 4] = trace
            .read_from_rust_memory(word_addr)
            .unwrap_or_else(|| panic!("Failed to read word at index {}", i));
        loaded.push(w[0].0);
        loaded.push(w[1].0);
        loaded.push(w[2].0);
        loaded.push(w[3].0);
    }

    // Compare the reconstructed Vec exactly with input
    assert_eq!(loaded, input, "Vec<Word> mismatch");
}
