//! One account component calling a single sibling component of the same account.

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
    common::build_sibling_test_packages,
    counter_storage_key,
};

/// Deploys one account holding the sibling counter and the caller component, then consumes a
/// note which makes the caller increment the sibling's counter through the generated sibling
/// trait (an intra-account cross-context call). The increment proves storage writes made by a
/// sibling-called component commit to the account.
#[test]
fn single_sibling() {
    let (counter_package, caller_package, note_package, counter_storage_slot) =
        build_sibling_test_packages(
            "single_sibling",
            COUNTER_CONTRACT_SOURCE,
            CALLER_ACCOUNT_SOURCE,
            NOTE_SOURCE,
        );

    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), counter_storage_key(), 314_u64)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };
    let caller_component =
        AccountComponent::from_package(&caller_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(counter_component)
        .with_component(caller_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the sibling-call account to the mock chain builder");

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

    assert_counter_storage_at_key(
        chain.committed_account(account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        314,
    );

    let tx_context_builder = chain.build_tx_context(account.clone(), &[note.id()], &[]).unwrap();
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage_at_key(
        chain.committed_account(account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        315,
    );
}

/// Sibling counter component with a read and a mutating increment over its storage map.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Sibling account component whose storage map holds one counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "sibling counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Sibling account component exposing read and increment over its counter.
#[component]
trait CounterContract {
    /// Returns the counter value stored under the provided key.
    #[account_procedure]
    fn get_count(&self, key: Word) -> Felt;
    /// Increments the counter value stored under the provided key, returning the new value.
    #[account_procedure]
    fn increment_count(&mut self, key: Word) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self, key: Word) -> Felt {
        self.count_map.get(key)
    }

    fn increment_count(&mut self, key: Word) -> Felt {
        let new_value = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
"#;

/// Caller account component which reaches its sibling through the generated sibling trait.
const CALLER_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{assert_eq, component, component_storage, felt, native_account::NativeAccount, Felt, Word};

/// Storage-less component which forwards calls to its sibling counter component.
#[component_storage]
struct CallerAccountStorage;

/// Account component which calls the sibling counter component of the same account.
#[component(single_sibling_counter_account::CounterContract)]
trait CallerAccount: NativeAccount + CounterContract {
    /// Increments the sibling counter and returns the new value.
    #[account_procedure]
    fn bump_sibling_count(&mut self) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    fn bump_sibling_count(&mut self) -> Felt {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let before = self.get_count(key);
        let after = self.increment_count(key);
        assert_eq(after, before + felt!(1));
        after
    }
}
"#;

/// Note script which triggers the sibling call on the active account.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller component account.
#[account(single_sibling_caller_account::CallerAccount)]
struct Account;

/// Note script input-less trigger for the sibling call.
#[note]
struct SiblingCallerNote;

#[note]
impl SiblingCallerNote {
    /// Invokes the caller component method which calls into its sibling counter component.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let count = account.bump_sibling_count();
        assert_eq(count, felt!(315));
    }
}
"#;
