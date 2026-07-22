//! Regression coverage for canonical dependency interfaces imported both plainly and through FPI.

use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{CompilerTestBuilder, Project, project};

use super::super::support::{
    account_cargo_toml_for, account_miden_project_toml_with_interface, compile_rust_package,
    note_cargo_toml_for_dependencies, note_cargo_toml_for_dependency,
    note_miden_project_toml_for_dependencies, note_miden_project_toml_for_dependency,
};

/// Cargo package name of the dependency shared by the regression fixtures.
const ACCOUNT_NAME: &str = "basic-wallet";
/// WIT package name of the dependency shared by the regression fixtures.
const ACCOUNT_PACKAGE: &str = "miden:basic-wallet";

/// Compiles a note whose plain dependency import and FPI bindings describe the same interface.
///
/// The note macro and `#[account]` both import the account dependency into their worlds. Before
/// the regression fix, `#[account]` also added generated `fpi-*` functions to that canonical
/// interface. The `template-test` package name gives the two incompatible component-type sections
/// the uncommon linker order which exposes the mismatch.
#[test]
fn plain_dependency_import_and_fpi_bindings_are_compatible() {
    const NOTE_NAME: &str = "template-test";
    const NOTE_PACKAGE: &str = "miden:template-test";

    let account_project = build_basic_wallet_project("plain-import-and-fpi-basic-wallet");
    let note_project =
        build_note_project(NOTE_NAME, NOTE_PACKAGE, &account_project, TEMPLATE_TEST_NOTE_SOURCE);
    compile_debug_note(&note_project);
}

/// Compiles same-named account wrappers in distinct Rust modules.
///
/// Each wrapper emits a complete component-metadata payload for the same binding world. Their
/// section names must remain distinct without incorporating source file names or positions.
#[test]
fn same_named_account_wrappers_have_distinct_stable_metadata_sections() {
    const NOTE_NAME: &str = "repeated-account-bindings";
    const NOTE_PACKAGE: &str = "miden:repeated-account-bindings";

    let account_project = build_basic_wallet_project("repeated-bindings-basic-wallet");
    let note_project = build_note_project(
        NOTE_NAME,
        NOTE_PACKAGE,
        &account_project,
        REPEATED_BINDINGS_NOTE_SOURCE,
    );
    compile_debug_note(&note_project);
}

/// Compiles account wrappers whose binding worlds import different dependencies.
#[test]
fn different_account_import_sets_have_distinct_metadata_worlds() {
    const FIRST_ACCOUNT_NAME: &str = "first-wallet";
    const FIRST_ACCOUNT_PACKAGE: &str = "miden:first-wallet";
    const SECOND_ACCOUNT_NAME: &str = "second-wallet";
    const SECOND_ACCOUNT_PACKAGE: &str = "miden:second-wallet";
    const NOTE_NAME: &str = "different-account-bindings";
    const NOTE_PACKAGE: &str = "miden:different-account-bindings";

    let first_account = build_wallet_project(
        "different-bindings-first-wallet",
        FIRST_ACCOUNT_NAME,
        FIRST_ACCOUNT_PACKAGE,
    );
    let second_account = build_wallet_project(
        "different-bindings-second-wallet",
        SECOND_ACCOUNT_NAME,
        SECOND_ACCOUNT_PACKAGE,
    );
    let first_root = first_account.root();
    let second_root = second_account.root();
    let dependencies = [
        (FIRST_ACCOUNT_PACKAGE, first_root.as_path()),
        (SECOND_ACCOUNT_PACKAGE, second_root.as_path()),
    ];
    let note_project = project(NOTE_NAME)
        .file(
            "miden-project.toml",
            &use_regression_version(note_miden_project_toml_for_dependencies(
                NOTE_NAME,
                NOTE_PACKAGE,
                &dependencies,
            )),
        )
        .file(
            "Cargo.toml",
            &use_regression_version(note_cargo_toml_for_dependencies(NOTE_NAME, &dependencies)),
        )
        .file("src/lib.rs", DIFFERENT_BINDINGS_NOTE_SOURCE)
        .build();

    compile_debug_note(&note_project);
}

/// Builds the basic-wallet dependency used by one regression fixture.
fn build_basic_wallet_project(folder_name: &str) -> Project {
    build_wallet_project(folder_name, ACCOUNT_NAME, ACCOUNT_PACKAGE)
}

