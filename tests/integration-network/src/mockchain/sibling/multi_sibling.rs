//! One account component calling two different sibling components of the same account.

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_protocol::{
    account::{AccountBuilder, AccountType, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{AccountState, Auth, MockChain};

use super::{
    super::support::{assert_counter_storage_at_key, execute_tx, note_script_root},
    common::build_multi_sibling_test_packages,
    counter_storage_key,
};

/// Deploys one account holding two sibling counter components and the caller component, then
/// consumes a note which makes the caller increment both counters through the two generated
/// sibling traits.
#[test]
fn multi_sibling() {
    let (
        first_package,
        second_package,
        caller_package,
        note_package,
        first_storage_slot,
        second_storage_slot,
    ) = build_multi_sibling_test_packages(
        "multi_sibling",
        FIRST_COUNTER_CONTRACT_SOURCE,
        SECOND_COUNTER_CONTRACT_SOURCE,
        CALLER_ACCOUNT_SOURCE,
        NOTE_SOURCE,
    );

    let first_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(first_storage_slot.clone(), counter_storage_key(), 41_u64)
            .unwrap();
        AccountComponent::from_package(&first_package, &init_storage_data).unwrap()
    };
    let second_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(second_storage_slot.clone(), counter_storage_key(), 73_u64)
            .unwrap();
        AccountComponent::from_package(&second_package, &init_storage_data).unwrap()
    };
    let caller_component =
        AccountComponent::from_package(&caller_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(first_component)
        .with_component(second_component)
        .with_component(caller_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the multi-sibling account to the mock chain builder");

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let note = NoteBuilder::new(account.id(), rng)
        .package((*note_package).clone())
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let storage = chain.committed_account(account.id()).unwrap().storage();
    assert_counter_storage_at_key(storage, &first_storage_slot, counter_storage_key(), 41);
    assert_counter_storage_at_key(storage, &second_storage_slot, counter_storage_key(), 73);

    let tx_context_builder = chain.build_tx_context(account.clone(), &[note.id()], &[]).unwrap();
    execute_tx(&mut chain, tx_context_builder);

    let storage = chain.committed_account(account.id()).unwrap().storage();
    assert_counter_storage_at_key(storage, &first_storage_slot, counter_storage_key(), 42);
    assert_counter_storage_at_key(storage, &second_storage_slot, counter_storage_key(), 74);
}

/// First sibling counter component.
const FIRST_COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Sibling account component whose storage map holds one counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "first sibling counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// First sibling component exposing an increment over its counter.
#[component]
trait CounterContract {
    /// Increments the counter value stored under the provided key, returning the new value.
    #[account_procedure]
    fn increment_count(&mut self, key: Word) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn increment_count(&mut self, key: Word) -> Felt {
        let new_value = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
"#;

/// Second sibling counter component with a distinct trait and method names.
///
/// Both generated sibling traits attach to the caller's storage struct, so the second sibling
/// uses distinct method names to keep `self.*` calls unambiguous.
const SECOND_COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Sibling account component whose storage map holds one counter value.
#[component_storage]
struct SecondCounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "second sibling counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Second sibling component exposing an increment over its counter.
#[component]
trait SecondCounterContract {
    /// Increments the counter value stored under the provided key, returning the new value.
    #[account_procedure]
    fn increment_second_count(&mut self, key: Word) -> Felt;
}

#[component]
impl SecondCounterContract for SecondCounterContractStorage {
    fn increment_second_count(&mut self, key: Word) -> Felt {
        let new_value = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
"#;

/// Caller account component which increments both sibling counters.
const CALLER_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{assert_eq, component, component_storage, felt, native_account::NativeAccount, Felt, Word};

/// Storage-less component which forwards calls to its two sibling counter components.
#[component_storage]
struct CallerAccountStorage;

/// Account component which calls both sibling counter components of the same account.
#[component(
    multi_sibling_counter_account::CounterContract,
    multi_sibling_second_counter_account::SecondCounterContract
)]
trait CallerAccount: NativeAccount + CounterContract + SecondCounterContract {
    /// Increments both sibling counters and returns the second counter's new value.
    #[account_procedure]
    fn bump_both_counts(&mut self) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    fn bump_both_counts(&mut self) -> Felt {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let first = self.increment_count(key);
        let second = self.increment_second_count(key);
        assert_eq(first, felt!(42));
        assert_eq(second, felt!(74));
        second
    }
}
"#;

/// Note script which triggers both sibling calls on the active account.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller component account.
#[account(multi_sibling_caller_account::CallerAccount)]
struct Account;

/// Note script input-less trigger for the sibling calls.
#[note]
struct MultiSiblingNote;

#[note]
impl MultiSiblingNote {
    /// Invokes the caller component method which calls into both sibling counter components.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let second = account.bump_both_counts();
        assert_eq(second, felt!(74));
    }
}
"#;
