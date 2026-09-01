//! Mock-chain tests for dispatch through a procedure root kept in account storage.
//!
//! The account component under test is `examples/stored-procedure-example`. Its `handler` storage
//! slot holds the MAST root of the procedure `dispatch` calls, and its `weighted_handler` slot
//! holds the root of the procedure `dispatch_weighted` calls.
//!
//! Four properties need a real transaction to be proved:
//!
//! * A root taken from another component of the same account dispatches to that component.
//! * A root that `set_handler` writes into an empty slot is committed as given, and dispatches
//!   back into the account.
//! * A slot that was never initialized reads as the zero word and stops the transaction.
//! * A call that carries arguments passes every field element to the callee in the declared
//!   order.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use miden_client::{
    Word,
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_mast_package::{Package, PackageExport};
use miden_protocol::{
    account::{
        Account, AccountBuilder, AccountType, StorageSlotName, auth::AuthScheme,
        component::StorageValueName,
    },
    crypto::rand::RandomCoin,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{AccountState, Auth, MockChain};
use midenc_integration_test_support::project;

use super::support::{
    COUNTER_CONTRACT_STORAGE_KEY, compile_rust_package, counter_storage_slot_name, execute_tx,
    execute_tx_expect_failure, note_cargo_toml_for_dependency,
    note_miden_project_toml_for_dependency, note_script_root,
};

/// WIT package of the stored-procedure example, as its `miden-project.toml` declares it.
const EXAMPLE_PACKAGE: &str = "miden:stored-procedure-example";

/// Value the example's `get_value` procedure returns in these tests.
const STORED_VALUE: u64 = 7;

/// Counter value the counter contract holds in these tests.
const COUNTER_VALUE: u64 = 41;

/// Returns the repository path of an example crate.
fn example_root(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}

/// Returns the storage slot name the example derives for one of its fields.
fn example_slot(field: &str) -> StorageSlotName {
    StorageSlotName::new(format!("stored_procedure_example::stored_procedure_example::{field}"))
        .expect("the example slot name must be valid")
}

/// Returns the MAST root the manifest records for the lifted component export `name`.
///
/// A leaf name alone is ambiguous, because a manifest can expose a core function and its lifted
/// component wrapper under one name. The lifted export is the one the transaction kernel executes,
/// and it is the only one whose module segment holds the `ns:pkg/interface@version` component
/// id — the only segment containing `/`.
fn lifted_export_root(package: &Package, name: &str) -> Word {
    let suffix = format!("::\"{name}\"");
    let matches: Vec<_> = package
        .manifest
        .exports()
        .filter_map(|export| match export {
            PackageExport::Procedure(export) => Some(export),
            PackageExport::Constant(_) | PackageExport::Type(_) => None,
        })
        .filter(|export| {
            let path = export.path.as_ref().as_str();
            path.contains('/') && path.ends_with(&suffix)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one lifted component export named '{name}', got {:?}",
        matches.iter().map(|export| export.path.as_ref().as_str()).collect::<Vec<_>>(),
    );
    matches[0].digest
}

/// Builds and compiles a note package whose script drives the stored-procedure example.
fn build_caller_note_package(test_name: &str, source: &str) -> Arc<Package> {
    let name = test_name.replace('_', "-");
    let note_package = format!("miden:{name}");
    let example_root = example_root("stored-procedure-example");
    let note_project = project(&name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                &name,
                &note_package,
                EXAMPLE_PACKAGE,
                &example_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(&name, EXAMPLE_PACKAGE, &example_root),
        )
        .file("src/lib.rs", source)
        .build();
    compile_rust_package(note_project.root(), true)
}

/// Returns the initial storage of the example with the two procedure slots set to the given roots.
///
/// Each slot fixes one signature, so `handler` holds the root `dispatch` calls and
/// `weighted_handler` holds the root `dispatch_weighted` calls. Every slot the schema declares
/// needs an entry, so `value` is always supplied, and a test that uses one procedure slot only
/// supplies the zero word for the other.
fn example_init_storage(handler: Word, weighted_handler: Word) -> InitStorageData {
    let mut init_storage_data = InitStorageData::default();
    init_storage_data
        .insert_value(StorageValueName::from_slot_name(&example_slot("handler")), handler)
        .expect("the handler slot must accept a word");
    init_storage_data
        .insert_value(
            StorageValueName::from_slot_name(&example_slot("weighted_handler")),
            weighted_handler,
        )
        .expect("the weighted_handler slot must accept a word");
    init_storage_data
        .insert_value(StorageValueName::from_slot_name(&example_slot("value")), STORED_VALUE)
        .expect("the value slot must accept a felt");
    init_storage_data
}

/// Adds an account carrying `components` to `builder` and returns it.
fn add_account(
    builder: &mut miden_testing::MockChainBuilder,
    seed: [u8; 32],
    components: Vec<AccountComponent>,
) -> Account {
    let mut account_builder = AccountBuilder::new(seed)
        .account_type(AccountType::Public)
        .with_component(BasicWallet);
    for component in components {
        account_builder = account_builder.with_component(component);
    }
    builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the account to the mock chain builder")
}

/// Note script that dispatches through the account's `handler` slot and checks the result.
const DISPATCH_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::stored_procedure_example::stored_procedure_example;

#[note]
struct DispatchNote;

#[note]
impl DispatchNote {
    /// Calls the procedure the `handler` slot points at, through the account's `dispatch`.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let value = stored_procedure_example::dispatch();
        assert_eq(value, felt!(41));
    }
}
"#;

/// Note script that installs the root delivered in its note argument and dispatches through it.
const INSTALL_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::stored_procedure_example::stored_procedure_example;

#[note]
struct InstallNote;

#[note]
impl InstallNote {
    /// Writes the root the note argument carries into the `handler` slot, then dispatches to it.
    #[note_script]
    pub fn run(self, arg: Word) {
        // The note argument is an untyped word, which is the wire form of a procedure root.
        stored_procedure_example::set_handler(ProcedureRoot::from(arg));
        let value = stored_procedure_example::dispatch();
        assert_eq(value, felt!(7));
    }
}
"#;

/// Note script that dispatches a call with arguments and checks the weighted result.
///
/// The callee weighs every input differently, so the assertion holds only if each field element
/// arrives at the position the declaration states. Any exchange of two inputs gives another sum.
const WEIGHTED_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::stored_procedure_example::stored_procedure_example;

#[note]
struct WeightedNote;

#[note]
impl WeightedNote {
    /// Calls `weighted_sum` through the `handler` slot with a word and a field element.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let w = Word::new([felt!(1), felt!(10), felt!(100), felt!(1000)]);
        // 1 + 10 * 2 + 100 * 3 + 1000 * 4 + 7 * 5
        let value = stored_procedure_example::dispatch_weighted(w, felt!(7));
        assert_eq(value, felt!(4356));
    }
}
"#;

