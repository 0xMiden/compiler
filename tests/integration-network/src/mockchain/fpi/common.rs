//! Shared fixtures for foreign procedure invocation mock-chain tests.

use std::{path::Path, sync::Arc};

use miden_client::{
    Word,
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{
    account::{AccountBuilder, AccountType, StorageSlotName, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};
use midenc_integration_test_support::project;

use super::super::support::*;

/// Builds isolated account and note projects for an FPI test case.
pub(super) fn build_fpi_test_packages(
    test_name: &str,
    counter_source: &str,
    caller_source: &str,
) -> (Arc<Package>, Arc<Package>, StorageSlotName) {
    let names = FpiTestProjectNames::new(test_name);
    let counter_storage_slot = counter_storage_slot_name_for_package(&names.account_package);

    let account_project = project(&names.account_name)
        .file("miden-project.toml", &account_miden_project_toml(&names))
        .file("Cargo.toml", &account_cargo_toml(&names))
        .file("src/lib.rs", counter_source)
        .build();
    let counter_package = compile_rust_package(account_project.root(), true);

    let note_project = project(&names.note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml(&names, account_project.root().as_path()),
        )
        .file("Cargo.toml", &note_cargo_toml(&names, account_project.root().as_path()))
        .file("src/lib.rs", caller_source)
        .build();
    let caller_note_package = compile_rust_package(note_project.root(), true);

    (counter_package, caller_note_package, counter_storage_slot)
}

/// Builds two isolated account projects and one note project for an FPI test case.
pub(super) fn build_multi_package_fpi_test_packages(
    test_name: &str,
    first_account_source: &str,
    second_account_source: &str,
    caller_source: &str,
) -> (Arc<Package>, Arc<Package>, Arc<Package>, StorageSlotName, StorageSlotName) {
    let names = FpiMultiPackageProjectNames::new(test_name);
    // The two components export distinct WIT interfaces (`first-counter`, `second-counter`) so the
    // `#[account(...)]` macro generates two differently named traits. Storage slot names derive
    // from the interface segment, so they track those interfaces too.
    let first_storage_slot =
        storage_slot_name_for_package(&names.first_account_package, "first_counter");
    let second_storage_slot =
        storage_slot_name_for_package(&names.second_account_package, "second_counter");

    let first_account_project = project(&names.first_account_name)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(
                &names.first_account_name,
                &names.first_account_package,
                "first-counter",
            ),
        )
        .file(
            "Cargo.toml",
            &account_cargo_toml_for(&names.first_account_name, &names.first_account_package),
        )
        .file("src/lib.rs", first_account_source)
        .build();
    let first_account_package = compile_rust_package(first_account_project.root(), true);

    let second_account_project = project(&names.second_account_name)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(
                &names.second_account_name,
                &names.second_account_package,
                "second-counter",
            ),
        )
        .file(
            "Cargo.toml",
            &account_cargo_toml_for(&names.second_account_name, &names.second_account_package),
        )
        .file("src/lib.rs", second_account_source)
        .build();
    let second_account_package = compile_rust_package(second_account_project.root(), true);

    let first_account_root = first_account_project.root();
    let second_account_root = second_account_project.root();
    let dependencies = [
        (names.first_account_package.as_str(), first_account_root.as_path()),
        (names.second_account_package.as_str(), second_account_root.as_path()),
    ];
    let note_project = project(&names.note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependencies(
                &names.note_name,
                &names.note_package,
                &dependencies,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependencies(&names.note_name, &names.note_package, &dependencies),
        )
        .file("src/lib.rs", caller_source)
        .build();
    let caller_note_package = compile_rust_package(note_project.root(), true);

    (
        first_account_package,
        second_account_package,
        caller_note_package,
        first_storage_slot,
        second_storage_slot,
    )
}

/// Builds isolated callee account, caller account, and note projects for an account-to-account FPI
/// test case.
pub(super) fn build_account_to_account_fpi_test_packages(
    test_name: &str,
    callee_source: &str,
    caller_source: &str,
    note_source: &str,
) -> (Arc<Package>, Arc<Package>, Arc<Package>, StorageSlotName) {
    let names = FpiAccountToAccountProjectNames::new(test_name);
    let callee_storage_slot = counter_storage_slot_name_for_package(&names.callee_account_package);

    let callee_project = project(&names.callee_account_name)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_for(
                &names.callee_account_name,
                &names.callee_account_package,
            ),
        )
        .file(
            "Cargo.toml",
            &account_cargo_toml_for(&names.callee_account_name, &names.callee_account_package),
        )
        .file("src/lib.rs", callee_source)
        .build();
    let callee_package = compile_rust_package(callee_project.root(), true);

    let caller_project = project(&names.caller_account_name)
        .file(
            "miden-project.toml",
            &dependent_account_miden_project_toml(
                &names.caller_account_name,
                &names.caller_account_package,
                &names.callee_account_package,
                callee_project.root().as_path(),
            ),
        )
        .file(
            "Cargo.toml",
            &dependent_account_cargo_toml(
                &names.caller_account_name,
                &names.caller_account_package,
                &names.callee_account_package,
                callee_project.root().as_path(),
            ),
        )
        .file("src/lib.rs", caller_source)
        .build();
    let caller_package = compile_rust_package(caller_project.root(), true);

    let note_project = project(&names.note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                &names.note_name,
                &names.note_package,
                &names.caller_account_package,
                caller_project.root().as_path(),
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(
                &names.note_name,
                &names.note_package,
                &names.caller_account_package,
                caller_project.root().as_path(),
            ),
        )
        .file("src/lib.rs", note_source)
        .build();
    let note_package = compile_rust_package(note_project.root(), true);

    (callee_package, caller_package, note_package, callee_storage_slot)
}

