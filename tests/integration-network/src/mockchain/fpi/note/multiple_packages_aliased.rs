//! Two account packages exporting the *same* interface name, disambiguated by `as Alias`.
//!
//! `multiple_packages` covers two components with *distinct* interfaces; this covers the `as Alias`
//! path end-to-end: both dependencies export `counter-contract` (trait `CounterContract`), and the
//! wrapper renames them `as FirstCounter` / `as SecondCounter`. This proves the alias not only
//! compiles but routes each call to its own package's procedure at runtime (the procedure-root
//! lookup keys on the interface's full WIT path, not the Rust trait name).

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
    common::build_multi_package_fpi_test_packages,
};

/// Deploys an account with two same-interface components and consumes a note that reads both through
/// `as`-aliased wrapper traits.
#[test]
pub fn multiple_packages_aliased() {
    let (first_package, second_package, note_package, first_storage_slot, second_storage_slot) =
        build_multi_package_fpi_test_packages(
            "multiple_packages_aliased",
            "counter-contract",
            "counter-contract",
            COUNTER_CONTRACT_SOURCE,
            COUNTER_CONTRACT_SOURCE,
            CALLER_SOURCE,
        );

    let first_component = component_with_count(&first_package, &first_storage_slot, 41);
    let second_component = component_with_count(&second_package, &second_storage_slot, 73);

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

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let caller_note = NoteBuilder::new(caller_account.id(), rng)
        .package((*note_package).clone())
        .note_storage(to_core_felts(&foreign_account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(caller_account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(caller_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    // The note asserts each aliased trait reads its own package's counter (41 vs 73) via FPI.
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

/// Builds a counter component from `package` with `count` stored under the counter key at `slot`.
fn component_with_count(
    package: &Arc<Package>,
    slot: &StorageSlotName,
    count: u64,
) -> AccountComponent {
    let mut init_storage_data = InitStorageData::default();
    init_storage_data
        .insert_map_entry(slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, count)
        .unwrap();
    AccountComponent::from_package(package, &init_storage_data).unwrap()
}

/// Counter component exporting the `counter-contract` interface, shared by both packages.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

#[component_storage]
struct CounterContractStorage {
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
trait CounterContract {
    /// Returns the stored counter value.
    #[account_procedure]
    fn get_count(&self) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }
}
"#;

/// Note script whose wrapper derives both same-interface components, renamed with `as`.
const CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[account(
    multiple_packages_aliased_first_account::CounterContract as FirstCounter,
    multiple_packages_aliased_second_account::CounterContract as SecondCounter
)]
struct ForeignCounters;

/// Note input containing the foreign account id.
#[note]
struct CounterCaller {
    foreign_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Both packages export `counter-contract`; the `as` aliases give the generated traits distinct
    /// names, and each routes to its own package's procedure.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let counters = ForeignCounters::new(self.foreign_account_id);

        let first = <ForeignCounters as FirstCounter>::get_count(&counters);
        let second = <ForeignCounters as SecondCounter>::get_count(&counters);

        assert_eq(first, felt!(41));
        assert_eq(second, felt!(73));
    }
}
"#;
