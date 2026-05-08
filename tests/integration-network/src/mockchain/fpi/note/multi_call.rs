//! Foreign procedure invocation tests for multiple methods called from one note.

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
    common::build_fpi_test_packages,
};

/// Deploys a counter contract and consumes a note which makes multiple FPI calls.
#[test]
pub fn multi_call() {
    let (counter_package, caller_note_package, counter_storage_slot) =
        build_fpi_test_packages("multi_call", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);

    execute_multi_call_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        [
            (first_storage_key(), expected_first_word()),
            (second_storage_key(), expected_second_word()),
            (third_storage_key(), expected_third_word()),
        ],
    );
}

/// Deploys a `Word`-valued counter contract and consumes the caller note.
fn execute_multi_call_counter_caller_note(
    counter_package: Arc<Package>,
    caller_note_package: Arc<Package>,
    counter_storage_slot: StorageSlotName,
    expected_entries: [(Word, Word); 3],
) {
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        for (storage_key, expected_word) in expected_entries {
            init_storage_data
                .insert_map_entry(counter_storage_slot.clone(), storage_key, expected_word)
                .unwrap();
        }
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

    assert_counter_storage_word_entries(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        expected_entries,
    );

    let foreign_account_inputs = chain.get_foreign_account_inputs(counter_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage_word_entries(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        expected_entries,
    );
}

/// Returns the first non-zero storage key used by the multi-call FPI test.
fn first_storage_key() -> Word {
    Word::new([Felt::new(17), Felt::new(34), Felt::new(51), Felt::new(68)])
}

/// Returns the second non-zero storage key used by the multi-call FPI test.
fn second_storage_key() -> Word {
    Word::new([Felt::new(85), Felt::new(102), Felt::new(119), Felt::new(136)])
}

/// Returns the third non-zero storage key used by the multi-call FPI test.
fn third_storage_key() -> Word {
    Word::new([Felt::new(153), Felt::new(170), Felt::new(187), Felt::new(204)])
}

/// Returns the first expected `Word` value used by the multi-call FPI test.
fn expected_first_word() -> Word {
    Word::new([Felt::new(901), Felt::new(802), Felt::new(703), Felt::new(604)])
}

/// Returns the second expected `Word` value used by the multi-call FPI test.
fn expected_second_word() -> Word {
    Word::new([Felt::new(505), Felt::new(406), Felt::new(307), Felt::new(208)])
}

/// Returns the third expected `Word` value used by the multi-call FPI test.
fn expected_third_word() -> Word {
    Word::new([Felt::new(109), Felt::new(210), Felt::new(311), Felt::new(412)])
}

/// Asserts the stored `Word` entries under their storage keys.
fn assert_counter_storage_word_entries(
    counter_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    expected_entries: [(Word, Word); 3],
) {
    for (storage_key, expected_word) in expected_entries {
        let word = counter_account_storage
            .get_map_item(storage_slot, storage_key)
            .expect("Failed to get counter value from storage slot");

        assert_eq!(word, expected_word, "Counter word value mismatch");
    }
}

/// Minimal counter account component source used by the multi-call FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type, Felt, StorageMap, Word};

/// Pair of storage keys passed through the FPI boundary.
#[export_type]
pub struct KeyPair {
    /// First storage key to read.
    pub first_key: Word,
    /// Second storage key to read.
    pub second_key: Word,
}

/// Pair of counter words returned through the FPI boundary.
#[export_type]
pub struct WordPair {
    /// Word associated with the first key.
    pub first: Word,
    /// Word associated with the second key.
    pub second: Word,
}

/// Account component whose storage map holds counter words.
#[component]
struct CounterContract {
    /// Storage map holding counter words.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Word>,
}

#[component]
impl CounterContract {
    /// Returns the sum of the first felt in the three words stored under the provided keys.
    pub fn sum_first_elements_by_keys(
        &self,
        first_key: Word,
        second_key: Word,
        third_key: Word,
    ) -> Felt {
        self.count_map.get(first_key)[0]
            + self.count_map.get(second_key)[0]
            + self.count_map.get(third_key)[0]
    }

    /// Returns the counter words stored under `keys`.
    pub fn get_count_pair_by_keys(&self, keys: KeyPair) -> WordPair {
        WordPair {
            first: self.count_map.get(keys.first_key),
            second: self.count_map.get(keys.second_key),
        }
    }
}
"#;

/// Minimal note script source which invokes multiple FPI methods on one account.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::multi_call_account::multi_call_account::KeyPair;
use crate::bindings::MultiCallAccount as CounterContract;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that multiple FPI calls on one account preserve per-call ABI metadata.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let first_key = Word::new([felt!(17), felt!(34), felt!(51), felt!(68)]);
        let second_key = Word::new([felt!(85), felt!(102), felt!(119), felt!(136)]);
        let third_key = Word::new([felt!(153), felt!(170), felt!(187), felt!(204)]);
        let expected_first = Word::new([felt!(901), felt!(802), felt!(703), felt!(604)]);
        let expected_second = Word::new([felt!(505), felt!(406), felt!(307), felt!(208)]);

        let sum = count_acc.sum_first_elements_by_keys(first_key, second_key, third_key);
        assert_eq(sum, felt!(1515));

        let pair = count_acc.get_count_pair_by_keys(KeyPair {
            first_key,
            second_key,
        });
        assert_word_eq(pair.first, expected_first);
        assert_word_eq(pair.second, expected_second);
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
