//! Foreign procedure invocation tests on a mock chain.

use std::{path::Path, sync::Arc};

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{
    account::{AccountBuilder, AccountStorageMode, AccountType, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};
use midenc_integration_test_support::{compiler_test::sdk_crate_path, project};

use super::support::{
    COUNTER_CONTRACT_STORAGE_KEY, assert_counter_storage, compile_rust_package,
    counter_storage_slot_name, execute_tx, note_script_root, to_core_felts,
};

/// Deploys a counter contract and consumes a note which reads it through FPI.
#[test]
pub fn counter_caller_note_reads_counter_through_fpi() {
    let (counter_package, caller_note_package) = build_fpi_test_packages();

    let counter_storage_slot = counter_storage_slot_name();
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, 42_u64)
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

    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        42,
    );

    let foreign_account_inputs = chain.get_foreign_account_inputs(counter_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        42,
    );
}

/// Builds isolated counter contract and caller note projects for the FPI test.
fn build_fpi_test_packages() -> (Arc<Package>, Arc<Package>) {
    let counter_project = project("fpi-counter-contract")
        .file("Cargo.toml", &counter_contract_cargo_toml())
        .file("src/lib.rs", COUNTER_CONTRACT_SOURCE)
        .build();
    let counter_package = compile_rust_package(counter_project.root(), true);

    let caller_project = project("fpi-counter-caller")
        .file("Cargo.toml", &counter_caller_cargo_toml(counter_project.root().as_path()))
        .file("src/lib.rs", COUNTER_CALLER_SOURCE)
        .build();
    let caller_note_package = compile_rust_package(caller_project.root(), true);

    (counter_package, caller_note_package)
}

/// Returns the generated counter contract manifest used by the FPI test.
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

/// Returns the generated counter caller note manifest used by the FPI test.
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

/// Minimal counter account component source used by the FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, felt, Felt, StorageMap, Word};

/// Account component whose storage map holds one counter value.
#[component]
struct CounterContract {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the current counter value.
    pub fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }
}
"#;

/// Minimal note script source which reads the generated counter account through FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::CounterContract;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that the foreign counter account stores the initialized value.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let count = count_acc.get_count();
        assert_eq(count, felt!(42));
    }
}
"#;
