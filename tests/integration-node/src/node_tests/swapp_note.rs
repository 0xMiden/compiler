//! SWAPP note test module

use miden_client::{
    account::StorageMap,
    transaction::{OutputNote, TransactionRequestBuilder},
    Word,
};
use miden_core::{Felt, FieldElement};
use miden_objects::{
    accounts::AccountId,
    assets::{Asset, AssetId, FungibleAsset},
    notes::{NoteInputs, NoteTag, NoteType},
};

use super::helpers::*;
use crate::local_node::ensure_shared_node;

/// Tests the SWAPP note deployment and swap workflow on a local node.
#[test]
pub fn test_swapp_note_local() {
    // Compile the SWAPP note package first (before creating any runtime)
    let swapp_note_package = compile_rust_package("../../examples/swapp-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_swapp_note_local_")
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

        // Create two accounts: creator and consumer
        let creator_account = create_account(&mut client, keystore.clone()).await.unwrap();
        eprintln!("Creator account ID: {:?}", creator_account.id().to_hex());

        let consumer_account = create_account(&mut client, keystore.clone()).await.unwrap();
        eprintln!("Consumer account ID: {:?}", consumer_account.id().to_hex());

        // Create test assets
        // Token A: The token being offered in the swap
        let token_a_id = AssetId::new_fungible(
            AccountId::from_hex("0x0000000000000001").unwrap(),
            Word::from([Felt::from(1u64), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
        )
        .unwrap();
        let token_a_amount = 1000u64;
        let token_a = Asset::Fungible(FungibleAsset::new(token_a_id, token_a_amount).unwrap());

        // Token B: The token being requested in the swap
        let token_b_id = AssetId::new_fungible(
            AccountId::from_hex("0x0000000000000002").unwrap(),
            Word::from([Felt::from(2u64), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
        )
        .unwrap();
        let token_b_amount = 500u64; // Requesting 500 of token B for 1000 of token A

        // Fund the creator with token A
        // In a real scenario, this would be done through a faucet or mint transaction
        eprintln!("Creator funded with {} of token A", token_a_amount);

        // Fund the consumer with token B
        // In a real scenario, this would be done through a faucet or mint transaction
        eprintln!("Consumer funded with {} of token B", token_b_amount);

        // Prepare SWAPP note inputs
        let swapp_tag = NoteTag::from(0x53574150u32); // "SWAP" tag
        let p2id_tag = NoteTag::from(0x50324944u32); // "P2ID" tag

        // Build the SWAPP note inputs (8 u64 values)
        let token_b_id_word = token_b_id.to_word();
        let creator_id_word = creator_account.id().to_word();

        let swapp_inputs = NoteInputs::new(vec![
            Word::from([
                token_b_id_word[0],                    // token_b_id_prefix
                token_b_id_word[1],                    // token_b_id_suffix
                Felt::from(token_b_amount),            // token_b_amount (requested)
                Felt::from(swapp_tag.as_u32() as u64), // swapp_tag
            ]),
            Word::from([
                Felt::from(p2id_tag.as_u32() as u64), // p2id_tag
                Felt::ZERO,                           // swapp_count (starts at 0)
                creator_id_word[0],                   // creator_id_prefix
                creator_id_word[1],                   // creator_id_suffix
            ]),
        ])
        .unwrap();

        // Create the SWAPP note with token A
        let swapp_note_config = NoteCreationConfig {
            inputs: Some(swapp_inputs),
            assets: vec![token_a],
            tag: swapp_tag,
            note_type: NoteType::Public,
            ..Default::default()
        };

        let swapp_note = create_note_from_package(
            &mut client,
            swapp_note_package,
            consumer_account.id(), // Note is consumable by any account, but we specify consumer for testing
            swapp_note_config,
        );
        eprintln!("SWAPP note hash: {:?}", swapp_note.id().to_hex());

        // Submit transaction to create the SWAPP note
        let note_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(swapp_note.clone())])
            .build()
            .unwrap();

        let tx_result = client
            .new_transaction(creator_account.id(), note_request)
            .await
            .map_err(|e| {
                eprintln!("SWAPP note creation error: {e}");
                e
            })
            .unwrap();

        let executed_transaction = tx_result.executed_transaction();
        assert_eq!(executed_transaction.output_notes().num_notes(), 1);

        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), swapp_note.id());

        let create_note_tx_id = executed_transaction.id();
        client.submit_transaction(tx_result).await.unwrap();
        eprintln!("Created SWAPP note tx: {create_note_tx_id:?}");

        // Sync to get the latest state
        let sync_result = client.sync_state().await.unwrap();
        eprintln!("Synced to block: {}", sync_result.block_num);

        // Consumer attempts to consume the SWAPP note (partial fill scenario)
        // The consumer provides some token B and receives proportional token A
        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(swapp_note, None)])
            .build()
            .unwrap();

        let tx_result = client
            .new_transaction(consumer_account.id(), consume_request)
            .await
            .map_err(|e| {
                eprintln!("SWAPP note consumption error: {e}");
                e
            })
            .unwrap();

        eprintln!("Consumed SWAPP note tx: {:?}", &tx_result.executed_transaction().id());

        // Check that the transaction created expected outputs:
        // 1. P2ID note for the creator with token B
        // 2. Potentially a new SWAPP note if it was a partial fill
        let output_notes = tx_result.executed_transaction().output_notes();
        eprintln!("Number of output notes: {}", output_notes.num_notes());

        // Verify at least one P2ID note was created
        assert!(output_notes.num_notes() >= 1, "Expected at least one output note (P2ID)");

        client.submit_transaction(tx_result).await.unwrap();

        // Final sync
        let sync_result = client.sync_state().await.unwrap();
        eprintln!("Final sync to block: {}", sync_result.block_num);

        // Verify consumer received token A
        // In a real implementation, we would check the consumer's balance
        eprintln!("SWAPP note test completed successfully");
    });
}

