use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::stripped_mast_size_str};

#[test]
fn basic_wallet_and_p2id() {
    let config = WasmTranslationConfig::default();
    let mut account_test =
        CompilerTest::rust_source_cargo_miden("../../examples/basic-wallet", config.clone(), []);
    let account_package = account_test.compile_package();
    assert!(account_package.is_library(), "expected library");
    expect!["35906"].assert_eq(stripped_mast_size_str(&account_package));

    let mut tx_script_test = CompilerTest::rust_source_cargo_miden(
        "../../examples/basic-wallet-tx-script",
        config.clone(),
        [],
    );
    let tx_script_package = tx_script_test.compile_package();
    assert!(tx_script_package.is_program(), "expected program");
    expect!["56437"].assert_eq(stripped_mast_size_str(&tx_script_package));

    let mut p2id_test =
        CompilerTest::rust_source_cargo_miden("../../examples/p2id-note", config.clone(), []);
    let note_package = p2id_test.compile_package();
    assert!(note_package.is_library(), "expected library");
    expect!["53082"].assert_eq(stripped_mast_size_str(&note_package));

    let mut p2ide_test =
        CompilerTest::rust_source_cargo_miden("../../examples/p2ide-note", config, []);
    let p2ide_package = p2ide_test.compile_package();
    assert!(p2ide_package.is_library(), "expected library");
    expect!["62672"].assert_eq(stripped_mast_size_str(&p2ide_package));
}
