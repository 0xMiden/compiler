//! Foreign procedure invocation tests for methods that accept a four-word record.

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
use miden_mast_package::Package;
use miden_protocol::{
    account::{AccountBuilder, AccountStorage, AccountType, StorageSlotName, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};

use super::super::{
    super::support::{execute_tx, note_script_root, to_core_felts},
    common::build_fpi_test_packages,
};

/// Deploys a counter contract and consumes a note which passes a four-word record through FPI.
#[test]
pub fn four_words_struct() {
    let (counter_package, caller_note_package, counter_storage_slot) = build_fpi_test_packages(
        "four_words_struct",
        COUNTER_CONTRACT_SOURCE,
        COUNTER_CALLER_SOURCE,
    );

    execute_four_words_struct_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        [
            (first_storage_key(), FIRST_COUNT),
            (second_storage_key(), SECOND_COUNT),
            (third_storage_key(), THIRD_COUNT),
            (fourth_storage_key(), FOURTH_COUNT),
        ],
    );
}

/// Deploys a `Felt`-valued counter contract and consumes the caller note.
fn execute_four_words_struct_counter_caller_note(
    counter_package: Arc<Package>,
    caller_note_package: Arc<Package>,
    counter_storage_slot: StorageSlotName,
    expected_entries: [(Word, u64); 4],
) {
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        for (storage_key, expected_count) in expected_entries {
            init_storage_data
                .insert_map_entry(counter_storage_slot.clone(), storage_key, expected_count)
                .unwrap();
        }
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

    assert_counter_storage_felt_entries(
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

    assert_counter_storage_felt_entries(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        expected_entries,
    );
}

/// Returns the first non-zero storage key used by the four-word record FPI test.
fn first_storage_key() -> Word {
    Word::new([17_u32.into(), 34_u32.into(), 51_u32.into(), 68_u32.into()])
}

/// Returns the second non-zero storage key used by the four-word record FPI test.
fn second_storage_key() -> Word {
    Word::new([85_u32.into(), 102_u32.into(), 119_u32.into(), 136_u32.into()])
}

/// Returns the third non-zero storage key used by the four-word record FPI test.
fn third_storage_key() -> Word {
    Word::new([153_u32.into(), 170_u32.into(), 187_u32.into(), 204_u32.into()])
}

/// Returns the fourth non-zero storage key used by the four-word record FPI test.
fn fourth_storage_key() -> Word {
    Word::new([221_u32.into(), 238_u32.into(), 255_u32.into(), 272_u32.into()])
}

/// Asserts the stored `Felt` entries under their storage keys.
fn assert_counter_storage_felt_entries(
    counter_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    expected_entries: [(Word, u64); 4],
) {
    for (storage_key, expected_count) in expected_entries {
        let word = counter_account_storage
            .get_map_item(storage_slot, storage_key)
            .expect("Failed to get counter value from storage slot");
        let val = word[0];

        assert_eq!(
            val.as_canonical_u64(),
            expected_count,
            "Counter felt value mismatch. Expected: {}, Got: {}",
            expected_count,
            val.as_canonical_u64()
        );
    }
}

/// First counter value used by the four-word record FPI test.
const FIRST_COUNT: u64 = 101;

/// Second counter value used by the four-word record FPI test.
const SECOND_COUNT: u64 = 202;

/// Third counter value used by the four-word record FPI test.
const THIRD_COUNT: u64 = 303;

/// Fourth counter value used by the four-word record FPI test.
const FOURTH_COUNT: u64 = 404;

/// Minimal counter account component source used by the four-word record FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, export_type, Felt, StorageMap, Word};

/// Four storage keys passed through the FPI boundary.
#[export_type]
pub struct KeyQuad {
    /// First storage key to read.
    pub first_key: Word,
    /// Second storage key to read.
    pub second_key: Word,
    /// Third storage key to read.
    pub third_key: Word,
    /// Fourth storage key to read.
    pub fourth_key: Word,
}

/// Account component whose storage map holds counter values.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding counter values.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose storage map holds counter values.
#[component]
trait CounterContract {
    /// Returns the sum of the counter values stored under `keys`.
    fn get_count_sum_by_keys(&self, keys: KeyQuad) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Returns the sum of the counter values stored under `keys`.
    fn get_count_sum_by_keys(&self, keys: KeyQuad) -> Felt {
        self.count_map.get(keys.first_key)
            + self.count_map.get(keys.second_key)
            + self.count_map.get(keys.third_key)
            + self.count_map.get(keys.fourth_key)
    }
}
"#;

/// Minimal note script source which reads the generated counter account through FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::four_words_struct_account::counter_contract::KeyQuad;
#[account(four_words_struct_account::CounterContract)]
struct Counter;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that four `Word` values in one record cross the FPI boundary.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = Counter::new(self.counter_account_id);
        let first_key = Word::new([felt!(17), felt!(34), felt!(51), felt!(68)]);
        let second_key = Word::new([felt!(85), felt!(102), felt!(119), felt!(136)]);
        let third_key = Word::new([felt!(153), felt!(170), felt!(187), felt!(204)]);
        let fourth_key = Word::new([felt!(221), felt!(238), felt!(255), felt!(272)]);

        let keys = KeyQuad {
            first_key,
            second_key,
            third_key,
            fourth_key,
        };
        let count = count_acc.get_count_sum_by_keys(keys);

        assert_eq(count, felt!(1010));
    }
}
"#;
