//! Basic wallet test module

use miden_client::{
    asset::{FungibleAsset, TokenSymbol},
    note::NoteAssets,
    transaction::{OutputNote, TransactionRequestBuilder},
};
use miden_core::{Felt, utils::Serializable};

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
                inputs: account_id_inputs(&alice_account.id()),
                ..Default::default()
            },
        );
        eprintln!("P2ID mint note hash: {:?}", p2id_note_mint.id().to_hex());

        let mint_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
            .build()
            .unwrap();

        let mint_tx_id =
            client.submit_new_transaction(faucet_account.id(), mint_request).await.unwrap();
        eprintln!("Submitted mint transaction. Tx ID: {mint_tx_id:?}");

        eprintln!("\n=== Step 2: Alice attempts to consume mint note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2id_note_mint, None)])
            .build()
            .unwrap();

        let _consume_tx_id = client
            .submit_new_transaction(alice_account.id(), consume_request)
            .await
            .map_err(|e| format!("{e:?}"))
            .unwrap();

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

        let consume_tx_id =
            client.submit_new_transaction(bob_account.id(), consume_request).await.unwrap();
        eprintln!("Bob created consume transaction. Tx ID: {consume_tx_id:?}");

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

/// Tests the basic-wallet contract deployment and p2ide note consumption workflow on a local node.
///
/// Flow:
/// - Create fungible faucet and mint tokens to Alice
/// - Alice creates a p2ide note for Bob (with timelock=0, reclaim=0)
/// - Bob consumes the p2ide note and receives the assets
#[test]
pub fn test_basic_wallet_p2ide_local() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let p2id_note_package = compile_rust_package("../../examples/p2id-note", true);
    let p2ide_note_package = compile_rust_package("../../examples/p2ide-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_basic_wallet_p2ide_local_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        // Initialize test infrastructure
        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        // Step 1: Create a fungible faucet account
        eprintln!("\n=== Step 1: Creating fungible faucet ===");
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
            with_basic_wallet: true,
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

        // Step 2: Mint assets from faucet to Alice using p2id note
        eprintln!("\n=== Step 2: Minting tokens from faucet to Alice (p2id note) ===");

        let mint_amount = 100_000u64; // 100,000 tokens
        let fungible_asset = FungibleAsset::new(faucet_account.id(), mint_amount).unwrap();

        // Create the p2id note from faucet to Alice
        let p2id_note_mint = create_note_from_package(
            &mut client,
            p2id_note_package.clone(),
            faucet_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![fungible_asset.into()]).unwrap(),
                inputs: account_id_inputs(&alice_account.id()),
                ..Default::default()
            },
        );
        eprintln!("P2ID mint note hash: {:?}", p2id_note_mint.id().to_hex());

        let mint_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
            .build()
            .unwrap();

        let mint_tx_id =
            client.submit_new_transaction(faucet_account.id(), mint_request).await.unwrap();
        eprintln!("Submitted mint transaction. Tx ID: {mint_tx_id:?}");

        // Step 3: Alice consumes the p2id note
        eprintln!("\n=== Step 3: Alice consumes p2id mint note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2id_note_mint, None)])
            .build()
            .unwrap();

        let _consume_tx_id = client
            .submit_new_transaction(alice_account.id(), consume_request)
            .await
            .map_err(|e| format!("{e:?}"))
            .unwrap();

        eprintln!("\n=== Checking Alice's account has the minted asset ===");

        assert_account_has_fungible_asset(
            &mut client,
            alice_account.id(),
            faucet_account.id(),
            mint_amount,
        )
        .await;

        // Create Bob's account
        eprintln!("\n=== Creating Bob's account ===");

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

        // Step 4: Alice creates p2ide note for Bob
        eprintln!("\n=== Step 4: Alice creates p2ide note for Bob ===");

        let transfer_amount = 10_000u64; // 10,000 tokens
        let transfer_asset = FungibleAsset::new(faucet_account.id(), transfer_amount).unwrap();

        let timelock_height = Felt::new(0);
        let reclaim_height = Felt::new(0);

        // Create the p2ide note
        let p2ide_note = create_note_from_package(
            &mut client,
            p2ide_note_package.clone(),
            alice_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![transfer_asset.into()]).unwrap(),
                inputs: {
                    let mut inputs: Vec<Felt> = account_id_inputs(&bob_account.id());
                    inputs.extend([timelock_height, reclaim_height]);
                    inputs
                },
                ..Default::default()
            },
        );
        eprintln!("P2IDE note hash: {:?}", p2ide_note.id().to_hex());

        let transfer_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2ide_note.clone())])
            .build()
            .unwrap();

        let alice_tx_id = client
            .submit_new_transaction(alice_account.id(), transfer_request)
            .await
            .unwrap();
        eprintln!("Submitted p2ide transaction. Tx ID: {alice_tx_id:?}");

        // Step 5: Bob consumes the p2ide note
        eprintln!("\n=== Step 5: Bob consumes p2ide note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2ide_note, None)])
            .build()
            .unwrap();

        let consume_tx_id =
            client.submit_new_transaction(bob_account.id(), consume_request).await.unwrap();
        eprintln!("Bob created consume transaction. Tx ID: {consume_tx_id:?}");

        eprintln!("\n=== Checking Bob's account has the transferred asset ===");

        assert_account_has_fungible_asset(
            &mut client,
            bob_account.id(),
            faucet_account.id(),
            transfer_amount,
        )
        .await;

        eprintln!("\n=== Checking Alice's account reflects the new token amount ===");

        assert_account_has_fungible_asset(
            &mut client,
            alice_account.id(),
            faucet_account.id(),
            mint_amount - transfer_amount,
        )
        .await;
    });
}