/// Dispatches from one component of an account to a procedure of another component.
///
/// The root comes from the counter contract's manifest, so nothing but the stored root connects
/// the two components: the example's package never names the counter contract.
#[test]
fn dispatch_reaches_a_procedure_of_another_component() {
    let example_package = compile_rust_package(example_root("stored-procedure-example"), true);
    let counter_package = compile_rust_package(example_root("counter-contract"), true);
    let note_package =
        build_caller_note_package("stored_procedure_cross_component", DISPATCH_NOTE_SOURCE);

    // `get-count` takes no argument and returns one field element, which is the signature the
    // example's `handler` slot fixes.
    let counter_root = lifted_export_root(&counter_package, "get-count");

    let counter_storage_slot = counter_storage_slot_name();
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(
                counter_storage_slot.clone(),
                COUNTER_CONTRACT_STORAGE_KEY,
                COUNTER_VALUE,
            )
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };
    let example_component = AccountComponent::from_package(
        &example_package,
        &example_init_storage(counter_root, Word::empty()),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let account = add_account(&mut builder, [1_u8; 32], vec![example_component, counter_component]);

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

    // The note asserts in the guest that `dispatch` returned the counter's value, so a
    // transaction that executes at all proves the dispatch reached the counter contract.
    let mock_tx = chain
        .build_transaction(account.clone())
        .authenticated_input_note(note.id())
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);
}

