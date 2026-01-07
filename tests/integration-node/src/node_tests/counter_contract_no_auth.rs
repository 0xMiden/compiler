//! Counter contract test with no-auth authentication component

use miden_client::{
    Word,
    account::component::BasicWallet,
    crypto::RpoRandomCoin,
    note::NoteTag,
    testing::{AccountState, Auth, MockChain},
    transaction::OutputNote,
};
use miden_core::{Felt, FieldElement};
use miden_objects::account::{
    AccountBuilder, AccountStorageMode, AccountType, StorageMap, StorageSlot,
};

use super::helpers::{
    NoteCreationConfig, assert_counter_storage,
    build_existing_counter_account_builder_with_auth_package, compile_rust_package,
    create_note_from_package, execute_tx,
};

/// Tests the counter contract with a "no-auth" authentication component.
///
/// Flow:
/// - Build counter account using `examples/auth-component-no-auth` as the auth component
/// - Build a separate sender account (basic wallet)
/// - Sender issues a counter note to the network
/// - Counter account consumes the note without requiring authentication/signature
#[test]
pub fn test_counter_contract_no_auth_mockchain() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);
    let no_auth_auth_component =
        compile_rust_package("../../examples/auth-component-no-auth", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let counter_storage_slots =
            vec![StorageSlot::Map(StorageMap::with_entries([(key, value)]).unwrap())];

        let mut builder = MockChain::builder();

        let counter_account = build_existing_counter_account_builder_with_auth_package(
            contract_package,
            no_auth_auth_component,
            vec![],
            counter_storage_slots,
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
        let mut rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
        let counter_note = create_note_from_package(
            note_package.clone(),
            sender_account.id(),
            NoteCreationConfig {
                tag: NoteTag::from_account_id(counter_account.id()),
                ..Default::default()
            },
            &mut rng,
        );
        eprintln!("Counter note hash: {:?}", counter_note.id().to_hex());
        builder.add_output_note(OutputNote::Full(counter_note.clone()));

        let mut chain = builder.build().expect("failed to build mock chain");
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        assert_counter_storage(
            chain.committed_account(counter_account.id()).unwrap().storage(),
            0,
            1,
        );

        // Consume the note with the counter account (no signature/auth required).
        let tx_context_builder = chain
            .build_tx_context(counter_account.clone(), &[counter_note.id()], &[])
            .unwrap();
        execute_tx(&mut chain, tx_context_builder).await;

        // The counter contract storage value should be 2 after the note is consumed
        assert_counter_storage(
            chain.committed_account(counter_account.id()).unwrap().storage(),
            0,
            2,
        );
    });
}
