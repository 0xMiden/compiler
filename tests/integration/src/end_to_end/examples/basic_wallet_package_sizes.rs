use std::path::Path;

use midenc_expect_test::expect;
use midenc_integration_test_support::compile_project;

use crate::testing::stripped_mast_size_str;

#[test]
fn basic_wallet_and_p2id() {
    let account_package = compile_project(Path::new("../../examples/basic-wallet"));
    assert!(account_package.is_library(), "expected library");
    expect!["8505"].assert_eq(stripped_mast_size_str(&account_package).as_str());

    let tx_script_package = compile_project(Path::new("../../examples/basic-wallet-tx-script"));
    assert!(tx_script_package.is_library(), "expected library");
    expect!["13784"].assert_eq(stripped_mast_size_str(&tx_script_package).as_str());

    let note_package = compile_project(Path::new("../../examples/p2id-note"));
    assert!(note_package.is_library(), "expected library");
    expect!["21797"].assert_eq(stripped_mast_size_str(&note_package).as_str());
    // The note package exports both the note script and the `build-recipient` constructor; the
    // constructor must not interfere with the `@note_script`-attributed export selection.
    assert!(
        note_package.manifest.exports().any(|export| export.name() == "build-recipient"),
        "expected the p2id note package to export the `build-recipient` constructor"
    );
    miden_protocol::note::NoteScript::from_package(&note_package)
        .expect("expected the p2id note package to contain exactly one note script export");

    let p2ide_package = compile_project(Path::new("../../examples/p2ide-note"));
    assert!(p2ide_package.is_library(), "expected library");
    expect!["16436"].assert_eq(stripped_mast_size_str(&p2ide_package).as_str());
}
