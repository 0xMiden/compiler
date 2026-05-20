//! Foreign procedure invocation tests for one account calling another account.

use std::sync::Arc;

use miden_client::{
    Word,
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_core::Felt;
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

use super::super::{
    super::support::{execute_tx, note_script_root, to_core_felts},
    common::build_account_to_account_fpi_test_packages,
};

/// Deploys two accounts and consumes a note whose target account calls the second account via FPI.
#[test]
pub fn account_to_account() {
    let (callee_package, caller_package, note_package, callee_storage_slot) =
        build_account_to_account_fpi_test_packages(
            "account_to_account",
            CALLEE_ACCOUNT_SOURCE,
            CALLER_ACCOUNT_SOURCE,
            NOTE_SOURCE,
        );

    execute_account_to_account_note(
        callee_package,
        caller_package,
        note_package,
        callee_storage_slot,
        callee_storage_key(),
        314,
    );
}

/// Deploys a callee account and a caller account, then consumes the forwarding note.
fn execute_account_to_account_note(
    callee_package: Arc<Package>,
    caller_package: Arc<Package>,
    note_package: Arc<Package>,
    callee_storage_slot: StorageSlotName,
    callee_storage_key: Word,
    expected_count: u64,
) {
    let callee_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(callee_storage_slot.clone(), callee_storage_key, expected_count)
            .unwrap();
        AccountComponent::from_package(&callee_package, &init_storage_data).unwrap()
    };
    let caller_component =
        AccountComponent::from_package(&caller_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let callee_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(callee_component)
        .build_existing()
        .expect("failed to build callee account");
    builder
        .add_account(callee_account.clone())
        .expect("failed to add callee account to mock chain builder");

    let caller_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(BasicWallet)
        .with_component(caller_component);
    let caller_account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            caller_builder,
            AccountState::Exists,
        )
        .expect("failed to add caller account to mock chain builder");

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let caller_note = NoteBuilder::new(caller_account.id(), rng)
        .package((*note_package).clone())
        .note_storage(to_core_felts(&callee_account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(caller_account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(caller_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    assert_counter_storage_at_key(
        chain.committed_account(callee_account.id()).unwrap().storage(),
        &callee_storage_slot,
        callee_storage_key,
        expected_count,
    );

    let foreign_account_inputs = chain.get_foreign_account_inputs(callee_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage_at_key(
        chain.committed_account(callee_account.id()).unwrap().storage(),
        &callee_storage_slot,
        callee_storage_key,
        expected_count,
    );
}

/// Returns the non-zero storage key read by the account-to-account FPI call.
fn callee_storage_key() -> Word {
    Word::new([Felt::new(13), Felt::new(21), Felt::new(34), Felt::new(55)])
}

/// Asserts the counter value stored in the callee account's storage map at `storage_key`.
fn assert_counter_storage_at_key(
    callee_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    storage_key: Word,
    expected: u64,
) {
    let word = callee_account_storage
        .get_map_item(storage_slot, storage_key)
        .expect("failed to get counter value from storage slot");

    assert_eq!(word[0].as_canonical_u64(), expected, "counter value mismatch");
}

/// Minimal callee account component source used by the account-to-account FPI test.
const CALLEE_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, Felt, StorageMap, Word};

/// Account component whose storage map holds one counter value.
#[component]
struct CounterContract {
    /// Storage map holding the counter value.
    #[storage(description = "callee account counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the counter value stored under the provided key.
    pub fn get_count(&self, key: Word) -> Felt {
        self.count_map.get(key)
    }
}
"#;

/// Minimal caller account component source used by the account-to-account FPI test.
const CALLER_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, felt, AccountId, Felt, Word};

use crate::bindings::MidenAccountToAccountCalleeAccount as CalleeAccount;

/// Account component which forwards reads to another account through FPI.
#[component]
struct CallerAccount;

#[component]
impl CallerAccount {
    /// Reads a counter value from the provided foreign account.
    pub fn read_foreign_count(&self, callee_account_id: AccountId) -> Felt {
        let callee = CalleeAccount::from_account(callee_account_id);
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        callee.get_count(key)
    }
}
"#;

/// Minimal note script source which invokes the caller account method.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::Account;

/// Note script input containing the foreign callee account id.
#[note]
struct AccountToAccountNote {
    /// Account id of the callee account to invoke through the caller account.
    callee_account_id: AccountId,
}

#[note]
impl AccountToAccountNote {
    /// Checks that the active caller account can read the foreign callee account through FPI.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let count = account.read_foreign_count(self.callee_account_id);
        assert_eq(count, felt!(314));
    }
}
"#;
