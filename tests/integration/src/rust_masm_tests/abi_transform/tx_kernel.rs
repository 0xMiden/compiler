use std::fmt::Write;

use expect_test::expect_file;
use miden_assembly::LibraryPath;
use miden_core::{Felt, FieldElement};
use miden_processor::ExecutionError;
use midenc_debug::Executor;
use midenc_session::{diagnostics::Report, Emit};

use crate::{execute_emulator, CompilerTestBuilder};

#[allow(unused)]
fn setup_log() {
    use log::LevelFilter;
    let _ = env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .format_timestamp(None)
        .is_test(true)
        .try_init();
}

#[test]
#[ignore = "temporarily disabled until we figure out what broke"]
fn test_get_inputs_4() -> Result<(), Report> {
    test_get_inputs("4", vec![u32::MAX.into(), Felt::ONE, Felt::ZERO, u32::MAX.into()])
}

fn test_get_inputs(test_name: &str, expected_inputs: Vec<Felt>) -> Result<(), Report> {
    assert!(expected_inputs.len() == 4, "for now only word-sized inputs are supported");
    let masm = format!(
        "
export.get_inputs
    push.{expect1}.{expect2}.{expect3}.{expect4}
    # write word to memory, leaving the pointer on the stack
    dup.4 mem_storew dropw
    # push the inputs len on the stack
    push.4
end
",
        expect1 = expected_inputs.first().map(|i| i.as_int()).unwrap_or(0),
        expect2 = expected_inputs.get(1).map(|i| i.as_int()).unwrap_or(0),
        expect3 = expected_inputs.get(2).map(|i| i.as_int()).unwrap_or(0),
        expect4 = expected_inputs.get(3).map(|i| i.as_int()).unwrap_or(0),
    );
    let main_fn = "() -> Vec<Felt> { miden::note::get_inputs() }";
    let artifact_name = format!("abi_transform_tx_kernel_get_inputs_{}", test_name);
    let mut test_builder =
        CompilerTestBuilder::rust_fn_body_with_sdk(artifact_name.clone(), main_fn, true, None);
    test_builder.link_with_masm_module("miden::note", masm);
    let mut test = test_builder.build();

    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/{artifact_name}.masm")]);

    let package = test.compiled_package();

    // Provide a place in memory where the vector returned by `get_inputs` should be stored
    let out_addr = 18u32 * 65536;
    let exec = Executor::for_package(&package, vec![Felt::new(out_addr as u64)], &test.session)?;
    let trace = exec.execute(&package.unwrap_program(), &test.session);
    // Verify that the vector contains the expected elements:
    //
    // Rust lays out the vector struct as follows (lowest addressed bytes first):
    //
    //     [capacity, buf_ptr, len]
    //
    // 1. Extract the data pointer and length from the vector written to out_addr
    let data_ptr = trace.read_memory_element(out_addr / 16, 1).unwrap().as_int() as u32;
    assert_ne!(data_ptr, 0, "expected non-null data pointer");
    dbg!(data_ptr);
    let len = trace.read_memory_element(out_addr / 16, 2).unwrap().as_int() as usize;
    assert_eq!(
        len,
        expected_inputs.len(),
        "expected vector to contain all of the expected inputs"
    );
    // 2. Read the vector elements via data_ptr and ensure they match the inputs
    dbg!(len);
    let word = trace.read_memory_word(data_ptr / 16).unwrap();
    assert_eq!(
        word.as_slice(),
        expected_inputs.as_slice(),
        "expected vector contents to match inputs"
    );

    // let ir_program = test.ir_masm_program();
    // let emul_out = execute_emulator(ir_program.clone(), &[]);
    Ok(())
}
