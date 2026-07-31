use miden_debug::Executor;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_session::diagnostics::Report;

use crate::CompilerTestBuilder;

#[test]
fn get_metadata() -> Result<(), Report> {
    // Mock the Miden protocol `active_note::get_metadata` procedure.
    //
    // The protocol signature returns a single metadata header word (4 felts) on the operand stack.
    let masm = r#"
pub proc get_metadata
    # Stack input: []
    # Stack output: [METADATA_HEADER]
    #
    # Return one word-sized value with distinct elements so we can validate that:
    # - the ABI adapter consumes exactly 4 felts
    # - the returned metadata word preserves the kernel order at the Rust call site
    #
    # The ABI adapter writes the current top of stack to the lowest memory address first, so the
    # values are pushed in reverse order within the returned word.
    push.24 push.23 push.22 push.21   # METADATA_HEADER
end
"#
    .to_string();

    let main_fn = r#"() -> () {
        let meta = miden::active_note::get_metadata();

        let header = meta.header;
        assert_eq(header[0], felt!(21));
        assert_eq(header[1], felt!(22));
        assert_eq(header[2], felt!(23));
        assert_eq(header[3], felt!(24));
    }"#
    .to_string();

    let artifact_name = "abi_transform_tx_kernel_get_metadata";
    let config = WasmTranslationConfig::default();
    let mut test_builder = CompilerTestBuilder::rust_fn_body_with_sdk_without_protocol(
        artifact_name,
        &main_fn,
        config,
        [],
    );
    test_builder.link_with_masm_module("miden::protocol::active_note", masm);
    let mut test = test_builder.build();

    let package = test.compile_package();

    let mut exec = Executor::new(vec![]);
    exec.with_package(miden_core_lib::CoreLibrary::default().package()).unwrap();

    let _ = exec.execute(package, test.session.source_manager.clone());
    Ok(())
}
