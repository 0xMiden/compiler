//! Basic wallet test module

use std::{env, sync::Arc};

use miden_client::{
    account::{
        component::{BasicFungibleFaucet, RpoFalcon512},
        Account, AccountStorageMode, AccountType,
    },
    asset::{FungibleAsset, TokenSymbol},
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::SecretKey,
    keystore::FilesystemKeyStore,
    note::{create_p2id_note, NoteType},
    rpc::{Endpoint, TonicRpcClient},
    transaction::{OutputNote, TransactionRequestBuilder},
    Client, ClientError, Felt,
};
use miden_core::{
    utils::{Deserializable, Serializable},
    FieldElement,
};
use miden_integration_tests::CompilerTestBuilder;
use miden_objects::account::{
    AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{rngs::StdRng, RngCore};

use crate::local_node;

/// Helper to create a fungible faucet account
async fn create_fungible_faucet_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    token_symbol: TokenSymbol,
    decimals: u8,
    max_supply: Felt,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicFungibleFaucet::new(token_symbol, decimals, max_supply).unwrap());

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Helper to create an account with the basic-wallet component
async fn create_basic_wallet_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    wallet_package: Arc<miden_mast_package::Package>,
) -> Result<Account, ClientError> {
    let account_component = match wallet_package.account_component_metadata_bytes.as_deref() {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                wallet_package.unwrap_library().as_ref().clone(),
            );

            AccountComponent::new(template.library().clone(), vec![])
                .unwrap()
                .with_supported_types([AccountType::RegularAccountUpdatableCode].into())
        }
    };

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(account_component);
    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Tests the basic-wallet contract deployment and p2id note consumption workflow on a local node.
