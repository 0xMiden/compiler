use std::sync::Arc;

use miden_debug::Executor;
use miden_protocol::ProtocolLib;
use miden_standards::StandardsLib;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_session::{STDLIB, diagnostics::Report};

use crate::CompilerTestBuilder;

#[test]
fn test_get_inputs_4() -> Result<(), Report> {
    test_get_inputs("4", vec![u32::MAX, 1, 2, 3])
}

fn test_get_inputs(test_name: &str, expected_inputs: Vec<u32>) -> Result<(), Report> {
    assert!(expected_inputs.len() == 4, "for now only word-sized inputs are supported");
    let masm = format!(
        "
pub proc get_storage
    # Stack input: [dest_ptr]
    #
    # Write 4 inputs to memory starting at `dest_ptr`, then return `[num_inputs, dest_ptr]`.
    #
    # This matches the Miden protocol `active_note::get_storage` convention, where `dest_ptr` is
    # preserved on the operand stack alongside `num_inputs`.
    dup.0 push.{expect1} swap.1 mem_store
    dup.0 push.1 u32wrapping_add push.{expect2} swap.1 mem_store
    dup.0 push.2 u32wrapping_add push.{expect3} swap.1 mem_store
    dup.0 push.3 u32wrapping_add push.{expect4} swap.1 mem_store
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
        let v = miden::active_note::get_storage();
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
    test_builder.link_with_masm_module("miden::protocol::active_note", masm);
    let mut test = test_builder.build();

    let package = test.compile_package();

    let mut exec = Executor::new(vec![]);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut().insert(*std_library.digest(), std_library);
    let protocol_library = Arc::new(ProtocolLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .insert(*protocol_library.digest(), protocol_library);
    let standards_library = Arc::new(StandardsLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .insert(*standards_library.digest(), standards_library);
    exec.with_dependencies(package.manifest.dependencies())?;

    let _ = exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
    Ok(())
}
