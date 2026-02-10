use std::sync::Arc;

use miden_assembly::Assembler;
use miden_core::Felt;
use miden_debug::{Executor, Felt as TestFelt};
use miden_protocol::{
    ProtocolLib,
    note::{NoteInputs, NoteRecipient, NoteScript},
};
use miden_standards::StandardsLib;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_session::{Emit, STDLIB, diagnostics::Report};

use crate::{
    CompilerTestBuilder,
    testing::{Initializer, eval_package},
};

#[test]
fn test_get_inputs_4() -> Result<(), Report> {
    test_get_inputs("4", vec![u32::MAX, 1, 2, 3])
}

fn test_get_inputs(test_name: &str, expected_inputs: Vec<u32>) -> Result<(), Report> {
    assert!(expected_inputs.len() == 4, "for now only word-sized inputs are supported");
    let masm = format!(
        "
pub proc get_inputs
    # Stack input: [dest_ptr]
    #
    # Write 4 inputs to memory starting at `dest_ptr`, then return `[num_inputs, dest_ptr]`.
    #
    # This matches the Miden protocol `active_note::get_inputs` convention, where `dest_ptr` is
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
        let v = miden::active_note::get_inputs();
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

    let package = test.compiled_package();

    let mut exec = Executor::new(vec![]);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut()
        .add(*std_library.digest(), std_library.clone().into());
    let protocol_library = Arc::new(ProtocolLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*protocol_library.digest(), protocol_library.clone().into());
    let standards_library = Arc::new(StandardsLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*standards_library.digest(), standards_library.clone().into());
    exec.with_dependencies(package.manifest.dependencies())?;

    let _ = exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
    Ok(())
}

#[test]
fn test_recipient_compute_matches_note_recipient_digest() -> Result<(), Report> {
    let note_script_program = Assembler::default()
        .assemble_program(
            r#"
begin
    push.1
    drop
end
"#,
        )
        .expect("failed to assemble note script program");
    let note_script = NoteScript::new(note_script_program);

    let serial_num =
        miden_core::Word::new([Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)]);
    let input1 = Felt::new(5);
    let input2 = Felt::new(6);
    let inputs = NoteInputs::new(vec![input1, input2]).expect("invalid note inputs");
    let note_recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs);
    let expected_digest = note_recipient.digest();

    let main_fn = r#"(serial_num: Word, script_digest: Digest, inputs: Vec<Felt>) -> Word {
        let recipient = Recipient::compute(serial_num, script_digest, inputs);
        recipient.inner
    }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTestBuilder::rust_fn_body_with_sdk(
        "abi_transform_tx_kernel_recipient_compute",
        &main_fn,
        config,
        ["--test-harness".into()],
    )
    .build();

    let package = test.compiled_package();

    let inputs = [input1, input2];
    let script_root: miden_core::Word = note_script.root();

    // The Rust extern "C" ABI for this entrypoint uses byval pointers for the `Word`, `Digest`,
    // and `Vec` arguments. We initialize all three arguments in a single contiguous payload and
    // pass their byte pointers as inputs. The return value is written to an output buffer, whose
    // pointer is passed as the final argument (see `test_adv_load_preimage` for similar patterns).
    let base_addr = 20u32 * 65536; // 1310720
    let serial_num_ptr = base_addr;
    let script_digest_ptr = base_addr + 16;
    let vec_ptr = base_addr + 32;
    let vec_data_ptr = base_addr + 48;

    let out_addr = 21u32 * 65536;

    let serial_num_felts: [Felt; 4] = serial_num.into();
    let script_digest_felts: [Felt; 4] = script_root.into();

    let mut init_felts = Vec::new();
    init_felts.extend_from_slice(&serial_num_felts);
    init_felts.extend_from_slice(&script_digest_felts);
    init_felts.extend_from_slice(&[
        Felt::from(inputs.len() as u32),
        Felt::from(vec_data_ptr),
        Felt::from(inputs.len() as u32),
        Felt::new(0),
    ]);
    init_felts.extend_from_slice(&inputs);

    let initializers = [Initializer::MemoryFelts {
        addr: base_addr / 4,
        felts: (&init_felts).into(),
    }];

    let args = [
        Felt::new(vec_ptr as u64),
        Felt::new(script_digest_ptr as u64),
        Felt::new(serial_num_ptr as u64),
        Felt::new(out_addr as u64),
    ];

    let _ = eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |trace| {
        let actual: [TestFelt; 4] =
            trace.read_from_rust_memory(out_addr).expect("expected output to be written");
        let expected: [Felt; 4] = expected_digest.into();
        assert_eq!(
            [actual[0].0, actual[1].0, actual[2].0, actual[3].0],
            expected,
            "recipient digest mismatch"
        );
        Ok(())
    })
    .map_err(|err| Report::msg(err.to_string()))?;
    Ok(())
}

#[test]
fn test_get_id() {
    let main_fn = "() -> AccountId { miden::active_account::get_id() }";
    let artifact_name = "abi_transform_tx_kernel_get_id";
    let config = WasmTranslationConfig::default();
    let test_builder =
        CompilerTestBuilder::rust_fn_body_with_sdk(artifact_name, main_fn, config, []);
    let mut test = test_builder.build();
    // Test expected compilation artifacts
    test.expect_wasm(expect_file![format!("../../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../expected/{artifact_name}.hir")]);
}
