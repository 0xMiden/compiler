//! This module provides accommodation for the integration tests that execute against the Miden
//! testnet

use std::{env, sync::Arc, time::Duration};

use miden_client::{
    account::{
        component::{BasicWallet, RpoFalcon512},
        Account, AccountBuilder, AccountId, AccountStorageMode, AccountType,
    },
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::{FeltRng, SecretKey},
    keystore::FilesystemKeyStore,
    note::{
        Note, NoteExecutionHint, NoteExecutionMode, NoteInputs, NoteMetadata, NoteRecipient,
        NoteScript, NoteTag, NoteType,
    },
    rpc::{Endpoint, TonicRpcClient},
    store::InputNoteRecord,
    transaction::{OutputNote, TransactionKernel, TransactionRequestBuilder},
    Client, ClientError, Felt, Word,
};
use miden_core::{utils::Deserializable, FieldElement};
use miden_objects::{
    account::{
        AccountComponent, AccountComponentMetadata, AccountComponentTemplate, InitStorageData,
    },
    Hasher,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{rngs::StdRng, RngCore};
use tokio::time::sleep;

use crate::CompilerTestBuilder;

/// Helper to create a basic account
#[allow(dead_code)]
async fn create_basic_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    let anchor_block = client.get_latest_epoch_block().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .anchor((&anchor_block).try_into().unwrap())
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicWallet);
    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Helper to create a basic account with the counter contract
async fn create_counter_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    account_package: Arc<miden_mast_package::Package>,
) -> Result<Account, ClientError> {
    let account_component = match account_package.account_component_metadata_bytes.as_deref() {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                account_package.unwrap_library().as_ref().clone(),
            );
            AccountComponent::from_template(&template, &InitStorageData::default()).unwrap()
        }
    };

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    let anchor_block = client.get_latest_epoch_block().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .anchor((&anchor_block).try_into().unwrap())
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicWallet)
        .with_component(account_component);
    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

// Helper to wait until an account has the expected number of consumable notes
async fn wait_for_notes(
    client: &mut Client,
    account_id: &miden_client::account::Account,
    expected: usize,
) -> Result<(), ClientError> {
    loop {
        client.sync_state().await?;
        let notes = client.get_consumable_notes(Some(account_id.id())).await?;
        if notes.len() >= expected {
            break;
        }
        eprintln!(
            "{} consumable notes found for account {}. Waiting...",
            notes.len(),
            account_id.id().to_hex()
        );
        sleep(Duration::from_secs(3)).await;
    }
    Ok(())
}

fn assert_counter_storage(counter_account: &Account, expected: u64) {
    // according to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO; 4]);
    // The storage slot is 1 since the RpoFalcon512 account component sits in 0 slot
    let counter_val_word =
        counter_account.storage().get_map_item(1, counter_contract_storage_key).unwrap();
    // Felt is stored in the last word item. See sdk/stdlib-sys/src/intrinsics/word.rs
    let counter_val = counter_val_word.last().unwrap();
    assert_eq!(counter_val.as_int(), expected);
}

/// Tests the counter contract deployment and note consumption workflow on testnet.
#[test]
pub fn test_counter_contract_testnet() {
    // Compile the contracts first (before creating any runtime)
    let config = WasmTranslationConfig::default();
    let mut contract_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-contract",
        config.clone(),
        [],
    );
    contract_builder.with_release(true);
    let mut contract_test = contract_builder.build();
    let contract_package = contract_test.compiled_package();

    // Compile the counter note
    let mut note_builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-note", config, []);
    note_builder.with_release(true);
    let mut note_test = note_builder.build();
    let note_package = note_test.compiled_package();

    let restore_dir = env::current_dir().unwrap();
    // switch cwd to temp_dir to have a fresh client store
    let temp_dir = temp_dir::TempDir::new().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Initialize client & keystore
        let endpoint = Endpoint::testnet();
        let timeout_ms = 10_000;
        let rpc_api = Arc::new(TonicRpcClient::new(&endpoint, timeout_ms));

        let keystore_path = temp_dir.path().join("keystore");
        let keystore = Arc::new(FilesystemKeyStore::new(keystore_path.clone()).unwrap());

        let mut client = ClientBuilder::new()
            .with_rpc(rpc_api)
            .with_filesystem_keystore(keystore_path.to_str().unwrap())
            .in_debug_mode(true)
            .build()
            .await
            .unwrap();

        let sync_summary = client.sync_state().await.unwrap();
        eprintln!("Latest block: {}", sync_summary.block_num);

        // Create the counter account
        let counter_account =
            create_counter_account(&mut client, keystore.clone(), contract_package)
                .await
                .unwrap();
        eprintln!("Counter account ID: {:?}", counter_account.id().to_hex());

        client.sync_state().await.unwrap();

        // The counter contract storage value should be zero after the account creation
        assert_counter_storage(&counter_account, 0);

        // Create the counter note from sender to counter
        let note_program = note_package.unwrap_program();
        let note_script =
            NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

        let serial_num = client.rng().draw_word();
        let note_inputs = NoteInputs::new(vec![]).unwrap();
        let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

        let tag = NoteTag::for_local_use_case(0, 0).unwrap();
        let metadata = NoteMetadata::new(
            counter_account.id(), // The sender is who creates the note
            NoteType::Public,
            tag,
            NoteExecutionHint::always(),
            Felt::ZERO,
        )
        .unwrap();

        let vault = miden_client::note::NoteAssets::new(vec![]).unwrap();
        let counter_note = Note::new(vault, metadata, recipient);
        eprintln!("Counter note hash: {:?}", counter_note.id().to_hex());

        // Submit transaction to create the note
        let note_request = TransactionRequestBuilder::new()
            .with_own_output_notes(vec![OutputNote::Full(counter_note.clone())])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(counter_account.id(), note_request).await.unwrap();
        let create_note_tx_id = tx_result.executed_transaction().id();
        client
            .submit_transaction(tx_result)
            .await
            .expect("failed to submit the tx creating the note");
        client.sync_state().await.unwrap();
        eprintln!(
            "Created counter note tx: https://testnet.midenscan.com/tx/{:?}",
            create_note_tx_id
        );

        wait_for_notes(&mut client, &counter_account, 1).await.unwrap();

        // Consume the note to increment the counter
        let consume_request = TransactionRequestBuilder::new()
            .with_authenticated_input_notes([(counter_note.id(), None)])
            .build()
            .unwrap();

        let tx_result =
            client.new_transaction(counter_account.id(), consume_request).await.unwrap();

        eprintln!(
            "Consumed counter note tx: https://testnet.midenscan.com/tx/{:?}",
            tx_result.executed_transaction().id()
        );

        client
            .submit_transaction(tx_result)
            .await
            .expect("failed to submit the tx consuming the note");

        // The counter contract storage value should be 1 (incremented) after the note is consumed
        assert_counter_storage(&counter_account, 1);
    });

    env::set_current_dir(restore_dir).unwrap();
}