/// Names derived from an FPI test function for the generated account and note projects.
struct FpiTestProjectNames {
    account_name: String,
    note_name: String,
    account_package: String,
    note_package: String,
}

impl FpiTestProjectNames {
    /// Builds Cargo crate names, WIT package names, and project paths from `test_name`.
    fn new(test_name: &str) -> Self {
        let name = test_name.replace('_', "-");
        let account_name = format!("{name}-account");
        let note_name = format!("{name}-note");
        let account_package = format!("miden:{account_name}");
        let note_package = format!("miden:{note_name}");

        Self {
            account_name,
            note_name,
            account_package,
            note_package,
        }
    }
}

/// Names derived from a test function for two generated account projects and one note project.
struct FpiMultiPackageProjectNames {
    first_account_name: String,
    second_account_name: String,
    note_name: String,
    first_account_package: String,
    second_account_package: String,
    note_package: String,
}

impl FpiMultiPackageProjectNames {
    /// Builds Cargo crate names, WIT package names, and project paths from `test_name`.
    fn new(test_name: &str) -> Self {
        let name = test_name.replace('_', "-");
        let first_account_name = format!("{name}-first-account");
        let second_account_name = format!("{name}-second-account");
        let note_name = format!("{name}-note");
        let first_account_package = format!("miden:{first_account_name}");
        let second_account_package = format!("miden:{second_account_name}");
        let note_package = format!("miden:{note_name}");

        Self {
            first_account_name,
            second_account_name,
            note_name,
            first_account_package,
            second_account_package,
            note_package,
        }
    }
}

/// Names derived from an FPI account-to-account test function.
struct FpiAccountToAccountProjectNames {
    callee_account_name: String,
    caller_account_name: String,
    note_name: String,
    callee_account_package: String,
    caller_account_package: String,
    note_package: String,
}

impl FpiAccountToAccountProjectNames {
    /// Builds Cargo crate names, WIT package names, and project paths from `test_name`.
    fn new(test_name: &str) -> Self {
        let name = test_name.replace('_', "-");
        let callee_account_name = format!("{name}-callee-account");
        let caller_account_name = format!("{name}-caller-account");
        let note_name = format!("{name}-note");
        let callee_account_package = format!("miden:{callee_account_name}");
        let caller_account_package = format!("miden:{caller_account_name}");
        let note_package = format!("miden:{note_name}");

        Self {
            callee_account_name,
            caller_account_name,
            note_name,
            callee_account_package,
            caller_account_package,
            note_package,
        }
    }
}

