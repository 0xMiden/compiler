//! A component method that shares a name with an `ActiveAccount` built-in coexists with it.
//!
//! Removing the old `ACTIVE_ACCOUNT_METHODS` reject-list is the headline capability of the trait
//! redesign: a component may export a method (here `get_id`) that shares a name with an
//! `ActiveAccount` built-in without shadowing it. This deploys such a component on the transaction's
//! active account and checks that both resolve, to different correct results, via UFCS:
//! `<Wallet as CounterContract>::get_id(account)` (the component, returning a marker value) and
//! `<Wallet as ActiveAccount>::get_id(account)` (the kernel built-in, returning the account id).

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

use super::super::{
    super::support::{execute_tx, note_script_root, to_core_felts},
    common::build_fpi_test_packages,
};

/// Deploys an account whose component exports a builtin-colliding `get_id`, then consumes a note
/// that reads both the component `get_id` and the `ActiveAccount::get_id` built-in with UFCS.
#[test]
fn get_id_component_coexists_with_active_account_builtin() {
    let (account_package, note_package, _storage_slot) =
        build_fpi_test_packages("get_id_collision", GET_ID_COMPONENT_SOURCE, NOTE_SOURCE);

    let component =
        AccountComponent::from_package(&account_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the active account to the mock chain builder");

    // The note input is the account's own id: the entrypoint checks the `ActiveAccount::get_id`
    // built-in returns it, distinct from the component `get_id`'s marker value.
    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let note = NoteBuilder::new(account.id(), rng)
        .package((*note_package).clone())
        .note_storage(to_core_felts(&account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    // The note asserts both `get_id`s internally; a wrong resolution fails the transaction.
    let tx_context_builder = chain.build_tx_context(account.clone(), &[note.id()], &[]).unwrap();
    execute_tx(&mut chain, tx_context_builder);
}

/// Account component whose `get_id` method deliberately collides with the `ActiveAccount` built-in.
const GET_ID_COMPONENT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct CounterContractStorage;

/// Component exporting a `get_id` that shares its name with the `ActiveAccount::get_id` built-in.
#[component]
trait CounterContract {
    /// Returns a marker value distinct from the account id.
    fn get_id(&self) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_id(&self) -> Felt {
        felt!(999)
    }
}
"#;

/// Note whose active account carries the builtin-colliding component; reads both `get_id`s by UFCS.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;
use miden::active_account::ActiveAccount;

#[account(get_id_collision_account::CounterContract)]
struct Wallet;

/// Note input: the id the active account is expected to report.
#[note]
struct GetIdNote {
    expected_active_id: AccountId,
}

#[note]
impl GetIdNote {
    /// Checks that the component `get_id` and the `ActiveAccount::get_id` built-in are distinct
    /// trait methods that both resolve via UFCS.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Wallet) {
        let component_id = <Wallet as CounterContract>::get_id(account);
        assert_eq(component_id, felt!(999));

        let active_id = <Wallet as ActiveAccount>::get_id(account);
        assert_eq(active_id.prefix, self.expected_active_id.prefix);
        assert_eq(active_id.suffix, self.expected_active_id.suffix);
    }
}
"#;