/// Tests the p2ide note reclaim functionality.
///
/// Flow:
/// - Create fungible faucet and mint tokens to Alice
/// - Alice creates a p2ide note intended for Bob (with reclaim enabled)
/// - Alice reclaims the note herself (exercises the reclaim branch)
/// - Verify Alice has her original balance back
#[test]
pub fn test_basic_wallet_p2ide_reclaim_local() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let p2id_note_package = compile_rust_package("../../examples/p2id-note", true);
    let p2ide_note_package = compile_rust_package("../../examples/p2ide-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_basic_wallet_p2ide_reclaim_local_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        // Initialize test infrastructure
        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        // Step 1: Create a fungible faucet account
        eprintln!("\n=== Step 1: Creating fungible faucet ===");
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
            with_basic_wallet: true,
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

        // Step 2: Mint assets from faucet to Alice using p2id note
        eprintln!("\n=== Step 2: Minting tokens from faucet to Alice (p2id note) ===");

        let mint_amount = 100_000u64; // 100,000 tokens
        let fungible_asset = FungibleAsset::new(faucet_account.id(), mint_amount).unwrap();

        // Create the p2id note from faucet to Alice
        let p2id_note_mint = create_note_from_package(
            &mut client,
            p2id_note_package.clone(),
            faucet_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![fungible_asset.into()]).unwrap(),
                inputs: account_id_inputs(&alice_account.id()),
                ..Default::default()
            },
        );
        eprintln!("P2ID mint note hash: {:?}", p2id_note_mint.id().to_hex());

        let mint_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
            .build()
            .unwrap();

        let mint_tx_id =
            client.submit_new_transaction(faucet_account.id(), mint_request).await.unwrap();
        eprintln!("Submitted mint transaction. Tx ID: {mint_tx_id:?}");

        // Step 3: Alice consumes the p2id note
        eprintln!("\n=== Step 3: Alice consumes p2id mint note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2id_note_mint, None)])
            .build()
            .unwrap();

        let _consume_tx_id = client
            .submit_new_transaction(alice_account.id(), consume_request)
            .await
            .map_err(|e| format!("{e:?}"))
            .unwrap();

        eprintln!("\n=== Checking Alice's account has the minted asset ===");

        assert_account_has_fungible_asset(
            &mut client,
            alice_account.id(),
            faucet_account.id(),
            mint_amount,
        )
        .await;

        // Create Bob's account
        eprintln!("\n=== Creating Bob's account ===");

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

        // Step 4: Alice creates p2ide note for Bob with reclaim enabled
        eprintln!("\n=== Step 4: Alice creates p2ide note for Bob with reclaim ===");

        let transfer_amount = 10_000u64; // 10,000 tokens
        let transfer_asset = FungibleAsset::new(faucet_account.id(), transfer_amount).unwrap();

        // Set timelock to 0 (no timelock) and reclaim height to a future block
        // This allows Alice to reclaim if she consumes the note herself
        let timelock_height = Felt::new(0);
        let reclaim_height = Felt::new(1000); // Future block height

        // Create the p2ide note
        let p2ide_note = create_note_from_package(
            &mut client,
            p2ide_note_package.clone(),
            alice_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![transfer_asset.into()]).unwrap(),
                inputs: {
                    let mut inputs: Vec<Felt> = account_id_inputs(&bob_account.id());
                    inputs.extend([timelock_height, reclaim_height]);
                    inputs
                },
                ..Default::default()
            },
        );
        eprintln!("P2IDE note hash: {:?}", p2ide_note.id().to_hex());

        let transfer_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2ide_note.clone())])
            .build()
            .unwrap();

        let alice_tx_id = client
            .submit_new_transaction(alice_account.id(), transfer_request)
            .await
            .unwrap();
        eprintln!("Submitted p2ide transaction. Tx ID: {alice_tx_id:?}");

        // Step 5: Alice reclaims the note (exercises the reclaim branch)
        eprintln!("\n=== Step 5: Alice reclaims the p2ide note ===");

        let reclaim_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2ide_note, None)])
            .build()
            .unwrap();

        let reclaim_tx_id = client
            .submit_new_transaction(alice_account.id(), reclaim_request)
            .await
            .unwrap();
        eprintln!("Alice created reclaim transaction. Tx ID: {reclaim_tx_id:?}");

        eprintln!("\n=== Checking Alice's account has reclaimed the asset ===");

        // Alice should have her original amount back (mint_amount)
        // because she reclaimed the note instead of Bob consuming it
        assert_account_has_fungible_asset(
            &mut client,
            alice_account.id(),
            faucet_account.id(),
            mint_amount,
        )
        .await;

        eprintln!("\n=== Test completed: Alice successfully reclaimed the p2ide note ===");
    });
}
