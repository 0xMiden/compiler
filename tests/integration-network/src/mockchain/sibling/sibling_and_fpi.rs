//! The same dependency used both as a sibling (intra-account) and through FPI (inter-account).
//!
//! The caller component depends on one counter package and reaches it two ways: through the
//! generated sibling trait against the copy deployed on its own account, and through an
//! `#[account(...)]` wrapper against a copy deployed on a second, foreign account. This also
//! exercises the same WIT interface being imported by the sibling bindings (plain functions
//! only) and the `#[account]` bindings (plain + `fpi-*` functions) in one crate.

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
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};

use super::{
    super::support::{execute_tx, note_script_root, to_core_felts},
    assert_counter_storage_at_key,
    common::build_sibling_test_packages,
    counter_storage_key,
};

/// Deploys the caller account (sibling counter + caller component) and a second account holding
/// the same counter package, then consumes a note which increments the local counter through the
/// sibling trait and reads the remote counter through FPI.
#[test]
fn sibling_and_fpi() {
    let (counter_package, caller_package, note_package, counter_storage_slot) =
        build_sibling_test_packages(
            "sibling_and_fpi",
            COUNTER_CONTRACT_SOURCE,
            CALLER_ACCOUNT_SOURCE,
            NOTE_SOURCE,
        );

    let local_counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), counter_storage_key(), 314_u64)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };
    let remote_counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), counter_storage_key(), 777_u64)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };
    let caller_component =
        AccountComponent::from_package(&caller_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let remote_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(remote_counter_component)
        .build_existing()
        .expect("failed to build the remote counter account");
    builder
        .add_account(remote_account.clone())
        .expect("failed to add the remote counter account to the mock chain builder");

    let account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(local_counter_component)
        .with_component(caller_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the caller account to the mock chain builder");

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let note = NoteBuilder::new(account.id(), rng)
        .package((*note_package).clone())
        .note_storage(to_core_felts(&remote_account.id()))
        .unwrap()
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
    assert_counter_storage_at_key(
        chain.committed_account(remote_account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        777,
    );

    let foreign_account_inputs = chain.get_foreign_account_inputs(remote_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(account.clone(), &[note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    // The local sibling counter was incremented; the remote counter was only read through FPI.
    assert_counter_storage_at_key(
        chain.committed_account(account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        315,
    );
    assert_counter_storage_at_key(
        chain.committed_account(remote_account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        777,
    );
}

/// Counter component deployed on both the caller account and the remote account.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Account component whose storage map holds one counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Counter component exposing read and increment over its counter.
#[component]
trait CounterContract {
    /// Returns the counter value stored under the provided key.
    fn get_count(&self, key: Word) -> Felt;
    /// Increments the counter value stored under the provided key, returning the new value.
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

/// Caller component using the counter dependency both as a sibling and through FPI.
const CALLER_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{
    account, assert_eq, component, component_storage, felt, native_account::NativeAccount,
    AccountId, Felt, Word,
};

/// Foreign binding to another account holding the same counter component package.
#[account(sibling_and_fpi_counter_account::CounterContract)]
struct RemoteCounter;

/// Storage-less component which reaches the counter dependency two ways.
#[component_storage]
struct CallerAccountStorage;

/// Account component using the counter both as a sibling and as a foreign account through FPI.
#[component(sibling_and_fpi_counter_account::CounterContract)]
trait CallerAccount: NativeAccount + CounterContract {
    /// Increments the local sibling counter, then reads the remote account's counter via FPI.
    fn bump_local_read_remote(&mut self, remote_account_id: AccountId) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    fn bump_local_read_remote(&mut self, remote_account_id: AccountId) -> Felt {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        // Intra-account sibling call through the generated `CounterContract` trait.
        let before = self.get_count(key);
        let after = self.increment_count(key);
        assert_eq(after, before + felt!(1));
        // Inter-account FPI call through the `#[account]` wrapper's inherent methods.
        let remote = RemoteCounter::new(remote_account_id);
        remote.get_count(key)
    }
}
"#;

/// Note script which triggers the mixed sibling + FPI flow on the active account.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller component account.
#[account(sibling_and_fpi_caller_account::CallerAccount)]
struct Account;

/// Note input: the remote account whose counter is read through FPI.
#[note]
struct SiblingAndFpiNote {
    /// Account id of the remote counter account.
    remote_account_id: AccountId,
}

#[note]
impl SiblingAndFpiNote {
    /// Invokes the caller component method mixing a sibling call and an FPI call.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let remote_count = account.bump_local_read_remote(self.remote_account_id);
        assert_eq(remote_count, felt!(777));
    }
}
"#;
