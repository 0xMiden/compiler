use miden_debug::Executor;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_session::{STDLIB, diagnostics::Report};

use crate::CompilerTestBuilder;

#[test]
fn test_get_metadata() -> Result<(), Report> {
    // Mock the Miden protocol `active_note::get_metadata` procedure.
    //
    // The raw protocol signature returns 8 felts on the operand stack:
    // `[NOTE_ATTACHMENT (4), METADATA_HEADER (4)]`.
    let masm = r#"
pub proc get_metadata
    # Stack input: []
    # Stack output: [NOTE_ATTACHMENT, METADATA_HEADER]
    #
    # Return two word-sized values with distinct elements so we can validate that:
    # - the ABI adapter consumes all 8 felts (not just 4)
    # - the words are grouped correctly
    # - the returned metadata words preserve the kernel order at the Rust call site
    #
    # The ABI adapter writes the current top of stack to the lowest memory address first, so the
    # values are pushed in reverse order within each returned word.
    push.24 push.23 push.22 push.21   # METADATA_HEADER
    push.14 push.13 push.12 push.11   # NOTE_ATTACHMENT
end
"#
    .to_string();

    let main_fn = r#"() -> () {
        let meta = miden::active_note::get_metadata();

        let attachment = meta.attachment;
        assert_eq(attachment[0], felt!(11));
        assert_eq(attachment[1], felt!(12));
        assert_eq(attachment[2], felt!(13));
        assert_eq(attachment[3], felt!(14));

        let header = meta.header;
        assert_eq(header[0], felt!(21));
        assert_eq(header[1], felt!(22));
        assert_eq(header[2], felt!(23));
        assert_eq(header[3], felt!(24));
    }"#
    .to_string();

    let artifact_name = "abi_transform_tx_kernel_get_metadata";
    let config = WasmTranslationConfig::default();
    let mut test_builder =
        CompilerTestBuilder::rust_fn_body_with_sdk(artifact_name, &main_fn, config, []);
    test_builder.link_with_masm_module("miden::protocol::active_note", masm);
    let mut test = test_builder.build();

    let package = test.compile_package();

    let mut exec = Executor::new(vec![]);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut().insert(*std_library.digest(), std_library);
    exec.with_dependencies(package.manifest.dependencies())?;

    let _ = exec.execute(&package.unwrap_program(), test.session.source_manager.clone());
    Ok(())
}