/// Installs a root delivered by a note, dispatches through it, and checks the committed slot.
///
/// The host reads the root of the lifted `get-value` export from the package manifest and hands it
/// to the note as the note argument. That root is the one the transaction kernel executes, so a
/// dispatch that returns the value slot proves the stored root reached the right procedure, and
/// the committed slot proves `set_handler` wrote the root unchanged.
#[test]
fn set_handler_installs_a_root_delivered_by_note() {
    let example_package = compile_rust_package(example_root("stored-procedure-example"), true);
    let note_package = build_caller_note_package("stored_procedure_install", INSTALL_NOTE_SOURCE);

    // `get-value` takes no argument and returns one field element, which is the signature the
    // example's `handler` slot fixes.
    let get_value_root = lifted_export_root(&example_package, "get-value");

    let example_component = AccountComponent::from_package(
        &example_package,
        &example_init_storage(Word::empty(), Word::empty()),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let account = add_account(&mut builder, [2_u8; 32], vec![example_component]);

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

    let handler_slot = example_slot("handler");
    assert_eq!(
        chain
            .committed_account(account.id())
            .unwrap()
            .storage()
            .get_item(&handler_slot)
            .expect("the handler slot must exist"),
        Word::empty(),
        "the handler slot must start uninitialized"
    );

    // The note installs the root the note argument carries and asserts in the guest that
    // dispatching through it returns the value slot.
    let mock_tx = chain
        .build_transaction(account.clone())
        .authenticated_input_note(note.id())
        .extend_note_args(BTreeMap::from([(note.id(), get_value_root)]))
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);

    let stored_root = chain
        .committed_account(account.id())
        .unwrap()
        .storage()
        .get_item(&handler_slot)
        .expect("the handler slot must exist");
    assert_eq!(
        stored_root, get_value_root,
        "`set_handler` must commit the root of the lifted `get-value` export unchanged"
    );
}

/// Dispatches a call that carries arguments and checks the order of the field elements.
///
/// The `weighted_handler` slot holds the root of the example's own `weighted-sum` export, which
/// weighs each of its five input field elements differently. The note asserts the one sum that the
/// declared order gives, so a call that flattens the word in another order, or that puts `scale`
/// before the word, produces another sum and stops the transaction.
#[test]
fn dispatch_passes_the_arguments_in_the_declared_order() {
    let example_package = compile_rust_package(example_root("stored-procedure-example"), true);
    let note_package =
        build_caller_note_package("stored_procedure_weighted_args", WEIGHTED_NOTE_SOURCE);

    // `weighted-sum` takes a word and a field element and returns one field element, which is the
    // signature the example's `weighted_handler` slot fixes.
    let weighted_sum_root = lifted_export_root(&example_package, "weighted-sum");

    let example_component = AccountComponent::from_package(
        &example_package,
        &example_init_storage(Word::empty(), weighted_sum_root),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let account = add_account(&mut builder, [4_u8; 32], vec![example_component]);

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

    // The note asserts the expected sum in the guest, so a transaction that executes at all proves
    // that every argument reached the callee at its declared position.
    let mock_tx = chain
        .build_transaction(account.clone())
        .authenticated_input_note(note.id())
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);
}

/// Dispatching through a slot that was never initialized stops the transaction.
#[test]
fn dispatch_through_an_uninitialized_slot_fails() {
    let example_package = compile_rust_package(example_root("stored-procedure-example"), true);
    // A generated project is keyed by its name, and the tests of this module run in parallel, so
    // every test needs its own name even when the sources agree.
    let note_package =
        build_caller_note_package("stored_procedure_uninitialized", DISPATCH_NOTE_SOURCE);

    let example_component = AccountComponent::from_package(
        &example_package,
        &example_init_storage(Word::empty(), Word::empty()),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let account = add_account(&mut builder, [3_u8; 32], vec![example_component]);

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

    let mock_tx = chain
        .build_transaction(account.clone())
        .authenticated_input_note(note.id())
        .build()
        .unwrap();
    let err = execute_tx_expect_failure(mock_tx);
    assert!(
        err.contains("stored procedure call: procedure root is zero"),
        "unexpected failure message: {err}"
    );
}
