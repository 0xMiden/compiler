use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use super::persist_cargo_miden_dependency;
use crate::{
    CompilerTest, assert_helpers::assert_lifted_component_exports, testing::stripped_mast_size_str,
};

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
    assert_lifted_component_exports(
        account_package.as_ref(),
        &[
            r#"::"miden:basic-wallet/miden-basic-wallet@0.1.0"::"move-asset-to-note""#,
            r#"::"miden:basic-wallet/miden-basic-wallet@0.1.0"::"receive-asset""#,
        ],
    );
    expect!["8045"].assert_eq(stripped_mast_size_str(&account_package));
    persist_cargo_miden_dependency("../../examples/basic-wallet", account_package.as_ref());

    let mut tx_script_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/basic-wallet-tx-script",
        config.clone(),
        no_debug_flags(),
    );
    let tx_script_package = tx_script_test.compile_package();
    assert!(tx_script_package.is_library(), "expected library");
    assert_lifted_component_exports(
        tx_script_package.as_ref(),
        &[r#"::"miden:base/transaction-script@1.0.0"::run"#],
    );
    expect!["16636"].assert_eq(stripped_mast_size_str(&tx_script_package));

    let mut p2id_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/p2id-note",
        config.clone(),
        no_debug_flags(),
    );
    let note_package = p2id_test.compile_package();
    assert!(note_package.is_library(), "expected library");
    assert_lifted_component_exports(
        note_package.as_ref(),
        &[r#"::"miden:p2id/miden-p2id@0.1.0"::script"#],
    );
    expect!["16681"].assert_eq(stripped_mast_size_str(&note_package));

    let mut p2ide_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/p2ide-note",
        config,
        no_debug_flags(),
    );
    let p2ide_package = p2ide_test.compile_package();
    assert!(p2ide_package.is_library(), "expected library");
    assert_lifted_component_exports(
        p2ide_package.as_ref(),
        &[r#"::"miden:p2ide/miden-p2ide@0.1.0"::run"#],
    );
    expect!["19823"].assert_eq(stripped_mast_size_str(&p2ide_package));
}
