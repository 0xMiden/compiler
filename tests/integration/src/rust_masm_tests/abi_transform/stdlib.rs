use core::panic;
use std::collections::VecDeque;

use expect_test::expect_file;
use miden_core::utils::group_slice_elements;
use miden_processor::AdviceInputs;
use midenc_debug::{Executor, PopFromStack, PushToStack, TestFelt};
use midenc_hir::Felt;
use midenc_session::Emit;
use proptest::{
    arbitrary::any,
    prelude::TestCaseError,
    prop_assert_eq,
    test_runner::{TestError, TestRunner},
};

use crate::CompilerTest;

#[test]
fn test_blake3_hash() {
    let main_fn =
        "(a: [u8; 32]) -> [u8; 32] {  miden_stdlib_sys::blake3_hash_1to1(a) }".to_string();
    let artifact_name = "abi_transform_stdlib_blake3_hash";
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        artifact_name,
        &main_fn,
        true,
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
        let in_addr = 21u32 * 65536;
        let out_addr = 20u32 * 65536;
        let mut advice_stack = Vec::<Felt>::default();
        // Provide input bytes via the advice stack
        //
        // NOTE: This relies on MasmComponent to emit a test harness via `emit_test_harness` during
        // assembly of the package.
        //
        // First, convert the input bytes to words, zero-padding as needed; and push on to the
        // advice stack in reverse.
        let words = midenc_debug::bytes_to_words(&ibytes);
        for word in words.into_iter().rev() {
            PushToStack::try_push(&word, &mut advice_stack);
        }
        // The test harness invokes std::mem::pipe_words_to_memory, which expects the operand stack
        // to look like: `[num_words, write_ptr]`.
        //
        // Since we're feeding this data in via the advice stack, the test harness code will expect
        // these values on the advice stack in the opposite order, as the `adv_push` instruction
        // will pop each element off the advice stack, and push on to the operand stack, after which
        // these two values will be in the expected order.
        PushToStack::try_push(&2u32, &mut advice_stack); // [num_words]
        PushToStack::try_push(&(in_addr / 4), &mut advice_stack); // [dest_ptr, num_words]
        dbg!(&ibytes, &advice_stack, rs_out);
        // Arguments are: [hash_output_ptr, hash_input_ptr]
        let mut exec = Executor::for_package(
            &package,
            // Place the hash output at 20 * PAGE_SIZE, and the hash input at 21 * PAGE_SIZE
            vec![Felt::new(in_addr as u64), Felt::new(out_addr as u64)],
            &test.session,
        )
        .map_err(|err| TestCaseError::fail(err.to_string()))?;

        // Reverse the stack contents, so that the correct order is preserved after
        // MemAdviceProvider does its own reverse
        advice_stack.reverse();
        let advice_inputs = AdviceInputs::default().with_stack(advice_stack);
        exec.with_advice_inputs(advice_inputs);
        let trace = exec.execute(&package.unwrap_program(), &test.session);
        let vm_in: [u8; 32] = trace
            .read_from_rust_memory(in_addr)
            .expect("expected memory to have been written");
        dbg!(&vm_in);
        let vm_out: [u8; 32] = trace
            .read_from_rust_memory(out_addr)
            .expect("expected memory to have been written");
        dbg!(&vm_out);
        prop_assert_eq!(rs_out, &vm_out, "VM output mismatch");
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