/// Builds one wallet dependency used by the metadata-world regression fixtures.
fn build_wallet_project(folder_name: &str, account_name: &str, account_package: &str) -> Project {
    let project = project(folder_name)
        .file(
            "miden-project.toml",
            &use_regression_version(account_miden_project_toml_with_interface(
                account_name,
                account_package,
                "basic-wallet",
            )),
        )
        .file(
            "Cargo.toml",
            &use_regression_version(account_cargo_toml_for(account_name, account_package)),
        )
        .file("src/lib.rs", BASIC_WALLET_SOURCE)
        .build();
    let _account_package = compile_rust_package(project.root(), true);
    project
}

/// Builds a note project with the supplied basic-wallet dependency and source.
fn build_note_project(
    note_name: &str,
    note_package: &str,
    account_project: &Project,
    source: &str,
) -> Project {
    let account_root = account_project.root();
    project(note_name)
        .file(
            "miden-project.toml",
            &use_regression_version(note_miden_project_toml_for_dependency(
                note_name,
                note_package,
                ACCOUNT_PACKAGE,
                &account_root,
            )),
        )
        .file(
            "Cargo.toml",
            &use_regression_version(note_cargo_toml_for_dependency(
                note_name,
                ACCOUNT_PACKAGE,
                &account_root,
            )),
        )
        .file("src/lib.rs", source)
        .build()
}

/// Compiles one generated note project in the debug profile used by the original failure.
fn compile_debug_note(note_project: &Project) {
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(
        note_project.root(),
        WasmTranslationConfig::default(),
        [],
    );
    builder.with_release(false);
    let mut test = builder.build();
    let _note_package = test.compile_package();
}

/// Updates generated fixture manifests to the package version which exposes the linker ordering.
fn use_regression_version(manifest: String) -> String {
    const ORIGINAL_PACKAGE_VERSION: &str = "version = \"0.0.1\"";
    const REPLACEMENT_PACKAGE_VERSION: &str = "version = \"0.1.0\"";
    const ORIGINAL_NAMESPACE_VERSION: &str = "@0.0.1\"";
    const REPLACEMENT_NAMESPACE_VERSION: &str = "@0.1.0\"";

    assert_eq!(
        manifest.matches(ORIGINAL_PACKAGE_VERSION).count(),
        1,
        "fixture manifest must contain exactly one default package version"
    );
    assert!(
        manifest.matches(ORIGINAL_NAMESPACE_VERSION).count() <= 1,
        "fixture manifest must contain at most one component namespace version"
    );

    manifest
        .replacen(ORIGINAL_PACKAGE_VERSION, REPLACEMENT_PACKAGE_VERSION, 1)
        .replacen(ORIGINAL_NAMESPACE_VERSION, REPLACEMENT_NAMESPACE_VERSION, 1)
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

/// Note source containing same-named account wrappers under distinct semantic module paths.
const REPEATED_BINDINGS_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AccountId, Word, note};

/// First semantic owner of a wrapper named `Wallet`.
mod first {
    use miden::account;

    /// First basic-wallet binding.
    #[account(basic_wallet::BasicWallet as FirstBasicWallet)]
    pub struct Wallet;
}

/// Second semantic owner of a wrapper with the same Rust item name.
mod second {
    use miden::account;

    /// Second basic-wallet binding.
    #[account(basic_wallet::BasicWallet as SecondBasicWallet)]
    pub struct Wallet;
}

/// Note carrying the account used to construct both foreign bindings.
#[note]
struct RepeatedBindingsNote {
    /// Foreign account selected by both wrappers.
    foreign_account_id: AccountId,
}

#[note]
impl RepeatedBindingsNote {
    /// Constructs both wrappers so their generated bindings remain part of the linked artifact.
    #[note_script]
    pub fn script(self, _arg: Word) {
        let _first = first::Wallet::new(self.foreign_account_id);
        let _second = second::Wallet::new(self.foreign_account_id);
    }
}
"#;

/// Note source containing wrappers whose binding worlds have different canonical imports.
const DIFFERENT_BINDINGS_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AccountId, Word, account, note};

/// Binding to the first wallet dependency.
#[account(first_wallet::BasicWallet as FirstBasicWallet)]
struct FirstWallet;

/// Binding to the second wallet dependency.
#[account(second_wallet::BasicWallet as SecondBasicWallet)]
struct SecondWallet;

/// Note carrying the accounts used to retain both foreign bindings.
#[note]
struct DifferentBindingsNote {
    /// Account selected for the first binding.
    first_account_id: AccountId,
    /// Account selected for the second binding.
    second_account_id: AccountId,
}

#[note]
impl DifferentBindingsNote {
    /// Constructs both wrappers so both metadata worlds reach the final Wasm module.
    #[note_script]
    pub fn script(self, _arg: Word) {
        let _first = FirstWallet::new(self.first_account_id);
        let _second = SecondWallet::new(self.second_account_id);
    }
}
"#;
