//! A dispatcher component calling a Rust-compiled sibling through stored procedure roots.

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData, StorageValueName},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_core::{Word, mast::error_code_from_msg};
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
        execute_tx_expect_failure, execute_tx_measurements, note_script_root, single_note_cycles,
    },
    common::{
        COUNTER_CONTRACT_SOURCE, DispatchProjectNames, assert_word_value_slots,
        build_dispatcher_package, build_note_package, build_target_package, counter_storage_key,
        lifted_export_root,
    },
};

/// Deploys one account holding a counter component and a dispatcher whose stored-procedure
/// slots are populated off-chain with the counter's lifted export roots. A note makes the
/// dispatcher call both stored procedures: the counter's storage write commits through the
/// `dyncall` (the kernel authenticates the loaded root as an account procedure), the results
/// round-trip to the note, and the two-argument call pins the argument order. A second note
/// dispatches through a slot that was never populated and must fail with the unset-slot
/// assertion.
#[test]
fn rust_sibling() {
    let names = DispatchProjectNames::new("stored_procedure_rust_sibling");
    let (_counter_project, counter_package) =
        build_target_package(&names, "counter-contract", COUNTER_CONTRACT_SOURCE);
    let (dispatcher_project, dispatcher_package) =
        build_dispatcher_package(&names, DISPATCHER_SOURCE);
    let note_package = build_note_package(&names, "note", dispatcher_project.root(), NOTE_SOURCE);
    let unset_note_package =
        build_note_package(&names, "unset-note", dispatcher_project.root(), UNSET_NOTE_SOURCE);

    let counter_storage_slot = counter_storage_slot_name_for_package(&names.target_account_package);
    let increment_slot = names.dispatcher_slot("increment");
    let add_slot = names.dispatcher_slot("add");
    let unset_slot = names.dispatcher_slot("unset");
    assert_word_value_slots(
        &dispatcher_package,
        &[increment_slot.clone(), add_slot.clone(), unset_slot.clone()],
    );

    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), counter_storage_key(), 314_u64)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };
    let dispatcher_component = {
        // The roots come from the sibling package's manifest, as a deployment would set them;
        // the `unset` slot is deployed as the zero word, which `is_set()` and the dispatch guard
        // treat as unpopulated.
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_value(StorageValueName::from_slot_name(&unset_slot), Word::default())
            .unwrap();
        init_storage_data
            .insert_value(
                StorageValueName::from_slot_name(&increment_slot),
                lifted_export_root(&counter_package, "increment-count"),
            )
            .unwrap();
        init_storage_data
            .insert_value(
                StorageValueName::from_slot_name(&add_slot),
                lifted_export_root(&counter_package, "add-to-count"),
            )
            .unwrap();
        AccountComponent::from_package(&dispatcher_package, &init_storage_data).unwrap()
    };

    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([1_u8; 32])
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

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let note = NoteBuilder::new(account.id(), rng)
        .package((*note_package).clone())
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));
    let rng = RandomCoin::new(note_script_root(unset_note_package.as_ref()));
    let unset_note = NoteBuilder::new(account.id(), rng)
        .package((*unset_note_package).clone())
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(unset_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    assert_counter_storage_at_key(
        chain.committed_account(account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        314,
    );

    let mock_tx = chain
        .build_transaction(account.clone())
        .authenticated_input_note(note.id())
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["12424"].assert_eq(single_note_cycles(&tx_measurements));

    // 314, incremented once, then increased by 5 through the two-argument procedure
    assert_counter_storage_at_key(
        chain.committed_account(account.id()).unwrap().storage(),
        &counter_storage_slot,
        counter_storage_key(),
        320,
    );

    let account = chain.committed_account(account.id()).unwrap().clone();
    let mock_tx = chain
        .build_transaction(account)
        .authenticated_input_note(unset_note.id())
        .build()
        .unwrap();
    let error = execute_tx_expect_failure(mock_tx);
    // Transaction errors surface assertion messages of account code only as their error codes
    // (see `tx_script_args.rs`), so the guard is pinned through the code derived from its message.
    let expected = format!(
        "assertion failed with error code: {}",
        error_code_from_msg(UNSET_SLOT_ASSERTION_MESSAGE)
    );
    assert!(
        error.contains(&expected),
        "expected the unset-slot assertion `{expected}` in the transaction error, got: {error}"
    );
}

/// The message of the assertion the compiler emits when a stored-procedure slot is dispatched
/// while unset (see the `dyncall` emitter in `midenc-codegen-masm`).
const UNSET_SLOT_ASSERTION_MESSAGE: &str =
    "stored procedure slot is unset: no procedure root to dyncall";

/// Dispatcher component: its call targets live in storage, so it depends on no other package.
const DISPATCHER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{assert_eq, component, component_storage, felt, Felt, StorageValue, StoredProcedure, Word};

/// Storage holding the roots of the procedures the dispatcher calls.
#[component_storage]
struct DispatcherStorage {
    #[storage(description = "root of a procedure incrementing a counter under a key")]
    increment: StorageValue<StoredProcedure<fn(key: Word) -> Felt>>,
    #[storage(description = "root of a procedure adding a delta to a counter under a key")]
    add: StorageValue<StoredProcedure<fn(key: Word, delta: Felt) -> Felt>>,
    #[storage(description = "slot which is never populated")]
    unset: StorageValue<StoredProcedure<fn(key: Word) -> Felt>>,
}

/// Account component dispatching to the procedures whose roots are stored in its slots.
#[component]
trait Dispatcher {
    /// Calls the stored increment and add procedures, returning the final counter value.
    #[account_procedure]
    fn dispatch(&mut self, key: Word, delta: Felt) -> Felt;
    /// Calls the never-populated slot, which must fail.
    #[account_procedure]
    fn dispatch_unset(&mut self, key: Word) -> Felt;
}

#[component]
impl Dispatcher for DispatcherStorage {
    fn dispatch(&mut self, key: Word, delta: Felt) -> Felt {
        let increment = self.increment.get();
        let add = self.add.get();
        assert_eq(if increment.is_set() { felt!(1) } else { felt!(0) }, felt!(1));
        assert_eq(if self.unset.get().is_set() { felt!(1) } else { felt!(0) }, felt!(0));
        let after_increment = increment.call(key);
        let after_add = add.call(key, delta);
        assert_eq(after_add, after_increment + delta);
        after_add
    }

    fn dispatch_unset(&mut self, key: Word) -> Felt {
        self.unset.get().call(key)
    }
}
"#;

/// Note script triggering the stored-procedure dispatch on the active account.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_rust_sibling_dispatcher_account::Dispatcher)]
struct Account;

/// Input-less trigger note.
#[note]
struct DispatchNote;

#[note]
impl DispatchNote {
    /// Dispatches to the stored procedures and checks the returned counter value.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let count = account.dispatch(key, felt!(5));
        assert_eq(count, felt!(320));
    }
}
"#;

/// Note script dispatching through the never-populated slot.
const UNSET_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_rust_sibling_dispatcher_account::Dispatcher)]
struct Account;

/// Input-less trigger note.
#[note]
struct UnsetDispatchNote;

#[note]
impl UnsetDispatchNote {
    /// Dispatches through the unset slot; the transaction must fail before this returns.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        account.dispatch_unset(key);
    }
}
"#;
