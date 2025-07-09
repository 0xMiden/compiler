use core::panic;
use std::collections::VecDeque;

use miden_core::{utils::group_slice_elements, FieldElement};
use miden_processor::AdviceInputs;
use midenc_debug::{Executor, TestFelt, ToMidenRepr};
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
fn test_blake3_hash() {
    let main_fn =
        "(a: [u8; 32]) -> [u8; 32] {  miden_stdlib_sys::blake3_hash_1to1(a) }".to_string();
    let artifact_name = "abi_transform_stdlib_blake3_hash";
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        artifact_name,
        &main_fn,
        config,
        ["--test-harness".into()],
    );
    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/{artifact_name}.masm")]);

    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<[u8; 32]>(), move |ibytes| {
        let hash_bytes = blake3::hash(&ibytes);
        let rs_out = hash_bytes.as_bytes();

        // Place the hash output at 20 * PAGE_SIZE, and the hash input at 21 * PAGE_SIZE
        let in_addr = 21u32 * 65536;
        let out_addr = 20u32 * 65536;
        let initializers = [Initializer::MemoryBytes {
            addr: in_addr,
            bytes: &ibytes,
        }];

        let owords = rs_out.to_words();

        dbg!(&ibytes, rs_out);

        // Arguments are: [hash_output_ptr, hash_input_ptr]
        let args = [Felt::new(in_addr as u64), Felt::new(out_addr as u64)];
        eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |trace| {
            let vm_in: [u8; 32] = trace
                .read_from_rust_memory(in_addr)
                .expect("expected memory to have been written");
            dbg!(&vm_in);
            prop_assert_eq!(&ibytes, &vm_in, "VM input mismatch");
            let vm_out: [u8; 32] = trace
                .read_from_rust_memory(out_addr)
                .expect("expected memory to have been written");
            dbg!(&vm_out);
            prop_assert_eq!(rs_out, &vm_out, "VM output mismatch");
            Ok(())
        })?;

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

#[test]
fn test_hash_elements_smoke() {
    let main_fn = r#"
    (f0: miden_stdlib_sys::Felt, f1: miden_stdlib_sys::Felt, f2: miden_stdlib_sys::Felt, f3: miden_stdlib_sys::Felt, f4: miden_stdlib_sys::Felt, f5: miden_stdlib_sys::Felt, f6: miden_stdlib_sys::Felt, f7: miden_stdlib_sys::Felt) -> miden_stdlib_sys::Felt {
        let input = [f0, f1, f2, f3, f4, f5, f6, f7];
        let res = miden_stdlib_sys::hash_elements(&input);
        res.inner.inner.0
    }"#
    .to_string();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("hash_elements_smoke", &main_fn, config, []);
    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/hash_elements_smoke.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/hash_elements_smoke.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/hash_elements_smoke.masm")]);

    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    let config = proptest::test_runner::Config::with_cases(1);
    let res = TestRunner::new(config).run(&any::<[midenc_debug::Felt; 8]>(), move |test_felts| {
        let raw_felts: Vec<Felt> = test_felts.into_iter().map(From::from).collect();
        dbg!(&raw_felts);
        let expected_digest = miden_core::crypto::hash::Rpo256::hash_elements(&raw_felts);
        let expected_felts: [TestFelt; 4] = [
            TestFelt(expected_digest[0]),
            TestFelt(expected_digest[1]),
            TestFelt(expected_digest[2]),
            TestFelt(expected_digest[3]),
        ];
        dbg!(&expected_felts);

        let args = [
            raw_felts[7],
            raw_felts[6],
            raw_felts[5],
            raw_felts[4],
            raw_felts[3],
            raw_felts[2],
            raw_felts[1],
            raw_felts[0],
        ];

        eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
            let res: Felt = trace.parse_result().unwrap();
            dbg!(res);
            dbg!(expected_digest[0]);
            prop_assert_eq!(res, expected_digest[0]);
            Ok(())
        })?;

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

#[test]
fn test_hash_elements_aligned() {
    let main_fn = r#"
    (input: &[miden_stdlib_sys::Felt]) -> miden_stdlib_sys::Felt {
        let res = miden_stdlib_sys::hash_elements(input);
        res.inner.inner.0
    }"#
    .to_string();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "hash_elements",
        &main_fn,
        config,
        ["--test-harness".into()],
    );
    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/hash_elements.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/hash_elements.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/hash_elements.masm")]);

    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    let config = proptest::test_runner::Config::with_cases(32);
    let res = TestRunner::new(config).run(&any::<Vec<midenc_debug::Felt>>(), move |test_felts| {
        let raw_felts: Vec<Felt> = test_felts.into_iter().map(From::from).collect();
        let expected_digest = miden_core::crypto::hash::Rpo256::hash_elements(&raw_felts);
        let expected_felts: [TestFelt; 4] = [
            TestFelt(expected_digest[0]),
            TestFelt(expected_digest[1]),
            TestFelt(expected_digest[2]),
            TestFelt(expected_digest[3]),
        ];
        let wide_ptr_addr = 30u32 * 65536;
        let mut wide_ptr = vec![
            Felt::from(wide_ptr_addr + 16),
            Felt::from(raw_felts.len() as u32),
            Felt::ZERO,
            Felt::ZERO,
        ];
        wide_ptr.extend_from_slice(&raw_felts);
        let initializers = [
            Initializer::MemoryFelts {
                addr: wide_ptr_addr / 4,
                felts: (&wide_ptr).into(),
            },
            // TODO: multiple initializers do not work
            // Initializer::MemoryFelts {
            //     addr: in_addr / 4,
            //     felts: raw_felts.into(),
            // },
        ];

        let args = [Felt::new(wide_ptr_addr as u64)];

        eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |trace| {
            let res: Felt = trace.parse_result().unwrap();
            dbg!(res);
            dbg!(expected_digest[0]);
            prop_assert_eq!(res, expected_digest[0]);
            Ok(())
        })?;

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}