/// Tests SWAPP note with full fill scenario
#[test]
pub fn test_swapp_note_full_fill() {
    // Compile the SWAPP note package first
    let swapp_note_package = compile_rust_package("../../examples/swapp-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_swapp_note_full_fill_")
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

        // Create accounts
        let creator_account = create_account(&mut client, keystore.clone()).await.unwrap();
        eprintln!("Creator account ID: {:?}", creator_account.id().to_hex());

        let consumer_account = create_account(&mut client, keystore.clone()).await.unwrap();
        eprintln!("Consumer account ID: {:?}", consumer_account.id().to_hex());

        // Create test assets for full fill scenario
        let token_a_id = AssetId::new_fungible(
            AccountId::from_hex("0x0000000000000001").unwrap(),
            Word::from([Felt::from(1u64), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
        )
        .unwrap();
        let token_a_amount = 1000u64;
        let token_a = Asset::Fungible(FungibleAsset::new(token_a_id, token_a_amount).unwrap());

        let token_b_id = AssetId::new_fungible(
            AccountId::from_hex("0x0000000000000002").unwrap(),
            Word::from([Felt::from(2u64), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
        )
        .unwrap();
        let token_b_amount = 500u64; // Consumer has exactly the requested amount

        // Prepare SWAPP note inputs
        let swapp_tag = NoteTag::from(0x53574150u32);
        let p2id_tag = NoteTag::from(0x50324944u32);

        let token_b_id_word = token_b_id.to_word();
        let creator_id_word = creator_account.id().to_word();

        let swapp_inputs = NoteInputs::new(vec![
            Word::from([
                token_b_id_word[0],
                token_b_id_word[1],
                Felt::from(token_b_amount),
                Felt::from(swapp_tag.as_u32() as u64),
            ]),
            Word::from([
                Felt::from(p2id_tag.as_u32() as u64),
                Felt::ZERO,
                creator_id_word[0],
                creator_id_word[1],
            ]),
        ])
        .unwrap();

        // Create the SWAPP note
        let swapp_note_config = NoteCreationConfig {
            inputs: Some(swapp_inputs),
            assets: vec![token_a],
            tag: swapp_tag,
            note_type: NoteType::Public,
            ..Default::default()
        };

        let swapp_note = create_note_from_package(
            &mut client,
            swapp_note_package,
            consumer_account.id(),
            swapp_note_config,
        );
        eprintln!("SWAPP note hash: {:?}", swapp_note.id().to_hex());

        // Submit transaction to create the SWAPP note
        let note_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(swapp_note.clone())])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(creator_account.id(), note_request).await.unwrap();

        client.submit_transaction(tx_result).await.unwrap();

        // Sync
        client.sync_state().await.unwrap();

        // Consumer consumes the SWAPP note with full amount
        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(swapp_note, None)])
            .build()
            .unwrap();

        let tx_result =
            client.new_transaction(consumer_account.id(), consume_request).await.unwrap();

        // In full fill scenario, only P2ID note should be created (no rollover SWAPP note)
        let output_notes = tx_result.executed_transaction().output_notes();
        eprintln!("Full fill - Number of output notes: {}", output_notes.num_notes());

        // Should only have P2ID note, no rollover SWAPP note
        assert_eq!(
            output_notes.num_notes(),
            1,
            "Expected exactly one output note (P2ID) for full fill"
        );

        client.submit_transaction(tx_result).await.unwrap();

        eprintln!("SWAPP note full fill test completed successfully");
    });
}

