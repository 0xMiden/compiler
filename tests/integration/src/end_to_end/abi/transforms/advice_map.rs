use core::panic;

use miden_core::field::PrimeField64;
use miden_debug::{DebugQuery, Felt as TestFelt};
use miden_processor::advice::AdviceInputs;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use midenc_integration_test_support::testing::executor_with_std;

use crate::CompilerTest;

#[test]
fn adv_load_preimage() {
    let main_fn = r#"
    (k0: Felt, k1: Felt, k2: Felt, k3: Felt) -> alloc::vec::Vec<Felt> {
        let key = Word::from([k0, k1, k2, k3]);

        let num_felts = intrinsics::advice::adv_push_mapvaln(key.clone());

        let num_felts_u64 = num_felts.as_canonical_u64();
        assert_eq(Felt::new((num_felts_u64 % 4) as u64).unwrap(), felt!(0));
        let num_words = Felt::new(num_felts_u64 / 4).unwrap();

        let commitment = key;
        let input = adv_load_preimage(num_words, commitment);
        assert_eq(input[0], felt!(1));
        assert_eq(input[1], felt!(2));
        assert_eq(input[5], felt!(6));
        assert_eq(input[14], felt!(15));
        input
    }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("adv_load_preimage", &main_fn, config, []);

    let package = test.compile_package();

    // Create test data: 4 words (16 felts)
    let input: Vec<Felt> = vec![
        Felt::new_unchecked(1),
        Felt::new_unchecked(2),
        Felt::new_unchecked(3),
        Felt::new_unchecked(4),
        Felt::new_unchecked(5),
        Felt::new_unchecked(6),
        Felt::new_unchecked(7),
        Felt::new_unchecked(8),
        Felt::new_unchecked(9),
        Felt::new_unchecked(10),
        Felt::new_unchecked(11),
        Felt::new_unchecked(12),
        Felt::new_unchecked(13),
        Felt::new_unchecked(14),
        Felt::new_unchecked(15),
        Felt::new_unchecked(Felt::ORDER_U64 - 1),
    ];

    let commitment = miden_core::crypto::hash::Poseidon2::hash_elements(&input);
    let mut advice_map = std::collections::BTreeMap::new();
    advice_map.insert(commitment, input.clone());

    let out_addr = 20u32 * 65536;
    let args = [
        Felt::new_unchecked(out_addr as u64),
        commitment[0],
        commitment[1],
        commitment[2],
        commitment[3],
    ];

    let mut exec = executor_with_std(args.to_vec());
    exec.with_advice_inputs(AdviceInputs::default().with_map(advice_map));
    let trace = exec.execute(package, test.session.source_manager.clone());

    let result_ptr = out_addr;
    // Read the Vec metadata from memory (capacity, ptr, len, padding)
    let vec_metadata: [TestFelt; 4] =
        trace.read_from_rust_memory(result_ptr).expect("Failed to read vec metadata");

    let capacity = vec_metadata[0].0.as_canonical_u64() as usize;
    let data_ptr = vec_metadata[1].0.as_canonical_u64() as u32;
    let vec_len = vec_metadata[2].0.as_canonical_u64() as usize;

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
