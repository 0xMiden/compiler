use core::panic;
use std::collections::VecDeque;

use miden_core::utils::group_slice_elements;
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
