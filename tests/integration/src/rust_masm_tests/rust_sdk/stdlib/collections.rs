use std::sync::Arc;

use miden_core::{
    Felt, FieldElement, Word,
    crypto::merkle::{MerkleStore, Smt},
};
use miden_debug::Executor;
use miden_lib::MidenLib;
use miden_processor::AdviceInputs;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_session::STDLIB;

use crate::CompilerTest;

fn felt(value: u64) -> Felt {
    Felt::new(value)
}

fn word(e0: u64, e1: u64, e2: u64, e3: u64) -> Word {
    [felt(e0), felt(e1), felt(e2), felt(e3)].into()
}

fn build_advice_inputs_for_smt(smt: &Smt) -> AdviceInputs {
    let store = MerkleStore::from(smt);
    let map = smt
        .leaves()
        .map(|(_, leaf)| (leaf.hash(), leaf.to_elements()))
        .collect::<Vec<_>>();

    AdviceInputs::default()
        .with_stack([Felt::ZERO])
        .with_map(map)
        .with_merkle_store(store)
}

fn word_components(word: Word) -> [Felt; 4] {
    word.into()
}

fn word_to_u64s(word: Word) -> [u64; 4] {
    let [a, b, c, d] = word_components(word);
    [a.as_int(), b.as_int(), c.as_int(), d.as_int()]
}

fn push_word_args(args: &mut Vec<Felt>, word: Word) {
    let [a, b, c, d] = word_components(word);
    args.push(d);
    args.push(c);
    args.push(b);
    args.push(a);
}

fn executor_with_std(args: Vec<Felt>) -> Executor {
    let mut exec = Executor::new(args);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut()
        .add(*std_library.digest(), std_library.clone().into());
    let base_library = Arc::new(MidenLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*base_library.digest(), base_library.clone().into());
    exec
}

#[test]
fn test_smt_get_binding() {
    let entries = [
        (word(1, 2, 3, 100), word(10, 11, 12, 13)),
        (word(5, 6, 7, 200), word(20, 21, 22, 23)),
        (word(8, 9, 10, 200), word(30, 31, 32, 33)),
    ];
    let smt = Smt::with_entries(entries).expect("smt initialization failed");
    let (key, expected_value) = entries[0];
    let root = smt.root();

    let expected_value_u64 = word_to_u64s(expected_value);

    let main_fn = format!(
        r#"
    (
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        r0: Felt,
        r1: Felt,
        r2: Felt,
        r3: Felt
    ) {{
        let key = Word::new([k0, k1, k2, k3]);
        let root = Word::new([r0, r1, r2, r3]);
        let result = smt_get(key, root);
        let value = result.value;
        let returned_root = result.root;

        let expected = Word::new([
            Felt::from_u64_unchecked({ev0}),
            Felt::from_u64_unchecked({ev1}),
            Felt::from_u64_unchecked({ev2}),
            Felt::from_u64_unchecked({ev3}),
        ]);
        assert_eq!(value, expected, "smt_get returned unexpected value");
        assert_eq!(returned_root, root, "smt_get should not mutate the root");
    }}"#,
        ev0 = expected_value_u64[0],
        ev1 = expected_value_u64[1],
        ev2 = expected_value_u64[2],
        ev3 = expected_value_u64[3],
    );

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "rust_sdk_stdlib_smt_get",
        &main_fn,
        config,
        ["--test-harness".into()],
    );
    test.expect_wasm(expect_file!["../../../../expected/rust_sdk_stdlib_smt_get.wat"]);
    test.expect_ir(expect_file!["../../../../expected/rust_sdk_stdlib_smt_get.hir"]);
    test.expect_masm(expect_file!["../../../../expected/rust_sdk_stdlib_smt_get.masm"]);

    let package = test.compiled_package();

    let advice_inputs = build_advice_inputs_for_smt(&smt);

    let mut args = Vec::new();
    push_word_args(&mut args, root);
    push_word_args(&mut args, key);

    let mut exec = executor_with_std(args.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    exec.with_advice_inputs(advice_inputs);

    exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
}

#[test]
fn test_smt_set_binding() {
    let key = word(5, 6, 7, 200);
    let new_value = word(40, 41, 42, 43);

    let smt = Smt::new();
    let root = smt.root();

    let mut expected_smt = smt.clone();
    let expected_old_value =
        expected_smt.insert(key, new_value).expect("inserting into SMT should succeed");
    let expected_new_root = expected_smt.root();

    let expected_old_u64 = word_to_u64s(expected_old_value);
    let expected_new_u64 = word_to_u64s(expected_new_root);

    let main_fn = format!(
        r#"
    (
        v0: Felt,
        v1: Felt,
        v2: Felt,
        v3: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        r0: Felt,
        r1: Felt,
        r2: Felt,
        r3: Felt
    ) {{
        let value = Word::new([v0, v1, v2, v3]);
        let key = Word::new([k0, k1, k2, k3]);
        let root = Word::new([r0, r1, r2, r3]);
        let result = smt_set(value, key, root);
        let old_value = result.old_value;
        let new_root = result.new_root;

        let expected_old = Word::new([
            Felt::from_u64_unchecked({eo0}),
            Felt::from_u64_unchecked({eo1}),
            Felt::from_u64_unchecked({eo2}),
            Felt::from_u64_unchecked({eo3}),
        ]);
        let expected_new = Word::new([
            Felt::from_u64_unchecked({en0}),
            Felt::from_u64_unchecked({en1}),
            Felt::from_u64_unchecked({en2}),
            Felt::from_u64_unchecked({en3}),
        ]);
        assert_eq!(old_value, expected_old, "smt_set returned unexpected old value");
        assert_eq!(new_root, expected_new, "smt_set returned unexpected new root");
    }}"#,
        eo0 = expected_old_u64[0],
        eo1 = expected_old_u64[1],
        eo2 = expected_old_u64[2],
        eo3 = expected_old_u64[3],
        en0 = expected_new_u64[0],
        en1 = expected_new_u64[1],
        en2 = expected_new_u64[2],
        en3 = expected_new_u64[3],
    );

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "rust_sdk_stdlib_smt_set",
        &main_fn,
        config,
        ["--test-harness".into()],
    );
    test.expect_wasm(expect_file!["../../../../expected/rust_sdk_stdlib_smt_set.wat"]);
    test.expect_ir(expect_file!["../../../../expected/rust_sdk_stdlib_smt_set.hir"]);
    test.expect_masm(expect_file!["../../../../expected/rust_sdk_stdlib_smt_set.masm"]);

    let package = test.compiled_package();

    let advice_inputs = build_advice_inputs_for_smt(&smt);

    let mut args = Vec::new();
    push_word_args(&mut args, root);
    push_word_args(&mut args, key);
    push_word_args(&mut args, new_value);

    let mut exec = executor_with_std(args.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    exec.with_advice_inputs(advice_inputs);

    exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
}
