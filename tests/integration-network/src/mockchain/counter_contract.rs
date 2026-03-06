//! Counter contract test module

use miden_client::{
    account::component::{BasicWallet, InitStorageData},
    crypto::RpoRandomCoin,
    testing::{Auth, MockChain, NoteBuilder},
    transaction::OutputNote,
    Word,
};
use miden_core::{Felt, FieldElement};
use miden_protocol::account::StorageSlotName;

use super::helpers::{assert_counter_storage, execute_tx};
use crate::mockchain::helpers::{CustomComponentBuilder, PackageFromProject};

/// Tests the counter contract deployment and note consumption workflow on a mock chain.
#[test]
pub fn test_counter_contract() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = CustomComponentBuilder::with_package("../../examples/counter-contract");
    let note_package = NoteBuilder::build_project("../../examples/counter-note");

    let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let counter_storage_slot =
        StorageSlotName::new("miden::component::miden_counter_contract::count_map").unwrap();

    let mut init_storage_data = InitStorageData::default();
    init_storage_data
        .insert_map_entry(counter_storage_slot.clone(), key, value)
        .unwrap();
    let contract_package = contract_package.with_init_storage_data(init_storage_data).build();

    let mut builder = MockChain::builder();
    let counter_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth,
            [BasicWallet.into(), contract_package.into()],
            [],
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
    execute_tx(&mut chain, tx_context_builder);

    // The counter contract storage value should be 2 after the note is consumed (incremented by 1).
    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        2,
    );
}
