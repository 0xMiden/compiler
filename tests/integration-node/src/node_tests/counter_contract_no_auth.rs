//! Counter contract test with no-auth authentication component

use miden_client::{
    account::StorageMap,
    transaction::{OutputNote, TransactionRequestBuilder},
    Word,
};
use miden_core::{Felt, FieldElement};

use super::helpers::*;
use crate::local_node::ensure_shared_node;

fn assert_counter_storage(
    counter_account_storage: &miden_client::account::AccountStorage,
    expected: u64,
) {
    // according to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

    // With no-auth auth component (no storage), the counter component occupies slot 0
    let word = counter_account_storage
        .get_map_item(0, counter_contract_storage_key)
        .expect("Failed to get counter value from storage slot 1");

    let val = word.last().unwrap();
    assert_eq!(
        val.as_int(),
        expected,
        "Counter value mismatch. Expected: {}, Got: {}",
        expected,
        val.as_int()
    );
}

/// Tests the counter contract with a "no-auth" authentication component.
///
/// Flow:
/// - Build counter account using `examples/auth-component-no-auth` as the auth component
/// - Build a separate sender account (basic wallet)
/// - Sender issues a counter note to the network
/// - Counter account consumes the note without requiring authentication/signature
#[test]
pub fn test_counter_contract_no_auth_local() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);
    let no_auth_auth_component =
        compile_rust_package("../../examples/auth-component-no-auth", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let temp_dir = temp_dir::TempDir::with_prefix("test_counter_contract_no_auth_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        let sync_summary = client.sync_state().await.unwrap();
        eprintln!("Latest block: {}", sync_summary.block_num);

        // Create the counter account with initial storage and no-auth auth component
        let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let counter_cfg = AccountCreationConfig {
            storage_slots: vec![miden_client::account::StorageSlot::Map(
                StorageMap::with_entries([(key.into(), value)]).unwrap(),
            )],
            ..Default::default()
        };

        let counter_account = create_account_with_component_and_auth_package(
            &mut client,
            contract_package.clone(),
            no_auth_auth_component.clone(),
            counter_cfg,
        )
        .await
        .unwrap();
        eprintln!("Counter account (no-auth) ID: {:?}", counter_account.id().to_hex());

        assert_counter_storage(
            client
                .get_account(counter_account.id())
                .await
                .unwrap()
                .unwrap()
                .account()
                .storage(),
            1,
        );

        // Create a separate sender account using only the BasicWallet component
        let sender_cfg = AccountCreationConfig::default();
        let sender_account =
            create_basic_wallet_account(&mut client, keystore.clone(), sender_cfg)
        .await
        .unwrap();
        eprintln!("Sender account ID: {:?}", sender_account.id().to_hex());

        // Sender creates the counter note (note script increments counter's storage on consumption)
        let counter_note = create_note_from_package(
            &mut client,
            note_package.clone(),
            sender_account.id(),
            NoteCreationConfig::default(),
        );
        eprintln!("Counter note hash: {:?}", counter_note.id().to_hex());

        // Submit transaction to create the note from the sender account
        let note_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(counter_note.clone())])
            .build()
            .unwrap();

        let tx_result = client
            .new_transaction(sender_account.id(), note_request)
            .await
            .map_err(|e| {
                eprintln!("Transaction creation error: {e}");
                e
            })
            .unwrap();
        let executed_transaction = tx_result.executed_transaction();
        assert_eq!(executed_transaction.output_notes().num_notes(), 1);
        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), counter_note.id());
        let create_note_tx_id = executed_transaction.id();
        client.submit_transaction(tx_result).await.unwrap();
        eprintln!("Created counter note tx: {create_note_tx_id:?}");

        // Consume the note with the counter account
        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(counter_note, None)])
            .build()
            .unwrap();

        let tx_result = client
            .new_transaction(counter_account.id(), consume_request)
            .await
            .map_err(|e| {
                eprintln!("Note consumption transaction error: {e}");
                e
            })
            .unwrap();
        eprintln!(
            "Consumed counter note tx: https://testnet.midenscan.com/tx/{:?}",
            &tx_result.executed_transaction().id()
        );

        client.submit_transaction(tx_result).await.unwrap();

        let sync_result = client.sync_state().await.unwrap();
        eprintln!("Synced to block: {}", sync_result.block_num);

        // The counter contract storage value should be 2 after the note is consumed
        assert_counter_storage(
            client
                .get_account(counter_account.id())
                .await
                .unwrap()
                .unwrap()
                .account()
                .storage(),
            2,
        );
    });
}
