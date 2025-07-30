//! Basic wallet test module

use std::{sync::Arc, time::Duration};

use miden_client::{
    account::{
        component::{BasicFungibleFaucet, RpoFalcon512},
        Account, AccountStorageMode, AccountType,
    },
    asset::{FungibleAsset, TokenSymbol},
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::{FeltRng, SecretKey},
    keystore::FilesystemKeyStore,
    note::{
        Note, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag,
        NoteType,
    },
    rpc::{Endpoint, TonicRpcClient},
    transaction::{OutputNote, TransactionRequestBuilder},
    Client, ClientError,
};
use miden_core::{
    utils::{Deserializable, Serializable},
    Felt, FieldElement,
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
    // .with_component(BasicWallet);
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
        // ["--debug=none".into()], // don't include any debug info in the compiled MAST
        [],
    );
    wallet_builder.with_release(true);
    let mut wallet_test = wallet_builder.build();
    let wallet_package = wallet_test.compiled_package();

    // let bytes = <miden_mast_package::Package as Clone>::clone(&wallet_package)
    //     .into_mast_artifact()
    //     .unwrap_library()
    //     .to_bytes();
    // assert!(bytes.len() < 32767, "expected to fit in 32 KB account update size limit");

    // Use temp_dir for a fresh client store
    let temp_dir = temp_dir::TempDir::with_prefix("test_basic_wallet_p2id_local_").unwrap();

    // write bytes to disc at temp_dir with basic_wallet.masp file name
    let wallet_package_path = temp_dir.path().join("basic_wallet.masp");
    std::fs::write(&wallet_package_path, wallet_package.to_bytes())
        .expect("Failed to write wallet");

    // dbg!(&wallet_package.manifest.exports);

    // Compile the p2id note
    let mut note_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/p2id-note",
        config,
        [
            // "--debug=none".into(),
            // "--link-library".into(),
            // wallet_package_path.to_string_lossy().into(),
        ],
    );
    dbg!(&wallet_package_path);
    note_builder.with_release(true);
    let mut note_test = note_builder.build();
    let note_package = note_test.compiled_package();

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

        let store_path = temp_dir.path().join("store.sqlite3").to_str().unwrap().to_string();
        let mut client = ClientBuilder::new()
            .rpc(rpc_api)
            .sqlite_store(&store_path)
            .filesystem_keystore(keystore_path.to_str().unwrap())
            .in_debug_mode(true)
            .build()
            .await
            .unwrap();

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

        // Resync to show newly deployed faucet
        client.sync_state().await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;

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

        // Create the p2id note from Alice to Bob
        let note_program = note_package.unwrap_program();
        let note_script =
            NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

        let serial_num = client.rng().draw_word();
        let note_inputs = NoteInputs::new(vec![
            alice_account.id().prefix().as_felt(),
            alice_account.id().suffix(),
        ])
        .unwrap();
        let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

        let tag = NoteTag::for_public_use_case(0, 0, miden_client::note::NoteExecutionMode::Local)
            .unwrap();
        let metadata = NoteMetadata::new(
            faucet_account.id(), // The sender is who creates the note
            NoteType::Public,
            tag,
            NoteExecutionHint::always(),
            Felt::ZERO,
        )
        .unwrap();

        let vault = miden_client::note::NoteAssets::new(vec![fungible_asset.into()]).unwrap();
        let p2id_note_mint = Note::new(vault, metadata, recipient);
        eprintln!("P2ID mint note hash: {:?}", p2id_note_mint.id().to_hex());

        let mint_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
            .build()
            .unwrap();

        let mint_tx_result =
            client.new_transaction(faucet_account.id(), mint_request).await.unwrap();
        let mint_tx_id = mint_tx_result.executed_transaction().id();
        eprintln!("Created mint transaction. Tx ID: {mint_tx_id:?}");

        // Try to submit the mint transaction
        client.submit_transaction(mint_tx_result).await.unwrap();
        eprintln!("Submitted mint transaction. Tx ID: {mint_tx_id:?}");

        // Step 2: Wait and try to consume the mint note
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

        // Create the p2id note from Alice to Bob
        let note_program = note_package.unwrap_program();
        let note_script =
            NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

        let serial_num = client.rng().draw_word();
        let note_inputs =
            NoteInputs::new(vec![bob_account.id().prefix().as_felt(), bob_account.id().suffix()])
                .unwrap();
        let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

        let tag = NoteTag::for_public_use_case(0, 0, miden_client::note::NoteExecutionMode::Local)
            .unwrap();
        let metadata = NoteMetadata::new(
            alice_account.id(), // The sender is who creates the note
            NoteType::Public,
            tag,
            NoteExecutionHint::always(),
            Felt::ZERO,
        )
        .unwrap();

        let vault = miden_client::note::NoteAssets::new(vec![transfer_asset.into()]).unwrap();
        let p2id_note = Note::new(vault, metadata, recipient);
        eprintln!("P2ID note hash: {:?}", p2id_note.id().to_hex());

        let alice_tx_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note.clone())])
            .build()
            .unwrap();

        let alice_tx_res = client.new_transaction(alice_account.id(), alice_tx_request).await;
        // We can create our custom P2ID note only from the tx script.
        // Until tx script compilation is implemented https://github.com/0xMiden/compiler/issues/622
        assert!(alice_tx_res.is_err());
        // // let alice_tx = client.new_transaction(alice_account.id(), alice_tx_request).await.unwrap();
        // let alice_tx_id = alice_tx.executed_transaction().id();
        // eprintln!("Alice created p2id transaction. Tx ID: {alice_tx_id:?}");
        //
        // // Try to submit
        // client.submit_transaction(alice_tx).await.unwrap();
        //
        // // Step 5: Bob attempts to consume the p2id note
        // eprintln!("\n=== Step 5: Bob attempts to consume p2id note ===");
        //
        // let consume_request = TransactionRequestBuilder::new()
        //     .unauthenticated_input_notes([(p2id_note.clone(), None)])
        //     .build()
        //     .unwrap();
        //
        // let consume_tx = client.new_transaction(bob_account.id(), consume_request).await.unwrap();
        // let consume_tx_id = consume_tx.executed_transaction().id();
        // eprintln!("Bob created consume transaction. Tx ID: {consume_tx_id:?}");
        //
        // client.submit_transaction(consume_tx).await.unwrap();
    });
}
