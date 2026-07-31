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
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};

use super::super::{
    super::support::{execute_tx, note_script_root, to_core_felts},
    common::build_fpi_test_packages,
};

/// Deploys an active account and a foreign account that both carry the builtin-colliding `get_id`
/// component, then consumes a note that reads the component `get_id` on the active account
/// (native), the `ActiveAccount::get_id` built-in, and the component `get_id` on the foreign
/// account (FPI) — all disambiguated with UFCS.
#[test]
fn get_id_component_coexists_with_active_account_builtin() {
    let (account_package, note_package, _storage_slot) =
        build_fpi_test_packages("get_id_collision", GET_ID_COMPONENT_SOURCE, NOTE_SOURCE);

    let active_component =
        AccountComponent::from_package(&account_package, &InitStorageData::default()).unwrap();
    let foreign_component =
        AccountComponent::from_package(&account_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();

    // A second account carrying the same component, reached from the note through FPI.
    let foreign_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(foreign_component)
        .build_existing()
        .expect("failed to build the foreign account");
    builder
        .add_account(foreign_account.clone())
        .expect("failed to add the foreign account to the mock chain builder");

    let account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(active_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the active account to the mock chain builder");

    // Note inputs: the active account's own id (checked against the `ActiveAccount::get_id`
    // built-in) followed by the foreign account's id (reached through FPI).
    let note_inputs = [to_core_felts(&account.id()), to_core_felts(&foreign_account.id())].concat();
    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let note = NoteBuilder::new(account.id(), rng)
        .package((*note_package).clone())
        .note_storage(note_inputs)
        .unwrap()
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    // The note asserts all three `get_id` results internally; a wrong resolution fails the tx.
    let foreign_account_inputs = chain.get_foreign_account_inputs(foreign_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(account.clone(), &[note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
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
    #[account_procedure]
    fn get_id(&self) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_id(&self) -> Felt {
        felt!(999)
    }
}
"#;

/// Note whose active account carries the builtin-colliding component; reads the component `get_id`
/// natively and through FPI, and the `ActiveAccount::get_id` built-in, all by UFCS.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;
use miden::active_account::ActiveAccount;

#[account(get_id_collision_account::CounterContract)]
struct Wallet;

/// Note inputs: the id the active account is expected to report, and a foreign account carrying
/// the same component, reached through FPI.
#[note]
struct GetIdNote {
    expected_active_id: AccountId,
    foreign_account_id: AccountId,
}

#[note]
impl GetIdNote {
    /// Checks that the component `get_id` and the `ActiveAccount::get_id` built-in are distinct
    /// trait methods that both resolve via UFCS, on both the native and the FPI dispatch branch.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Wallet) {
        // Native branch: the component `get_id` on the active account, and the built-in.
        let component_id = <Wallet as CounterContract>::get_id(account);
        assert_eq(component_id, felt!(999));

        let active_id = <Wallet as ActiveAccount>::get_id(account);
        assert_eq(active_id.prefix, self.expected_active_id.prefix);
        assert_eq(active_id.suffix, self.expected_active_id.suffix);

        // FPI branch: the same builtin-colliding component `get_id` on the foreign account.
        let remote = Wallet::new(self.foreign_account_id);
        let remote_id = <Wallet as CounterContract>::get_id(&remote);
        assert_eq(remote_id, felt!(999));
    }
}
"#;
