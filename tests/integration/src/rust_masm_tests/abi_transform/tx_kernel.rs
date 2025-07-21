use std::fmt::Write;

use miden_assembly::LibraryPath;
use miden_core::{Felt, FieldElement};
use miden_processor::ExecutionError;
use midenc_debug::Executor;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_session::{diagnostics::Report, Emit};

use crate::CompilerTestBuilder;

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
fn test_get_inputs_4() -> Result<(), Report> {
    test_get_inputs("4", vec![u32::MAX, 1, 2, 3])
}

fn test_get_inputs(test_name: &str, expected_inputs: Vec<u32>) -> Result<(), Report> {
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
        expect1 = expected_inputs[0],
        expect2 = expected_inputs[1],
        expect3 = expected_inputs[2],
        expect4 = expected_inputs[3],
    );
    let main_fn = format!(
        r#"() -> () {{
        let v = miden::note::get_inputs();
        assert_eq(v.len().into(), felt!(4));
        assert_eq(v[0], felt!({expect1}));
        assert_eq(v[1], felt!({expect2}));
        assert_eq(v[2], felt!({expect3}));
        assert_eq(v[3], felt!({expect4}));
    }}"#,
        expect1 = expected_inputs[0],
        expect2 = expected_inputs[1],
        expect3 = expected_inputs[2],
        expect4 = expected_inputs[3],
    );
    let artifact_name = format!("abi_transform_tx_kernel_get_inputs_{test_name}");
    let config = WasmTranslationConfig::default();
    let mut test_builder =
        CompilerTestBuilder::rust_fn_body_with_sdk(artifact_name.clone(), &main_fn, config, []);
    test_builder.link_with_masm_module("miden::note", masm);
    let mut test = test_builder.build();

    test.expect_wasm(expect_file![format!("../../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../../expected/{artifact_name}.masm")]);
    let package = test.compiled_package();

    let exec = Executor::for_package(&package, vec![], &test.session)?;
    let _ = exec.execute(&package.unwrap_program(), &test.session);
    Ok(())
}

#[test]
fn test_get_id() {
    let main_fn = "() -> Felt { miden::account::get_id().into() }";
    let artifact_name = "abi_transform_tx_kernel_get_id";
    let config = WasmTranslationConfig::default();
    let test_builder =
        CompilerTestBuilder::rust_fn_body_with_sdk(artifact_name, main_fn, config, []);
    let mut test = test_builder.build();
    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/{artifact_name}.hir")]);
}
