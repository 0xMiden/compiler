//! Regression coverage for canonical dependency interfaces imported both plainly and through FPI.

use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{CompilerTestBuilder, project};

use super::super::support::{
    account_cargo_toml_for, account_miden_project_toml_with_interface, compile_rust_package,
    note_cargo_toml_for_dependency, note_miden_project_toml_for_dependency,
};

/// Compiles a note whose plain dependency import and FPI bindings describe the same interface.
///
/// The note macro and `#[account]` both import the account dependency into their worlds. Before
/// the regression fix, `#[account]` also added generated `fpi-*` functions to that canonical
/// interface. The `template-test` package name gives the two incompatible component-type sections
/// the uncommon linker order which exposes the mismatch.
#[test]
fn plain_dependency_import_and_fpi_bindings_are_compatible() {
    const ACCOUNT_NAME: &str = "basic-wallet";
    const ACCOUNT_PACKAGE: &str = "miden:basic-wallet";
    const NOTE_NAME: &str = "template-test";
    const NOTE_PACKAGE: &str = "miden:template-test";

    let account_project = project("plain-import-and-fpi-basic-wallet")
        .file(
            "miden-project.toml",
            &use_regression_version(account_miden_project_toml_with_interface(
                ACCOUNT_NAME,
                ACCOUNT_PACKAGE,
                "basic-wallet",
            )),
        )
        .file(
            "Cargo.toml",
            &use_regression_version(account_cargo_toml_for(ACCOUNT_NAME, ACCOUNT_PACKAGE)),
        )
        .file("src/lib.rs", BASIC_WALLET_SOURCE)
        .build();
    let _account_package = compile_rust_package(account_project.root(), true);

    let account_root = account_project.root();
    let note_project = project(NOTE_NAME)
        .file(
            "miden-project.toml",
            &use_regression_version(note_miden_project_toml_for_dependency(
                NOTE_NAME,
                NOTE_PACKAGE,
                ACCOUNT_PACKAGE,
                &account_root,
            )),
        )
        .file(
            "Cargo.toml",
            &use_regression_version(note_cargo_toml_for_dependency(
                NOTE_NAME,
                NOTE_PACKAGE,
                ACCOUNT_PACKAGE,
                &account_root,
            )),
        )
        .file("src/lib.rs", TEMPLATE_TEST_NOTE_SOURCE)
        .build();

    let mut note_builder = CompilerTestBuilder::rust_source_cargo_miden(
        note_project.root(),
        WasmTranslationConfig::default(),
        [],
    );
    note_builder.with_release(false);
    let mut note_test = note_builder.build();
    let _note_package = note_test.compile_package();
}

/// Updates generated fixture manifests to the package version which exposes the linker ordering.
fn use_regression_version(manifest: String) -> String {
    manifest.replace("0.0.1", "0.1.0")
}

/// Account component source used to produce the canonical basic-wallet interface.
const BASIC_WALLET_SOURCE: &str = include_str!("../../../../../examples/basic-wallet/src/lib.rs");

/// Note source containing plain and FPI views of the same basic-wallet dependency.
const TEMPLATE_TEST_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AccountId, Word, account, active_note, note};

/// Native or foreign basic-wallet account bindings.
#[account(basic_wallet::BasicWallet)]
struct Wallet;

/// Note which receives its assets into the requested account.
#[note]
struct BlindImportNote {
    /// Account allowed to consume this note.
    target_account_id: AccountId,
}

#[note]
impl BlindImportNote {
    /// Checks the consumer and transfers every note asset through the wallet binding.
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        assert_eq!(account.get_id(), self.target_account_id);

        let assets = active_note::get_assets();
        for asset in assets {
            account.receive_asset(asset);
        }
    }
}
"#;
