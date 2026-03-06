//! Counter contract test with no-auth authentication component

use miden_client::{
    Word,
    account::component::{BasicWallet, InitStorageData},
    crypto::RpoRandomCoin,
    note::NoteTag,
    testing::{AccountState, Auth, MockChain, NoteBuilder},
    transaction::OutputNote,
};
use miden_core::{Felt, FieldElement};
use miden_protocol::account::{AccountBuilder, AccountStorageMode, AccountType, StorageSlotName};

use super::helpers::{
    NoteCreationConfig, assert_counter_storage,
    build_existing_counter_account_builder_with_auth_package, create_note_from_package, execute_tx,
};
use crate::mockchain::helpers::{CustomComponentBuilder, PackageFromProject};

/// Tests the counter contract with a "no-auth" authentication component.
///
/// Flow:
/// - Build counter account using `examples/auth-component-no-auth` as the auth component
/// - Build a separate sender account (basic wallet)
/// - Sender issues a counter note to the network
/// - Counter account consumes the note without requiring authentication/signature
#[test]
pub fn test_counter_contract_no_auth() {
    // Compile the contracts first (before creating any runtime)
    let counter_component = CustomComponentBuilder::with_package("../../examples/counter-contract");
    let note_package = NoteBuilder::build_project("../../examples/counter-note");
    let no_auth_auth_component =
        CustomComponentBuilder::with_package("../../examples/auth-component-no-auth");

    let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let counter_storage_slot =
        StorageSlotName::new("miden::component::miden_counter_contract::count_map").unwrap();
    let mut builder = MockChain::builder();
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), key, value)
            .unwrap();
        let counter_component = counter_component.with_init_storage_data(init_storage_data);
        counter_component.build()
    };

    let no_auth_auth_component = no_auth_auth_component
        .with_init_storage_data(InitStorageData::default())
        .build();

    let mut counter_init_storage_data = InitStorageData::default();
    counter_init_storage_data
        .insert_map_entry(counter_storage_slot.clone(), key, value)
        .expect("failed to insert counter map entry");

    let counter_account = build_existing_counter_account_builder_with_auth_package(
        counter_component.package,
        no_auth_auth_component.package,
        vec![],
        counter_init_storage_data,
        [0_u8; 32],
    )
    .build_existing()
    .expect("failed to build counter account");
    builder
        .add_account(counter_account.clone())
        .expect("failed to add counter account to mock chain builder");
    eprintln!("Counter account (no-auth) ID: {:?}", counter_account.id().to_hex());

    // Create a separate sender account using only the BasicWallet component
    let seed = [1_u8; 32];
    let sender_builder = AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(BasicWallet);
    let sender_account = builder
        .add_account_from_builder(Auth::BasicAuth, sender_builder, AccountState::Exists)
        .expect("failed to add sender account to mock chain builder");
    eprintln!("Sender account ID: {:?}", sender_account.id().to_hex());

    // Sender creates the counter note (note script increments counter's storage on consumption)
    let rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
    let counter_note = NoteBuilder::new(sender_account.id(), rng)
        .package((*note_package).clone())
        .tag(NoteTag::with_account_target(counter_account.id()).into())
        .build()
        .unwrap();

    eprintln!("Counter note hash: {:?}", counter_note.id().to_hex());
    builder.add_output_note(OutputNote::Full(counter_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        1,
    );

    // Consume the note with the counter account (no signature/auth required).
    let tx_context_builder = chain
        .build_tx_context(counter_account.clone(), &[counter_note.id()], &[])
        .unwrap();
    execute_tx(&mut chain, tx_context_builder);

    // The counter contract storage value should be 2 after the note is consumed
    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        2,
    );
}