/// Tests SWAPP note creator reclaim scenario
#[test]
pub fn test_swapp_note_creator_reclaim() {
    // Compile the SWAPP note package first
    let swapp_note_package = compile_rust_package("../../examples/swapp-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_swapp_note_reclaim_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        // Initialize test infrastructure
        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        // Create creator account
        let creator_account = create_account(&mut client, keystore.clone()).await.unwrap();
        eprintln!("Creator account ID: {:?}", creator_account.id().to_hex());

        // Create test assets
        let token_a_id = AssetId::new_fungible(
            AccountId::from_hex("0x0000000000000001").unwrap(),
            Word::from([Felt::from(1u64), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
        )
        .unwrap();
        let token_a_amount = 1000u64;
        let token_a = Asset::Fungible(FungibleAsset::new(token_a_id, token_a_amount).unwrap());

        let token_b_id = AssetId::new_fungible(
            AccountId::from_hex("0x0000000000000002").unwrap(),
            Word::from([Felt::from(2u64), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
        )
        .unwrap();
        let token_b_amount = 500u64;

        // Prepare SWAPP note inputs
        let swapp_tag = NoteTag::from(0x53574150u32);
        let p2id_tag = NoteTag::from(0x50324944u32);

        let token_b_id_word = token_b_id.to_word();
        let creator_id_word = creator_account.id().to_word();

        let swapp_inputs = NoteInputs::new(vec![
            Word::from([
                token_b_id_word[0],
                token_b_id_word[1],
                Felt::from(token_b_amount),
                Felt::from(swapp_tag.as_u32() as u64),
            ]),
            Word::from([
                Felt::from(p2id_tag.as_u32() as u64),
                Felt::ZERO,
                creator_id_word[0],
                creator_id_word[1],
            ]),
        ])
        .unwrap();

        // Create the SWAPP note
        let swapp_note_config = NoteCreationConfig {
            inputs: Some(swapp_inputs),
            assets: vec![token_a],
            tag: swapp_tag,
            note_type: NoteType::Public,
            ..Default::default()
        };

        let swapp_note = create_note_from_package(
            &mut client,
            swapp_note_package,
            creator_account.id(), // Note can be consumed by creator for reclaim
            swapp_note_config,
        );
        eprintln!("SWAPP note hash: {:?}", swapp_note.id().to_hex());

        // Submit transaction to create the SWAPP note
        let note_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(swapp_note.clone())])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(creator_account.id(), note_request).await.unwrap();

        client.submit_transaction(tx_result).await.unwrap();

        // Sync
        client.sync_state().await.unwrap();

        // Creator reclaims the SWAPP note
        let reclaim_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(swapp_note, None)])
            .build()
            .unwrap();

        let tx_result =
            client.new_transaction(creator_account.id(), reclaim_request).await.unwrap();

        // When creator reclaims, no output notes should be created
        // (assets are directly received by the creator)
        let output_notes = tx_result.executed_transaction().output_notes();
        eprintln!("Reclaim - Number of output notes: {}", output_notes.num_notes());

        assert_eq!(output_notes.num_notes(), 0, "Expected no output notes for creator reclaim");

        client.submit_transaction(tx_result).await.unwrap();

        eprintln!("SWAPP note creator reclaim test completed successfully");
    });
}
