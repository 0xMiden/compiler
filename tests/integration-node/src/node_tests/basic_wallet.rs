//! Basic wallet test module

use miden_client::{
    asset::{FungibleAsset, TokenSymbol},
    note::NoteAssets,
    transaction::{OutputNote, TransactionRequestBuilder},
};
use miden_core::{utils::Serializable, Felt};

use super::helpers::*;
use crate::local_node::ensure_shared_node;

/// Tests the basic-wallet contract deployment and p2id note consumption workflow on a local node.
#[test]
pub fn test_basic_wallet_p2id_local() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let tx_script_package = compile_rust_package("../../examples/basic-wallet-tx-script", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_basic_wallet_p2id_local_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        // Initialize test infrastructure
        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        // Write wallet package to disk for potential future use
        let wallet_package_path = temp_dir.path().join("basic_wallet.masp");
        std::fs::write(&wallet_package_path, wallet_package.to_bytes())
            .expect("Failed to write wallet");

        // Create a fungible faucet account
        let token_symbol = TokenSymbol::new("TEST").unwrap();
        let decimals = 8u8;
        let max_supply = Felt::new(1_000_000_000); // 1 billion tokens

        let faucet_account = create_fungible_faucet_account(
            &mut client,
            keystore.clone(),
            token_symbol,
            decimals,
            max_supply,
        )
        .await
        .unwrap();

        eprintln!("Faucet account ID: {:?}", faucet_account.id().to_hex());

        // Create Alice's account with basic-wallet component
        let alice_config = AccountCreationConfig {
            with_basic_wallet: false,
            ..Default::default()
        };
        let alice_account = create_account_with_component(
            &mut client,
            keystore.clone(),
            wallet_package.clone(),
            alice_config,
        )
        .await
        .unwrap();
        eprintln!("Alice account ID: {:?}", alice_account.id().to_hex());

        eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");

        let mint_amount = 100_000u64; // 100,000 tokens
        let fungible_asset = FungibleAsset::new(faucet_account.id(), mint_amount).unwrap();

        // Create the p2id note from faucet to Alice
        let p2id_note_mint = create_note_from_package(
            &mut client,
            note_package.clone(),
            faucet_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![fungible_asset.into()]).unwrap(),
                inputs: vec![alice_account.id().prefix().as_felt(), alice_account.id().suffix()],
                ..Default::default()
            },
        );
        eprintln!("P2ID mint note hash: {:?}", p2id_note_mint.id().to_hex());

        let mint_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
            .build()
            .unwrap();

        let mint_tx_result =
            client.new_transaction(faucet_account.id(), mint_request).await.unwrap();
        let mint_tx_id = mint_tx_result.executed_transaction().id();
        eprintln!("Created mint transaction. Tx ID: {mint_tx_id:?}");

        client.submit_transaction(mint_tx_result).await.unwrap();
        eprintln!("Submitted mint transaction. Tx ID: {mint_tx_id:?}");

        eprintln!("\n=== Step 2: Alice attempts to consume mint note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2id_note_mint, None)])
            .build()
            .unwrap();

        let consume_tx = client
            .new_transaction(alice_account.id(), consume_request)
            .await
            .map_err(|e| format!("{e:?}"))
            .unwrap();

        client.submit_transaction(consume_tx).await.unwrap();

        eprintln!("\n=== Checking Alice's account has the minted asset ===");

        assert_account_has_fungible_asset(
            &mut client,
            alice_account.id(),
            faucet_account.id(),
            mint_amount,
        )
        .await;

        eprintln!("\n=== Step 3: Creating Bob's account ===");

        let bob_config = AccountCreationConfig {
            with_basic_wallet: false,
            ..Default::default()
        };
        let bob_account = create_account_with_component(
            &mut client,
            keystore.clone(),
            wallet_package,
            bob_config,
        )
        .await
        .unwrap();
        eprintln!("Bob account ID: {:?}", bob_account.id().to_hex());

        eprintln!("\n=== Step 4: Alice creates p2id note for Bob ===");

        let transfer_amount = 10_000u64; // 10,000 tokens
        let transfer_asset = FungibleAsset::new(faucet_account.id(), transfer_amount).unwrap();

        let (alice_tx_id, bob_note) = send_asset_to_account(
            &mut client,
            alice_account.id(),
            bob_account.id(),
            transfer_asset,
            note_package.clone(),
            tx_script_package,
            None, // Use default configuration
        )
        .await
        .unwrap();

        eprintln!("Alice created p2id transaction. Tx ID: {alice_tx_id:?}");

        // Step 5: Bob attempts to consume the p2id note
        eprintln!("\n=== Step 5: Bob attempts to consume p2id note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(bob_note, None)])
            .build()
            .unwrap();

        let consume_tx = client.new_transaction(bob_account.id(), consume_request).await.unwrap();
        let consume_tx_id = consume_tx.executed_transaction().id();
        eprintln!("Bob created consume transaction. Tx ID: {consume_tx_id:?}");

        client.submit_transaction(consume_tx).await.unwrap();

        eprintln!("\n=== Step 6: Checking Bob's account has the transferred asset ===");

        assert_account_has_fungible_asset(
            &mut client,
            bob_account.id(),
            faucet_account.id(),
            transfer_amount,
        )
        .await;

        eprintln!(
            "\n=== Step 7: Checking Alice's account reflects the new token amount after sending \
             to Bob ==="
        );

        assert_account_has_fungible_asset(
            &mut client,
            alice_account.id(),
            faucet_account.id(),
            mint_amount - transfer_amount,
        )
        .await;
    });
}
