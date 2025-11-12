//! Counter contract test module

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

    // The counter contract is in slot 1 when deployed, auth_component takes slot 0
    let word = counter_account_storage
        .get_map_item(1, counter_contract_storage_key)
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

/// Tests the counter contract deployment and note consumption workflow on a local node.
#[test]
pub fn test_counter_contract_local() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_counter_contract_local_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        // Initialize test infrastructure
        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        let sync_summary = client.sync_state().await.unwrap();
        eprintln!("Latest block: {}", sync_summary.block_num);

        // Create the counter account with initial storage
        let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let config = AccountCreationConfig {
            storage_slots: vec![miden_client::account::StorageSlot::Map(
                StorageMap::with_entries([(key, value)]).unwrap(),
            )],
            ..Default::default()
        };

        let counter_account =
            create_account_with_component(&mut client, keystore.clone(), contract_package, config)
                .await
                .unwrap();
        eprintln!("Counter account ID: {:?}", counter_account.id().to_hex());

        // The counter contract storage value should be zero after the account creation
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

        // Create the counter note from sender to counter
        let counter_note = create_note_from_package(
            &mut client,
            note_package,
            counter_account.id(),
            NoteCreationConfig::default(),
        );
        eprintln!("Counter note hash: {:?}", counter_note.id().to_hex());

        // Submit transaction to create the note
        let note_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(counter_note.clone())])
            .build()
            .unwrap();

        let tx_result = client
            .execute_transaction(counter_account.id(), note_request)
            .await
            .map_err(|e| {
                eprintln!("Transaction creation error: {e}");
                e
            })
            .unwrap();
        let executed_transaction = tx_result.executed_transaction();
        // dbg!(executed_transaction.output_notes());

        assert_eq!(executed_transaction.output_notes().num_notes(), 1);

        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), counter_note.id());
        let create_note_tx_id = executed_transaction.id();
        let proven_tx = client.prove_transaction(&tx_result).await.unwrap();
        let submission_height = client
            .submit_proven_transaction(proven_tx, tx_result.tx_inputs().clone())
            .await
            .unwrap();
        client.apply_transaction(&tx_result, submission_height).await.unwrap();
        eprintln!("Created counter note tx: {create_note_tx_id:?}");

        // Consume the note to increment the counter
        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(counter_note, None)])
            .build()
            .unwrap();

        let tx_result = client
            .execute_transaction(counter_account.id(), consume_request)
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

        let proven_tx = client.prove_transaction(&tx_result).await.unwrap();
        let submission_height = client
            .submit_proven_transaction(proven_tx, tx_result.tx_inputs().clone())
            .await
            .unwrap();
        client.apply_transaction(&tx_result, submission_height).await.unwrap();

        let sync_result = client.sync_state().await.unwrap();
        eprintln!("Synced to block: {}", sync_result.block_num);

        // The counter contract storage value should be 1 (incremented) after the note is consumed
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
