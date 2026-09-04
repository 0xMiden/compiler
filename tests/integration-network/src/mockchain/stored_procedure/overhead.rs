//! Cycle cost of stored-procedure dispatch against a direct sibling call of the same procedure.

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData, StorageValueName},
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
use midenc_expect_test::expect;

use super::{
    super::support::{
        assert_counter_storage_at_key, counter_storage_slot_name_for_package,
        execute_tx_measurements, note_script_root, single_note_cycles,
    },
    common::{
        COUNTER_CONTRACT_SOURCE, DispatchProjectNames, build_dispatcher_package_with_dependencies,
        build_note_package, build_target_package, counter_storage_key, lifted_export_root,
    },
};

/// One dispatcher reaches the same two counter procedures directly, through the generated
/// sibling trait (`call`), and through stored roots (`dyncall`). Three notes perform identical
/// work: direct calls; direct calls after reading the two slots; and stored dispatch. The pinned
/// differences split the cost of stored dispatch into the storage-slot read and the dispatch
/// itself (unset-slot guard, root spill, `dyncall`) relative to a direct `call`.
#[test]
fn overhead() {
    let names = DispatchProjectNames::new("stored_procedure_overhead");
    let (counter_project, counter_package) =
        build_target_package(&names, "counter-contract", COUNTER_CONTRACT_SOURCE);
    let dependencies = [(names.target_account_package.as_str(), counter_project.root())];
    let (dispatcher_project, dispatcher_package) =
        build_dispatcher_package_with_dependencies(&names, &dependencies, DISPATCHER_SOURCE);
    let direct_note_package =
        build_note_package(&names, "direct-note", dispatcher_project.root(), DIRECT_NOTE_SOURCE);
    let read_direct_note_package = build_note_package(
        &names,
        "read-direct-note",
        dispatcher_project.root(),
        READ_DIRECT_NOTE_SOURCE,
    );
    let stored_note_package =
        build_note_package(&names, "stored-note", dispatcher_project.root(), STORED_NOTE_SOURCE);

    let counter_storage_slot = counter_storage_slot_name_for_package(&names.target_account_package);
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), counter_storage_key(), 314_u64)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };
    let dispatcher_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_value(
                StorageValueName::from_slot_name(&names.dispatcher_slot("increment")),
                lifted_export_root(&counter_package, "increment-count"),
            )
            .unwrap();
        init_storage_data
            .insert_value(
                StorageValueName::from_slot_name(&names.dispatcher_slot("add")),
                lifted_export_root(&counter_package, "add-to-count"),
            )
            .unwrap();
        AccountComponent::from_package(&dispatcher_package, &init_storage_data).unwrap()
    };

    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([3_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(counter_component)
        .with_component(dispatcher_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the dispatch account to the mock chain builder");

    let mut notes = Vec::new();
    for note_package in [&direct_note_package, &read_direct_note_package, &stored_note_package] {
        let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
        let note = NoteBuilder::new(account.id(), rng)
            .package((**note_package).clone())
            .tag(NoteTag::with_account_target(account.id()).into())
            .build()
            .unwrap();
        builder.add_output_note(RawOutputNote::Full(note.clone()));
        notes.push(note);
    }

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let mut note_cycles = Vec::new();
    for note in notes {
        let account = chain.committed_account(account.id()).unwrap().clone();
        let mock_tx = chain
            .build_transaction(account)
            .authenticated_input_note(note.id())
            .build()
            .unwrap();
        let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
        note_cycles.push(single_note_cycles(&tx_measurements).parse::<usize>().unwrap());
    }
    // All three notes ran: 314, then + 1 + 5 through each of the three paths
    assert_counter_storage_at_key(
        chain.committed_account(account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        332,
    );

    let [direct, read_direct, stored] = note_cycles[..] else {
        panic!("expected three note measurements, got {note_cycles:?}");
    };
    assert!(
        direct < read_direct && direct < stored,
        "expected direct ({direct}) to be the cheapest path; read + direct {read_direct}, stored \
         {stored}"
    );
    let per_call = |a: usize, b: usize| (b as i64 - a as i64) / 2;
    // Per dispatched call: [direct call note total, slot read, dispatch through the read root
    // relative to a direct call]
    expect![[r#"
        [
            9963,
            517,
            125,
        ]
    "#]]
    .assert_debug_eq(&[
        direct as i64,
        per_call(direct, read_direct),
        per_call(read_direct, stored),
    ]);
}

/// Dispatcher reaching the counter both through the sibling trait and through stored roots.
const DISPATCHER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, native_account::NativeAccount, Felt, StorageValue, StoredProcedure, Word};

/// Storage holding the roots of the counter procedures the dispatcher also calls directly.
#[component_storage]
struct DispatcherStorage {
    #[storage(description = "root of the counter's increment procedure")]
    increment: StorageValue<StoredProcedure<fn(key: Word) -> Felt>>,
    #[storage(description = "root of the counter's add procedure")]
    add: StorageValue<StoredProcedure<fn(key: Word, delta: Felt) -> Felt>>,
}

/// Account component dispatching to the counter either directly or through stored roots.
#[component(stored_procedure_overhead_target_account::CounterContract)]
trait Dispatcher: NativeAccount + CounterContract {
    /// Increments and adds through direct sibling calls, returning the final counter value.
    #[account_procedure]
    fn dispatch_direct(&mut self, key: Word, delta: Felt) -> Felt;
    /// Reads both stored-root slots, then increments and adds through direct sibling calls.
    #[account_procedure]
    fn dispatch_read_direct(&mut self, key: Word, delta: Felt) -> Felt;
    /// Increments and adds through the stored roots, returning the final counter value.
    #[account_procedure]
    fn dispatch_stored(&mut self, key: Word, delta: Felt) -> Felt;
}

#[component]
impl Dispatcher for DispatcherStorage {
    fn dispatch_direct(&mut self, key: Word, delta: Felt) -> Felt {
        self.increment_count(key);
        self.add_to_count(key, delta)
    }

    fn dispatch_read_direct(&mut self, key: Word, delta: Felt) -> Felt {
        // The reads are host calls, so they are not dead code even though the roots go unused
        let _increment = self.increment.get();
        let _add = self.add.get();
        self.increment_count(key);
        self.add_to_count(key, delta)
    }

    fn dispatch_stored(&mut self, key: Word, delta: Felt) -> Felt {
        self.increment.get().call(key);
        self.add.get().call(key, delta)
    }
}
"#;

/// Note triggering the direct-call path.
const DIRECT_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_overhead_dispatcher_account::Dispatcher)]
struct Account;

/// Input-less trigger note.
#[note]
struct DirectNote;

#[note]
impl DirectNote {
    /// Updates the counter through direct sibling calls and checks the result.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let count = account.dispatch_direct(key, felt!(5));
        assert_eq(count, felt!(320));
    }
}
"#;

/// Note triggering the slot-read-then-direct-call path.
const READ_DIRECT_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_overhead_dispatcher_account::Dispatcher)]
struct Account;

/// Input-less trigger note.
#[note]
struct ReadDirectNote;

#[note]
impl ReadDirectNote {
    /// Reads the stored roots, updates the counter directly, and checks the result.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let count = account.dispatch_read_direct(key, felt!(5));
        assert_eq(count, felt!(326));
    }
}
"#;

/// Note triggering the stored-dispatch path.
const STORED_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_overhead_dispatcher_account::Dispatcher)]
struct Account;

/// Input-less trigger note.
#[note]
struct StoredNote;

#[note]
impl StoredNote {
    /// Updates the counter through the stored roots and checks the result.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let count = account.dispatch_stored(key, felt!(5));
        assert_eq(count, felt!(332));
    }
}
"#;