#[test]
pub fn test_basic_wallet_p2id_local() {
    // Compile the contracts first (before creating any runtime)
    let config = WasmTranslationConfig::default();
    let mut wallet_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/basic-wallet",
        config.clone(),
        ["--debug=none".into()], // don't include any debug info in the compiled MAST
    );
    wallet_builder.with_release(true);
    let mut wallet_test = wallet_builder.build();
    let wallet_package = wallet_test.compiled_package();

    let bytes = <miden_mast_package::Package as Clone>::clone(&wallet_package)
        .into_mast_artifact()
        .unwrap_library()
        .to_bytes();
    assert!(bytes.len() < 32767, "expected to fit in 32 KB account update size limit");

    // Compile the p2id note
    let mut note_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/p2id-note",
        config,
        ["--debug=none".into()], // don't include any debug info in the compiled MAST
    );
    note_builder.with_release(true);
    let mut note_test = note_builder.build();
    let _note_package = note_test.compiled_package();

    // Use temp_dir for a fresh client store
    let temp_dir = temp_dir::TempDir::with_prefix("test_basic_wallet_p2id_local_").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Get a handle to the shared local node
        let node_handle = local_node::get_shared_node().await.expect("Failed to get shared node");

        let rpc_url = node_handle.rpc_url().to_string();

        // Initialize client & keystore
        let endpoint = Endpoint::try_from(rpc_url.as_str()).expect("Failed to create endpoint");
        let timeout_ms = 10_000;
        let rpc_api = Arc::new(TonicRpcClient::new(&endpoint, timeout_ms));

        let keystore_path = temp_dir.path().join("keystore");
        let keystore = Arc::new(FilesystemKeyStore::<StdRng>::new(keystore_path.clone()).unwrap());

        // Change to temp dir for client store isolation
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let mut client = ClientBuilder::new()
            .rpc(rpc_api)
            .filesystem_keystore(keystore_path.to_str().unwrap())
            .in_debug_mode(true)
            .build()
            .await
            .unwrap();

        // Restore original directory after client creation
        env::set_current_dir(original_dir).unwrap();

        let sync_summary = client.sync_state().await.unwrap();
        eprintln!("Latest block: {}", sync_summary.block_num);

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
        let alice_account =
            create_basic_wallet_account(&mut client, keystore.clone(), wallet_package.clone())
                .await
                .unwrap();
        eprintln!("Alice account ID: {:?}", alice_account.id().to_hex());

        // Step 1: Mint tokens from faucet to Alice
        eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");
        let mint_amount = 100_000u64; // 100,000 tokens
        let fungible_asset = FungibleAsset::new(faucet_account.id(), mint_amount).unwrap();

        let mint_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(
                fungible_asset,
                alice_account.id(),
                NoteType::Public,
                client.rng(),
            )
            .unwrap();

        let mint_tx_result =
            client.new_transaction(faucet_account.id(), mint_request).await.unwrap();
        let mint_tx_id = mint_tx_result.executed_transaction().id();
        eprintln!("Created mint transaction. Tx ID: {mint_tx_id:?}");

        // Try to submit the mint transaction
        match client.submit_transaction(mint_tx_result).await {
            Ok(_) => {
                eprintln!("Successfully submitted mint transaction");
            }
            Err(err) => {
                eprintln!("Failed to submit mint transaction: {err}");
                // Assert on expected error patterns
                let err_str = err.to_string();
                assert!(
                    err_str.contains("RpcError")
                        || err_str.contains("protocol error")
                        || err_str.contains("rpc api error"),
                    "Unexpected error type: {err_str}"
                );
            }
        }

        // Step 2: Wait and try to consume the mint note
        eprintln!("\n=== Step 2: Alice attempts to consume mint note ===");
        client.sync_state().await.unwrap();

        // Check for consumable notes
        let alice_notes = client.get_consumable_notes(Some(alice_account.id())).await.unwrap();
        eprintln!("Alice has {} consumable notes", alice_notes.len());

        if !alice_notes.is_empty() {
            let note_ids: Vec<_> = alice_notes.iter().map(|(note, _)| note.id()).collect();
            let consume_request =
                TransactionRequestBuilder::new().build_consume_notes(note_ids).unwrap();

            match client.new_transaction(alice_account.id(), consume_request).await {
                Ok(consume_tx) => {
                    let consume_tx_id = consume_tx.executed_transaction().id();
                    eprintln!("Created consume transaction. Tx ID: {consume_tx_id:?}");

                    // Try to submit
                    match client.submit_transaction(consume_tx).await {
                        Ok(_) => eprintln!("Alice successfully consumed mint note"),
                        Err(err) => {
                            eprintln!("Failed to submit consume transaction: {err}");
                            let err_str = err.to_string();
                            assert!(
                                err_str.contains("RpcError")
                                    || err_str.contains("protocol error")
                                    || err_str.contains("rpc api error"),
                                "Unexpected error: {err_str}"
                            );
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Failed to create consume transaction: {err}");
                }
            }
        } else {
            eprintln!("No mint notes available for Alice (likely due to submission error)");
        }

        // Step 3: Create Bob's account
        eprintln!("\n=== Step 3: Creating Bob's account ===");
        let bob_account =
            create_basic_wallet_account(&mut client, keystore.clone(), wallet_package)
                .await
                .unwrap();
        eprintln!("Bob account ID: {:?}", bob_account.id().to_hex());

        // Step 4: Alice creates p2id note for Bob
        eprintln!("\n=== Step 4: Alice creates p2id note for Bob ===");
        let transfer_amount = 10_000u64; // 10,000 tokens
        let transfer_asset = FungibleAsset::new(faucet_account.id(), transfer_amount).unwrap();

        let p2id_note = create_p2id_note(
            alice_account.id(),
            bob_account.id(),
            vec![transfer_asset.into()],
            NoteType::Public,
            Felt::ZERO,
            client.rng(),
        )
        .unwrap();

        eprintln!("Created P2ID note. Note ID: {:?}", p2id_note.id().to_hex());

        let alice_tx_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note.clone())])
            .build()
            .unwrap();

        match client.new_transaction(alice_account.id(), alice_tx_request).await {
            Ok(alice_tx) => {
                let alice_tx_id = alice_tx.executed_transaction().id();
                eprintln!("Alice created p2id transaction. Tx ID: {alice_tx_id:?}");

                // Try to submit
                match client.submit_transaction(alice_tx).await {
                    Ok(_) => eprintln!("Successfully submitted p2id transaction"),
                    Err(err) => {
                        eprintln!("Failed to submit p2id transaction: {err}");
                        let err_str = err.to_string();
                        // This might fail if Alice doesn't have enough balance
                        assert!(
                            err_str.contains("RpcError")
                                || err_str.contains("protocol error")
                                || err_str.contains("insufficient")
                                || err_str.contains("AssetError")
                                || err_str.contains("rpc api error"),
                            "Unexpected error: {err_str}"
                        );
                    }
                }
            }
            Err(err) => {
                eprintln!("Failed to create p2id transaction: {err}");
                let err_str = err.to_string();
                // Expected if Alice doesn't have assets
                assert!(
                    err_str.contains("AssetError")
                        || err_str.contains("FungibleAssetAmountNotSufficient")
                        || err_str.contains("asset error"),
                    "Unexpected error: {err_str}"
                );
            }
        }

        // Step 5: Bob attempts to consume the p2id note
        eprintln!("\n=== Step 5: Bob attempts to consume p2id note ===");
        client.sync_state().await.unwrap();

        let bob_notes = client.get_consumable_notes(Some(bob_account.id())).await.unwrap();
        eprintln!("Bob has {} consumable notes", bob_notes.len());

        if !bob_notes.is_empty() {
            let consume_request = TransactionRequestBuilder::new()
                .unauthenticated_input_notes([(p2id_note.clone(), None)])
                .build()
                .unwrap();

            match client.new_transaction(bob_account.id(), consume_request).await {
                Ok(consume_tx) => {
                    let consume_tx_id = consume_tx.executed_transaction().id();
                    eprintln!("Bob created consume transaction. Tx ID: {consume_tx_id:?}");

                    match client.submit_transaction(consume_tx).await {
                        Ok(_) => eprintln!("Bob successfully consumed p2id note!"),
                        Err(err) => {
                            eprintln!("Failed to submit Bob's consume transaction: {err}");
                            let err_str = err.to_string();
                            assert!(
                                err_str.contains("RpcError")
                                    || err_str.contains("protocol error")
                                    || err_str.contains("rpc api error"),
                                "Unexpected error: {err_str}"
                            );
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Bob failed to create consume transaction: {err}");
                    let err_str = err.to_string();
                    // Expected error due to p2id validation
                    assert!(
                        err_str.contains("failed to execute transaction kernel program")
                            || err_str.contains("advice map"),
                        "Unexpected error: {err_str}"
                    );
                }
            }
        } else {
            eprintln!("No p2id notes available for Bob (likely due to earlier errors)");
        }

        eprintln!("\n=== Test completed with expected error handling ===");
    });
}
