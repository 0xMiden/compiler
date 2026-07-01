//! A note whose active account is a multi-component `#[account(...)]` wrapper.
//!
//! This is the active-account counterpart of `multiple_packages` (which drives the same two
//! components as a *foreign* FPI binding): here both components are deployed on the transaction's
//! active account, the `#[note]` entrypoint takes the wrapper as `account: &mut Wallet`, and the two
//! component traits — which share a `get_count` method — are called through the active account with
//! UFCS (`<Wallet as FirstCounter>::get_count(account)`), exercising the native dispatch branch.

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
    super::support::{
        COUNTER_CONTRACT_STORAGE_KEY, assert_counter_storage, execute_tx, note_script_root,
    },
    common::{
        FIRST_COUNTER_COMPONENT_SOURCE, SECOND_COUNTER_COMPONENT_SOURCE,
        build_multi_package_fpi_test_packages,
    },
};

/// Deploys an active account carrying two counter components that share a method name, then consumes
/// a note whose entrypoint reads both through the active-account wrapper with UFCS.
#[test]
pub fn multiple_components_active() {
    let (first_package, second_package, note_package, first_storage_slot, second_storage_slot) =
        build_multi_package_fpi_test_packages(
            "multiple_components_active",
            FIRST_COUNTER_COMPONENT_SOURCE,
            SECOND_COUNTER_COMPONENT_SOURCE,
            ACTIVE_CALLER_SOURCE,
        );

    let first_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(first_storage_slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, 41_u64)
            .unwrap();
        AccountComponent::from_package(&first_package, &init_storage_data).unwrap()
    };
    let second_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(second_storage_slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, 73_u64)
            .unwrap();
        AccountComponent::from_package(&second_package, &init_storage_data).unwrap()
    };

    // Both components are deployed on the transaction's *active* account.
    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(first_component)
        .with_component(second_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the active account to the mock chain builder");

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

    assert_counter_storage(
        chain.committed_account(account.id()).unwrap().storage(),
        &first_storage_slot,
        41,
    );
    assert_counter_storage(
        chain.committed_account(account.id()).unwrap().storage(),
        &second_storage_slot,
        73,
    );

    // The note reads both components on the active account through UFCS; nothing is mutated.
    let tx_context_builder = chain.build_tx_context(account.clone(), &[note.id()], &[]).unwrap();
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage(
        chain.committed_account(account.id()).unwrap().storage(),
        &first_storage_slot,
        41,
    );
    assert_counter_storage(
        chain.committed_account(account.id()).unwrap().storage(),
        &second_storage_slot,
        73,
    );
}

/// Note script whose active account derives both counter components, read with UFCS.
const ACTIVE_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[account(multiple_components_active_first_account::FirstCounter, multiple_components_active_second_account::SecondCounter)]
struct Wallet;

#[note]
struct ActiveCounterNote;

#[note]
impl ActiveCounterNote {
    /// Checks that a multi-component active-account wrapper exposes both component methods.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Wallet) {
        // Both components export `get_count`, so `account.get_count()` is ambiguous; the
        // active-account calls are disambiguated through the per-component trait with UFCS.
        let first = <Wallet as FirstCounter>::get_count(account);
        let second = <Wallet as SecondCounter>::get_count(account);

        assert_eq(first, felt!(41));
        assert_eq(second, felt!(73));
    }
}
"#;
