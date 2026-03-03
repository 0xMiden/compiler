//! Counter contract test module

use miden_client::{
    Word,
    account::component::BasicWallet,
    crypto::RpoRandomCoin,
    note::NoteTag,
    testing::{AccountState, Auth, MockChain},
    transaction::OutputNote,
};
use miden_core::{Felt, FieldElement};
use miden_protocol::account::{
    AccountBuilder, AccountStorageMode, AccountType, StorageMap, StorageSlot, StorageSlotName,
};
use midenc_expect_test::expect;

use super::{
    cycle_helpers::note_cycles,
    helpers::{
        NoteCreationConfig, account_component_from_package, assert_counter_storage,
        compile_rust_package, create_note_from_package, execute_tx,
    },
};

/// Tests the counter contract deployment and note consumption workflow on a mock chain.
#[test]
pub fn test_counter_contract() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);

    let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let counter_storage_slot =
        StorageSlotName::new("miden::component::miden_counter_contract::count_map").unwrap();
    let storage_slots = vec![StorageSlot::with_map(
        counter_storage_slot.clone(),
        StorageMap::with_entries([(key, value)]).unwrap(),
    )];

    let counter_component = account_component_from_package(contract_package, storage_slots);
    let counter_account_builder = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(BasicWallet)
        .with_component(counter_component);

    let mut builder = MockChain::builder();
    let counter_account = builder
        .add_account_from_builder(Auth::BasicAuth, counter_account_builder, AccountState::Exists)
        .expect("failed to add counter account to mock chain builder");

    let mut rng = RpoRandomCoin::new(note_package.clone().unwrap_program().hash());
    let counter_note = create_note_from_package(
        note_package,
        counter_account.id(),
        NoteCreationConfig {
            tag: NoteTag::with_account_target(counter_account.id()),
            ..Default::default()
        },
        &mut rng,
    );
    builder.add_output_note(OutputNote::Full(counter_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("Counter account ID: {:?}", counter_account.id().to_hex());

    // The counter contract storage value should be 1 after account creation (initialized to 1).
    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        1,
    );

    // Consume the note to increment the counter
    let tx_context_builder = chain
        .build_tx_context(counter_account.clone(), &[counter_note.id()], &[])
        .unwrap();
    let tx_measurements = execute_tx(&mut chain, tx_context_builder);
    expect!["11916"].assert_eq(note_cycles(&tx_measurements, counter_note.id()));

    // The counter contract storage value should be 2 after the note is consumed (incremented by 1).
    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        2,
    );
}
