use std::sync::Arc;

use miden_assembly::Assembler;
use miden_core::Felt;
use miden_debug::{Executor, Felt as TestFelt};
use miden_protocol::{
    ProtocolLib,
    note::{NoteRecipient, NoteScript, NoteStorage},
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
fn test_note_build_recipient_matches_note_recipient_digest() -> Result<(), Report> {
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
    let storage = NoteStorage::new(vec![input1, input2]).expect("invalid note storage");
    let note_recipient = NoteRecipient::new(serial_num, note_script.clone(), storage);
    let expected_digest = note_recipient.digest();

    let main_fn = r#"(serial_num: Word, script_root: Word, storage: Vec<Felt>) -> Word {
        let recipient = note::build_recipient(serial_num, script_root, storage);
        recipient.inner
    }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTestBuilder::rust_fn_body_with_sdk(
        "abi_transform_tx_kernel_note_build_recipient",
        &main_fn,
        config,
        ["--test-harness".into(), "--link-library".into(), "base".into()],
    )
    .build();

    let package = test.compile_package();

    let inputs = [input1, input2];
    let script_root: miden_core::Word = note_script.root();

    // The Rust extern "C" ABI for this entrypoint uses byval pointers for the `Word`,
    // and `Vec` arguments. We initialize all three arguments in a single contiguous payload and
    // pass their byte pointers as inputs. The return value is written to an output buffer whose
    // pointer is passed as the first argument (see `test_adv_load_preimage` for similar patterns).
    let base_addr = 20u32 * 65536; // 1310720
    let serial_num_ptr = base_addr;
    let script_root_ptr = base_addr + 16;
    let vec_ptr = base_addr + 32;
    let vec_data_ptr = base_addr + 48;

    let out_addr = 21u32 * 65536;

    let serial_num_felts: [Felt; 4] = serial_num.into();
    let script_root_felts: [Felt; 4] = script_root.into();

    let mut init_felts = Vec::new();
    init_felts.extend_from_slice(&serial_num_felts);
    init_felts.extend_from_slice(&script_root_felts);
    init_felts.extend_from_slice(&[
        Felt::from(inputs.len() as u32),
        Felt::from(vec_data_ptr),
        Felt::from(inputs.len() as u32),
        Felt::new(0),
    ]);
    init_felts.extend_from_slice(&inputs);

    let args = [
        Felt::new(out_addr as u64),
        Felt::new(serial_num_ptr as u64),
        Felt::new(script_root_ptr as u64),
        Felt::new(vec_ptr as u64),
    ];

    let initializers = [Initializer::MemoryFelts {
        addr: base_addr / 4,
        felts: (&init_felts).into(),
    }];

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
