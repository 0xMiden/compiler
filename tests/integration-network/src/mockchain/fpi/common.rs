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
    account::{
        AccountBuilder, AccountStorage, AccountStorageMode, AccountType, StorageSlotName,
        auth::AuthScheme,
    },
    crypto::rand::RandomCoin,
};
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};
use midenc_integration_test_support::{compiler_test::sdk_crate_path, project};

use super::super::support::{compile_rust_package, execute_tx, note_script_root, to_core_felts};

/// Builds isolated account and note projects for an FPI test case.
pub(super) fn build_fpi_test_packages(
    test_name: &str,
    counter_source: &str,
    caller_source: &str,
) -> (Arc<Package>, Arc<Package>, StorageSlotName) {
    let names = FpiTestProjectNames::new(test_name);
    let counter_storage_slot = counter_storage_slot_name_for_package(&names.account_package);

    let account_project = project(&names.account_name)
        .file("Cargo.toml", &account_cargo_toml(&names))
        .file("src/lib.rs", counter_source)
        .build();
    let counter_package = compile_rust_package(account_project.root(), true);

    let note_project = project(&names.note_name)
        .file("Cargo.toml", &note_cargo_toml(&names, account_project.root().as_path()))
        .file("src/lib.rs", caller_source)
        .build();
    let caller_note_package = compile_rust_package(note_project.root(), true);

    (counter_package, caller_note_package, counter_storage_slot)
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
            "Cargo.toml",
            &account_cargo_toml_for(&names.callee_account_name, &names.callee_account_package),
        )
        .file("src/lib.rs", callee_source)
        .build();
    let callee_package = compile_rust_package(callee_project.root(), true);

    let caller_project = project(&names.caller_account_name)
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
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(counter_component)
        .build_existing()
        .expect("failed to build counter account");
    builder
        .add_account(counter_account.clone())
        .expect("failed to add counter account to mock chain builder");

    let caller_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
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

    let foreign_account_inputs = chain.get_foreign_account_inputs(counter_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage_at_key(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key,
        expected_count,
    );
}

/// Returns the derived storage slot name for the generated counter account package.
fn counter_storage_slot_name_for_package(account_package: &str) -> StorageSlotName {
    let namespace = sanitize_slot_name_component(account_package);
    StorageSlotName::new(format!("{namespace}::counter_contract::count_map"))
        .expect("generated FPI counter storage slot name must be valid")
}

/// Normalizes a generated component package into its storage slot namespace segment.
fn sanitize_slot_name_component(component: &str) -> String {
    let component = component.split('@').next().unwrap_or(component);
    let mut out: String = component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if out.is_empty() {
        out.push('x');
    }
    if out.starts_with('_') {
        out.insert(0, 'x');
    }

    out
}

/// Returns the generated account manifest used by an FPI test.
fn account_cargo_toml(names: &FpiTestProjectNames) -> String {
    account_cargo_toml_for(&names.account_name, &names.account_package)
}

/// Returns the generated account manifest for a package without FPI dependencies.
fn account_cargo_toml_for(account_name: &str, account_package: &str) -> String {
    let sdk_path = sdk_crate_path();
    format!(
        r#"
[package]
name = "{account_name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{account_package}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]

[profile.release]
opt-level = "z"
panic = "abort"
debug = false

[profile.dev]
panic = "abort"
opt-level = 1
debug-assertions = true
overflow-checks = false
debug = false
"#,
        sdk_path = sdk_path.display(),
        account_name = account_name,
        account_package = account_package,
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

/// Returns the generated caller note manifest used by an FPI test.
fn note_cargo_toml(names: &FpiTestProjectNames, account_project_root: &Path) -> String {
    note_cargo_toml_for_dependency(
        &names.note_name,
        &names.note_package,
        &names.account_package,
        account_project_root,
    )
}

/// Returns the generated note manifest with one Miden dependency.
fn note_cargo_toml_for_dependency(
    note_name: &str,
    note_package: &str,
    dependency_package: &str,
    dependency_root: &Path,
) -> String {
    let sdk_path = sdk_crate_path();
    let dependency_wit_path = dependency_root.join("target/generated-wit");

    format!(
        r#"
[package]
name = "{note_name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.miden]
project-kind = "note-script"

[package.metadata.component]
package = "{note_package}"

[package.metadata.miden.dependencies]
"{dependency_package}" = {{ path = "{dependency_root}" }}

[package.metadata.component.target.dependencies]
"{dependency_package}" = {{ path = "{dependency_wit_path}" }}

[profile.release]
opt-level = "z"
panic = "abort"
debug = false

[profile.dev]
panic = "abort"
opt-level = 1
debug-assertions = true
overflow-checks = false
debug = false
"#,
        sdk_path = sdk_path.display(),
        dependency_package = dependency_package,
        note_name = note_name,
        note_package = note_package,
        dependency_root = dependency_root.display(),
        dependency_wit_path = dependency_wit_path.display(),
    )
}

/// Asserts the counter value stored in the counter contract's storage map at `storage_key`.
fn assert_counter_storage_at_key(
    counter_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    storage_key: Word,
    expected: u64,
) {
    let word = counter_account_storage
        .get_map_item(storage_slot, storage_key)
        .expect("Failed to get counter value from storage slot");

    // `AccountStorage` exposes scalar felt values as `[felt, 0, 0, 0]`.
    let val = word[0];
    assert_eq!(
        val.as_canonical_u64(),
        expected,
        "Counter value mismatch. Expected: {}, Got: {}",
        expected,
        val.as_canonical_u64()
    );
}
