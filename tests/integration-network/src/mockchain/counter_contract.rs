//! Counter contract test module

use miden_client::{
    account::component::BasicWallet,
    crypto::{RandomCoin, RpoRandomCoin},
    note::NoteTag,
    transaction::RawOutputNote,
    Word,
};
use miden_core::Felt;
use miden_protocol::account::{
    auth::AuthScheme, AccountBuilder, AccountStorageMode, AccountType, StorageMap, StorageMapKey,
    StorageSlot, StorageSlotName,
};
use miden_testing::{AccountState, Auth, MockChain};
use midenc_expect_test::expect;

use super::{
    cycle_helpers::note_cycles,
    helpers::{
        account_component_from_package, assert_counter_storage, compile_rust_package,
        counter_storage_slot_name, create_note_from_package, execute_tx, NoteCreationConfig,
    },
};
use crate::mockchain::helpers::COUNTER_CONTRACT_STORAGE_KEY;

/// Tests the counter contract deployment and note consumption workflow on a mock chain.
#[test]
pub fn test_counter_contract() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);

    let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let counter_storage_slot = counter_storage_slot_name();
    let storage_slots = vec![StorageSlot::with_map(
        counter_storage_slot.clone(),
        StorageMap::with_entries([(StorageMapKey::new(COUNTER_CONTRACT_STORAGE_KEY), value)])
            .unwrap(),
    )];

    let mut init_storage_data = InitStorageData::default();
    init_storage_data
        .insert_map_entry(counter_storage_slot.clone(), key, value)
        .unwrap();
    let contract_package = AccountComponent::from_package(&contract_package, &init_storage_data)
        .expect("Failed to build account component from counter project");

    let mut builder = MockChain::builder();
    let counter_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth,
            [BasicWallet.into(), contract_package],
        )
        .unwrap();

    let mut rng = RpoRandomCoin::new(note_package.clone().unwrap_program().hash());
    let counter_note = NoteBuilder::new(counter_account.id(), &mut rng)
        .package((*note_package).clone())
        .build()
        .unwrap();
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
    expect!["28731"].assert_eq(note_cycles(&tx_measurements, counter_note.id()));

    // The counter contract storage value should be 2 after the note is consumed (incremented by 1).
    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        2,
    );
}
