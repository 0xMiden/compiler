use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use super::persist_cargo_miden_dependency;
use crate::{CompilerTest, testing::stripped_mast_size_str};

fn no_debug_flags() -> [String; 2] {
    ["--debug".to_string(), "none".to_string()]
}

#[test]
fn basic_wallet_and_p2id() {
    let config = WasmTranslationConfig::default();
    let mut account_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/basic-wallet",
        config.clone(),
        no_debug_flags(),
    );
    let account_package = account_test.compile_package();
    assert!(account_package.is_library(), "expected library");
    expect!["15614"].assert_eq(stripped_mast_size_str(&account_package));
    persist_cargo_miden_dependency("../../examples/basic-wallet", account_package.as_ref());

    let mut tx_script_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/basic-wallet-tx-script",
        config.clone(),
        no_debug_flags(),
    );
    let tx_script_package = tx_script_test.compile_package();
    assert!(tx_script_package.is_library(), "expected library");
    expect!["19659"].assert_eq(stripped_mast_size_str(&tx_script_package));

    let mut p2id_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/p2id-note",
        config.clone(),
        no_debug_flags(),
    );
    let note_package = p2id_test.compile_package();
    assert!(note_package.is_library(), "expected library");
    expect!["26542"].assert_eq(stripped_mast_size_str(&note_package));
    // The note package exports both the note script and the `create` constructor; the
    // constructor must not interfere with the `@note_script`-attributed export selection.
    assert!(
        note_package.manifest.exports().any(|export| export.name() == "create"),
        "expected the p2id note package to export the `create` constructor"
    );
    miden_protocol::note::NoteScript::from_package(&note_package)
        .expect("expected the p2id note package to contain exactly one note script export");

    let mut p2ide_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/p2ide-note",
        config,
        no_debug_flags(),
    );
    let p2ide_package = p2ide_test.compile_package();
    assert!(p2ide_package.is_library(), "expected library");
    expect!["22787"].assert_eq(stripped_mast_size_str(&p2ide_package));
}
