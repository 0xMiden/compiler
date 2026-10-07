//! Compiles the examples that link the standard (MASM) basic wallet account component through a
//! registry dependency on its package, with no WIT of its own.

use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{CompilerTestBuilder, example_build_lock, workspace_root};

use super::super::support::sysroot_with_standard_components;

/// The Miden path of the standard basic wallet component's procedures.
const BASIC_WALLET: &str = "::miden::standards::components::wallets::basic_wallet";

/// Compiles the example at `project` in release mode against the sysroot that ships the standard
/// account components, and returns the MASM handed to the assembler.
fn compile_masm(project: &str) -> String {
    let _build_lock = example_build_lock(&workspace_root());
    let sysroot = sysroot_with_standard_components().display().to_string();
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(
        project,
        WasmTranslationConfig::default(),
        ["--sysroot".to_string(), sysroot],
    );
    builder.with_release(true);
    let mut test = builder.build();
    let masm = test.masm_src();
    // Assembling the package proves the emitted MASM links against the standard wallet.
    let _package = test.compile_package();
    masm
}

/// Asserts that `masm` calls the standard basic wallet procedure `procedure`.
fn assert_calls_wallet(masm: &str, procedure: &str) {
    let call = format!("call.{BASIC_WALLET}::{procedure}");
    assert!(masm.contains(&call), "expected `{call}` in the emitted MASM:\n{masm}");
}

#[test]
fn std_wallet_p2id_note_calls_standard_wallet() {
    let masm = compile_masm("../../examples/std-wallet-p2id-note");
    assert_calls_wallet(&masm, "receive_asset");
}

#[test]
fn std_wallet_tx_script_calls_standard_wallet() {
    let masm = compile_masm("../../examples/std-wallet-tx-script");
    assert_calls_wallet(&masm, "create_note");
    assert_calls_wallet(&masm, "move_asset_to_note");
}