/// Deploys a counter contract and consumes a caller note that invokes it through FPI.
pub(super) fn execute_counter_caller_note(
    counter_package: Arc<Package>,
    caller_note_package: Arc<Package>,
    counter_storage_slot: StorageSlotName,
    counter_storage_key: Word,
    expected_count: u64,
) {
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), counter_storage_key, expected_count)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };

    let mut builder = MockChain::builder();
    let counter_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(counter_component)
        .build_existing()
        .expect("failed to build counter account");
    builder
        .add_account(counter_account.clone())
        .expect("failed to add counter account to mock chain builder");

    let caller_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet);
    let caller_account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            caller_builder,
            AccountState::Exists,
        )
        .expect("failed to add caller account to mock chain builder");

    let rng = RandomCoin::new(note_script_root(caller_note_package.as_ref()));
    let caller_note = NoteBuilder::new(caller_account.id(), rng)
        .package((*caller_note_package).clone())
        .note_storage(to_core_felts(&counter_account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(caller_account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(caller_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    assert_counter_storage_at_key(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key,
        expected_count,
    );

    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([chain.get_foreign_account_inputs(counter_account.id()).unwrap()]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage_at_key(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key,
        expected_count,
    );
}

/// Returns the generated account project manifest used by an FPI test.
fn account_miden_project_toml(names: &FpiTestProjectNames) -> String {
    account_miden_project_toml_for(&names.account_name, &names.account_package)
}

/// Returns the generated account project manifest for a package without FPI dependencies.
fn account_miden_project_toml_for(account_name: &str, account_package: &str) -> String {
    account_miden_project_toml_with_interface(account_name, account_package, "counter-contract")
}

/// Returns the generated account manifest used by an FPI test.
fn account_cargo_toml(names: &FpiTestProjectNames) -> String {
    account_cargo_toml_for(&names.account_name, &names.account_package)
}

/// Returns the generated account project manifest for a package with one FPI account dependency.
fn dependent_account_miden_project_toml(
    account_name: &str,
    account_package: &str,
    dependency_package: &str,
    dependency_root: &Path,
) -> String {
    let namespace = account_component_namespace(account_package, "caller-account");
    let dependency_name = miden_dependency_name(dependency_package);
    let dependency_wit_path = dependency_root.join("target/generated-wit");
    format!(
        r#"
[package]
name = "{account_name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"{dependency_name}" = {{ path = "{dependency_root}" }}

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]

[package.metadata.miden.dependencies]
"{dependency_name}" = {{ wit = "{dependency_wit_path}" }}
"#,
        dependency_root = dependency_root.display(),
        dependency_wit_path = dependency_wit_path.display(),
    )
}

/// Returns the generated account manifest for a package with one FPI account dependency.
fn dependent_account_cargo_toml(
    account_name: &str,
    account_package: &str,
    dependency_package: &str,
    dependency_root: &Path,
) -> String {
    let mut manifest = account_cargo_toml_for(account_name, account_package);
    let dependency_wit_path = dependency_root.join("target/generated-wit");
    manifest.push_str(&format!(
        r#"
[package.metadata.miden.dependencies]
"{dependency_package}" = {{ path = "{dependency_root}" }}

[package.metadata.component.target.dependencies]
"{dependency_package}" = {{ path = "{dependency_wit_path}" }}
"#,
        dependency_package = dependency_package,
        dependency_root = dependency_root.display(),
        dependency_wit_path = dependency_wit_path.display(),
    ));
    manifest
}

/// Returns the generated caller note project manifest used by an FPI test.
fn note_miden_project_toml(names: &FpiTestProjectNames, account_project_root: &Path) -> String {
    note_miden_project_toml_for_dependency(
        &names.note_name,
        &names.note_package,
        &names.account_package,
        account_project_root,
    )
}

/// Returns the generated caller note manifest used by an FPI test.
fn note_cargo_toml(names: &FpiTestProjectNames, account_project_root: &Path) -> String {
    note_cargo_toml_for_dependency(
        &names.note_name,
        &names.note_package,
        &names.account_package,
        account_project_root,
    )
}

/// First counter component shared by the multi-package FPI tests.
///
/// Exports the `first-counter` interface with a `get_count` method returning the value stored under
/// the counter key. Its method name deliberately matches [`SECOND_COUNTER_COMPONENT_SOURCE`] so a
/// wrapper deriving both components must disambiguate the two generated traits with UFCS.
pub(super) const FIRST_COUNTER_COMPONENT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Account component whose storage map holds the first counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "first counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose storage map holds the first counter value.
#[component]
trait FirstCounter {
    /// Returns the first counter value.
    fn get_count(&self) -> Felt;
}

#[component]
impl FirstCounter for CounterContractStorage {
    /// Returns the first counter value.
    fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }
}
"#;

/// Second counter component shared by the multi-package FPI tests.
///
/// Exports the `second-counter` interface; see [`FIRST_COUNTER_COMPONENT_SOURCE`] for the shared
/// `get_count` method name that forces UFCS disambiguation on a wrapper deriving both.
pub(super) const SECOND_COUNTER_COMPONENT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Account component whose storage map holds the second counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "second counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose storage map holds the second counter value.
#[component]
trait SecondCounter {
    /// Returns the second counter value.
    fn get_count(&self) -> Felt;
}

#[component]
impl SecondCounter for CounterContractStorage {
    /// Returns the second counter value.
    fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }
}
"#;
