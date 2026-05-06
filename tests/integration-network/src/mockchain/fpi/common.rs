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

use super::super::support::{
    compile_rust_package, counter_storage_slot_name, execute_tx, note_script_root, to_core_felts,
};

/// Builds isolated counter contract and caller note projects for an FPI test case.
pub(super) fn build_fpi_test_packages(
    project_name: &str,
    counter_source: &str,
    caller_source: &str,
) -> (Arc<Package>, Arc<Package>) {
    let counter_project = project(&format!("{project_name}-counter-contract"))
        .file("Cargo.toml", &counter_contract_cargo_toml())
        .file("src/lib.rs", counter_source)
        .build();
    let counter_package = compile_rust_package(counter_project.root(), true);

    let caller_project = project(&format!("{project_name}-counter-caller"))
        .file("Cargo.toml", &counter_caller_cargo_toml(counter_project.root().as_path()))
        .file("src/lib.rs", caller_source)
        .build();
    let caller_note_package = compile_rust_package(caller_project.root(), true);

    (counter_package, caller_note_package)
}

/// Deploys a counter contract and consumes a caller note that invokes it through FPI.
pub(super) fn execute_counter_caller_note(
    counter_package: Arc<Package>,
    caller_note_package: Arc<Package>,
    counter_storage_key: Word,
    expected_count: u64,
) {
    let counter_storage_slot = counter_storage_slot_name();
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

/// Returns the generated counter contract manifest used by FPI tests.
fn counter_contract_cargo_toml() -> String {
    let sdk_path = sdk_crate_path();
    format!(
        r#"
[package]
name = "counter-contract"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "miden:counter-contract"

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
    )
}

/// Returns the generated counter caller note manifest used by FPI tests.
fn counter_caller_cargo_toml(counter_project_root: &Path) -> String {
    let sdk_path = sdk_crate_path();
    let counter_wit_path = counter_project_root.join("target/generated-wit");
    format!(
        r#"
[package]
name = "fpi-counter-caller"
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
package = "miden:counter-caller"

[package.metadata.miden.dependencies]
"miden:counter-contract" = {{ path = "{counter_project_root}" }}

[package.metadata.component.target.dependencies]
"miden:counter-account" = {{ path = "{counter_wit_path}" }}

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
        counter_project_root = counter_project_root.display(),
        counter_wit_path = counter_wit_path.display(),
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
