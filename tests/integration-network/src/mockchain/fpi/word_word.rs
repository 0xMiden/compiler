//! Foreign procedure invocation tests for methods that accept and return a `Word`.

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

use super::{
    super::support::{counter_storage_slot_name, execute_tx, note_script_root, to_core_felts},
    common::build_fpi_test_packages,
};

/// Deploys a counter contract and consumes a note which reads it through `Word -> Word` FPI.
#[test]
pub fn counter_caller_note_reads_word_through_fpi_word_arg() {
    let (counter_package, caller_note_package) =
        build_fpi_test_packages("fpi-word-word", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);

    execute_word_word_counter_caller_note(
        counter_package,
        caller_note_package,
        word_word_storage_key(),
        expected_count_word(),
    );
}

/// Deploys a `Word`-valued counter contract and consumes the caller note.
fn execute_word_word_counter_caller_note(
    counter_package: Arc<Package>,
    caller_note_package: Arc<Package>,
    counter_storage_key: Word,
    expected_count: Word,
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

    assert_counter_storage_word_at_key(
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

    assert_counter_storage_word_at_key(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key,
        expected_count,
    );
}

/// Returns the non-zero storage key used by the `Word -> Word` FPI test.
fn word_word_storage_key() -> Word {
    Word::new([Felt::new(101), Felt::new(202), Felt::new(303), Felt::new(404)])
}

/// Returns the expected `Word` value used by the `Word -> Word` FPI test.
fn expected_count_word() -> Word {
    Word::new([Felt::new(987), Felt::new(654), Felt::new(321), Felt::new(111)])
}

/// Asserts the stored `Word` value under `storage_key`.
fn assert_counter_storage_word_at_key(
    counter_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    storage_key: Word,
    expected: Word,
) {
    let word = counter_account_storage
        .get_map_item(storage_slot, storage_key)
        .expect("Failed to get counter value from storage slot");

    assert_eq!(word, expected, "Counter word value mismatch");
}

/// Minimal counter account component source used by the `Word -> Word` FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, StorageMap, Word};

/// Account component whose storage map holds one counter word.
#[component]
struct CounterContract {
    /// Storage map holding the counter word.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Word>,
}

#[component]
impl CounterContract {
    /// Returns the counter word stored under `key`.
    pub fn get_count_by_key(&self, key: Word) -> Word {
        self.count_map.get(key)
    }
}
"#;

/// Minimal note script source which reads the generated counter account through `Word -> Word` FPI.
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
    /// Checks that a `Word` argument and return value cross the FPI boundary.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let key = Word::new([felt!(101), felt!(202), felt!(303), felt!(404)]);
        let expected = Word::new([felt!(987), felt!(654), felt!(321), felt!(111)]);
        let count = count_acc.get_count_by_key(key);
        assert_word_eq(count, expected);
    }
}

/// Asserts that two words contain the same field elements.
fn assert_word_eq(actual: Word, expected: Word) {
    assert_eq(actual[0], expected[0]);
    assert_eq(actual[1], expected[1]);
    assert_eq(actual[2], expected[2]);
    assert_eq(actual[3], expected[3]);
}
"#;
