//! Foreign procedure invocation tests for one account with multiple component packages.

use std::sync::Arc;

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{
    account::{AccountBuilder, AccountType, StorageSlotName, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::{account::auth::NoAuth, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};

use super::super::{
    super::support::{
        COUNTER_CONTRACT_STORAGE_KEY, assert_counter_storage, execute_tx, note_script_root,
        to_core_felts,
    },
    common::{
        FIRST_COUNTER_COMPONENT_SOURCE, SECOND_COUNTER_COMPONENT_SOURCE,
        build_multi_package_fpi_test_packages,
    },
};

/// Deploys an account with two components and consumes a note using one multi-package FPI binding.
#[test]
pub fn multiple_packages() {
    let (
        first_account_package,
        second_account_package,
        caller_note_package,
        first_storage_slot,
        second_storage_slot,
    ) = build_multi_package_fpi_test_packages(
        "multiple_packages",
        FIRST_COUNTER_COMPONENT_SOURCE,
        SECOND_COUNTER_COMPONENT_SOURCE,
        COUNTER_CALLER_SOURCE,
    );

    execute_multiple_package_counter_caller_note(
        first_account_package,
        second_account_package,
        caller_note_package,
        first_storage_slot,
        second_storage_slot,
    );
}

/// Deploys both foreign account components and consumes the caller note.
fn execute_multiple_package_counter_caller_note(
    first_account_package: Arc<Package>,
    second_account_package: Arc<Package>,
    caller_note_package: Arc<Package>,
    first_storage_slot: StorageSlotName,
    second_storage_slot: StorageSlotName,
) {
    let first_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(first_storage_slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, 41_u64)
            .unwrap();
        AccountComponent::from_package(&first_account_package, &init_storage_data).unwrap()
    };
    let second_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(second_storage_slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, 73_u64)
            .unwrap();
        AccountComponent::from_package(&second_account_package, &init_storage_data).unwrap()
    };

    let mut builder = MockChain::builder();
    let foreign_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(first_component)
        .with_component(second_component)
        .build_existing()
        .expect("failed to build foreign account");
    builder
        .add_account(foreign_account.clone())
        .expect("failed to add foreign account to mock chain builder");

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
        .note_storage(to_core_felts(&foreign_account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(caller_account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(caller_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    assert_counter_storage(
        chain.committed_account(foreign_account.id()).unwrap().storage(),
        &first_storage_slot,
        41,
    );
    assert_counter_storage(
        chain.committed_account(foreign_account.id()).unwrap().storage(),
        &second_storage_slot,
        73,
    );

    let foreign_account_inputs = chain.get_foreign_account_inputs(foreign_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage(
        chain.committed_account(foreign_account.id()).unwrap().storage(),
        &first_storage_slot,
        41,
    );
    assert_counter_storage(
        chain.committed_account(foreign_account.id()).unwrap().storage(),
        &second_storage_slot,
        73,
    );
}

/// Note script source which invokes FPI methods from two imported packages on one account.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[account(multiple_packages_first_account::FirstCounter, multiple_packages_second_account::SecondCounter)]
struct ForeignCounters;

/// Note script input containing the foreign account id.
#[note]
struct CounterCaller {
    /// Account id with both counter components deployed.
    foreign_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that a multi-package foreign account binding exposes both component methods.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let counters = ForeignCounters::new(self.foreign_account_id);

        // Both components export a `get_count` method, so `counters.get_count()` is ambiguous;
        // the call must be disambiguated through the per-component trait with UFCS.
        let first = <ForeignCounters as FirstCounter>::get_count(&counters);
        let second = <ForeignCounters as SecondCounter>::get_count(&counters);

        assert_eq(first, felt!(41));
        assert_eq(second, felt!(73));
    }
}
"#;
