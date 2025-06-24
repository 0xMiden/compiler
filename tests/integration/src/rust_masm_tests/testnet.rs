//! This module provides accommodation for the integration tests that execute against the Miden
//! testnet

use std::{env, sync::Arc, time::Duration};

use miden_client::{
    account::{
        component::{BasicWallet, RpoFalcon512},
        Account, AccountBuilder, AccountId, AccountStorage, AccountStorageMode, AccountType,
        StorageMap, StorageSlot,
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
use miden_core::{
    utils::{Deserializable, Serializable},
    FieldElement,
};
use miden_objects::{
    account::{
        AccountComponent, AccountComponentMetadata, AccountComponentTemplate, InitStorageData,
        StorageValueName,
    },
    Hasher,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{rngs::StdRng, RngCore};
use tokio::time::sleep;

use crate::{rust_masm_tests::local_node, CompilerTestBuilder};

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

            let word_zero = Word::from([Felt::ZERO; 4]);
            AccountComponent::new(
                template.library().clone(),
                vec![StorageSlot::Map(
                    StorageMap::with_entries([(word_zero.into(), word_zero)]).unwrap(),
                )],
            )
            .unwrap()
            .with_supported_types([AccountType::RegularAccountUpdatableCode].into())
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
#[allow(dead_code)]
async fn wait_for_notes(
    client: &mut Client,
    account_id: &miden_client::account::Account,
    expected: usize,
) -> Result<(), ClientError> {
    let mut try_num = 0;
    loop {
        client.sync_state().await?;
        let notes = client.get_consumable_notes(None).await?;
        if notes.len() >= expected {
            break;
        }
        eprintln!(
            "{} consumable notes found for account {}. Waiting...",
            notes.len(),
            account_id.id().to_hex()
        );
        if try_num > 10 {
            panic!("waiting for too long");
        } else {
            try_num += 1;
        }
        sleep(Duration::from_secs(3)).await;
    }
    Ok(())
}

fn assert_counter_storage(counter_account_storage: &AccountStorage, expected: u64) {
    // dbg!(counter_account_storage);
    // according to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO; 4]);
    // The storage slot is 1 since the RpoFalcon512 account component sits in 0 slot
    let counter_val_word =
        counter_account_storage.get_map_item(1, counter_contract_storage_key).unwrap();
    // Felt is stored in the last word item. See sdk/stdlib-sys/src/intrinsics/word.rs
    let counter_val = counter_val_word.last().unwrap();
    // dbg!(&counter_val_word);
    assert_eq!(counter_val.as_int(), expected);
}

/// Tests the counter contract deployment and note consumption workflow on testnet.
#[test]
#[ignore] // Ignore by default as it requires testnet connection
pub fn test_counter_contract_testnet() {
    // Compile the contracts first (before creating any runtime)
    let config = WasmTranslationConfig::default();
    let mut contract_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-contract",
        config.clone(),
        ["--debug=none".into()], // don't include any debug info in the compiled MAST
    );
    contract_builder.with_release(true);
    let mut contract_test = contract_builder.build();
    let contract_package = contract_test.compiled_package();

    let bytes = <miden_mast_package::Package as Clone>::clone(&contract_package)
        .into_mast_artifact()
        .unwrap_library()
        .to_bytes();
    // dbg!(bytes.len());
    assert!(bytes.len() < 32767, "expected to fit in 32 KB account update size limit");

    // Compile the counter note
    let mut note_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-note",
        config,
        ["--debug=none".into()], // don't include any debug info in the compiled MAST
    );
    note_builder.with_release(true);
    let mut note_test = note_builder.build();
    let note_package = note_test.compiled_package();
    dbg!(note_package.unwrap_program().mast_forest().advice_map());

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

        // client.sync_state().await.unwrap();

        // The counter contract storage value should be zero after the account creation
        assert_counter_storage(
            client
                .get_account(counter_account.id())
                .await
                .unwrap()
                .unwrap()
                .account()
                .storage(),
            0,
        );

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
        let executed_transaction = tx_result.executed_transaction();
        // dbg!(executed_transaction.output_notes());

        assert_eq!(executed_transaction.output_notes().num_notes(), 1);

        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), counter_note.id());
        let create_note_tx_id = executed_transaction.id();
        // client
        //     .submit_transaction(tx_result)
        //     .await
        //     .expect("failed to submit the tx creating the note");
        eprintln!(
            "Created counter note tx: https://testnet.midenscan.com/tx/{:?}",
            create_note_tx_id
        );

        client.sync_state().await.unwrap();

        // wait_for_notes(&mut client, &counter_account, 1).await.unwrap();

        // Consume the note to increment the counter
        let consume_request = TransactionRequestBuilder::new()
            // .with_authenticated_input_notes([(counter_note.id(), None)])
            .with_unauthenticated_input_notes([(counter_note, None)])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(counter_account.id(), consume_request).await;

        // Assert that tx_result contains the expected error until the
        // https://github.com/0xMiden/miden-base/issues/1452 is not propagated into the client
        assert!(tx_result.is_err());
        let err = tx_result.unwrap_err();

        // Check if the error matches the expected pattern
        let err_str = err.to_string();

        // The error should indicate a failure to execute transaction kernel program
        assert!(
            err_str.contains("failed to execute transaction kernel program"),
            "Expected transaction kernel program execution failure, got: {}",
            err_str
        );

        // Check that it mentions value not present in advice map
        assert!(
            err_str.contains("not present in the advice map"),
            "Expected advice map key not found error, got: {}",
            err_str
        );

        // Check for the specific key in hex format
        // The key [10393006917776393985, 11082306316302361448, 8154980225314320902, 11512975618068632545]
        // corresponds to hex: 4558874500473d2ab899ee9a662345cbacbea1b604f231d8ccdd82d9dfd3b686
        assert!(
            err_str.contains("4558874500473d2ab899ee9a662345cbacbea1b604f231d8ccdd82d9dfd3b686"),
            "Expected specific key in error, got: {}",
            err_str
        );

        // eprintln!(
        //     "Consumed counter note tx: https://testnet.midenscan.com/tx/{:?}",
        //     tx_result.executed_transaction().id()
        // );

        // client
        //     .submit_transaction(tx_result)
        //     .await
        //     .expect("failed to submit the tx consuming the note");

        // client.sync_state().await.unwrap();

        // The counter contract storage value should be 1 (incremented) after the note is consumed
        // assert_counter_storage(
        //     client
        //         .get_account(counter_account.id())
        //         .await
        //         .unwrap()
        //         .unwrap()
        //         .account()
        //         .storage(),
        //     1,
        // );
    });

    env::set_current_dir(restore_dir).unwrap();
}

/// Tests the counter contract deployment and note consumption workflow on a local node.
#[test]
pub fn test_counter_contract_local() {
    // Compile the contracts first (before creating any runtime)
    let config = WasmTranslationConfig::default();
    let mut contract_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-contract",
        config.clone(),
        ["--debug=none".into()], // don't include any debug info in the compiled MAST
    );
    contract_builder.with_release(true);
    let mut contract_test = contract_builder.build();
    let contract_package = contract_test.compiled_package();

    let bytes = <miden_mast_package::Package as Clone>::clone(&contract_package)
        .into_mast_artifact()
        .unwrap_library()
        .to_bytes();
    // dbg!(bytes.len());
    assert!(bytes.len() < 32767, "expected to fit in 32 KB account update size limit");

    // Compile the counter note
    let mut note_builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-note",
        config,
        ["--debug=none".into()], // don't include any debug info in the compiled MAST
    );
    note_builder.with_release(true);
    let mut note_test = note_builder.build();
    let note_package = note_test.compiled_package();
    dbg!(note_package.unwrap_program().mast_forest().advice_map());

    let restore_dir = env::current_dir().unwrap();
    // switch cwd to temp_dir to have a fresh client store
    let temp_dir = temp_dir::TempDir::new().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create an isolated node instance for this test
        let mut node = local_node::create_isolated_node()
            .await
            .expect("Failed to start local node");

        let rpc_url = node.rpc_url().to_string();

        // Initialize client & keystore
        let endpoint = Endpoint::try_from(rpc_url.as_str()).expect("Failed to create endpoint");
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

        // client.sync_state().await.unwrap();

        // The counter contract storage value should be zero after the account creation
        assert_counter_storage(
            client
                .get_account(counter_account.id())
                .await
                .unwrap()
                .unwrap()
                .account()
                .storage(),
            0,
        );

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
        let executed_transaction = tx_result.executed_transaction();
        // dbg!(executed_transaction.output_notes());

        assert_eq!(executed_transaction.output_notes().num_notes(), 1);

        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), counter_note.id());
        let create_note_tx_id = executed_transaction.id();
        // client
        //     .submit_transaction(tx_result)
        //     .await
        //     .expect("failed to submit the tx creating the note");
        eprintln!("Created counter note tx: {:?}", create_note_tx_id);

        client.sync_state().await.unwrap();

        // wait_for_notes(&mut client, &counter_account, 1).await.unwrap();

        // Consume the note to increment the counter
        let consume_request = TransactionRequestBuilder::new()
            // .with_authenticated_input_notes([(counter_note.id(), None)])
            .with_unauthenticated_input_notes([(counter_note, None)])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(counter_account.id(), consume_request).await;

        // Assert that tx_result contains the expected error until the
        // https://github.com/0xMiden/miden-base/issues/1452 is not propagated into the client
        assert!(tx_result.is_err());
        let err = tx_result.unwrap_err();

        // Check if the error matches the expected pattern
        let err_str = err.to_string();

        // The error should indicate a failure to execute transaction kernel program
        assert!(
            err_str.contains("failed to execute transaction kernel program"),
            "Expected transaction kernel program execution failure, got: {}",
            err_str
        );

        // Check that it mentions value not present in advice map
        assert!(
            err_str.contains("not present in the advice map"),
            "Expected advice map key not found error, got: {}",
            err_str
        );

        // Check for the specific key in hex format
        // The key [10393006917776393985, 11082306316302361448, 8154980225314320902, 11512975618068632545]
        // corresponds to hex: 4558874500473d2ab899ee9a662345cbacbea1b604f231d8ccdd82d9dfd3b686
        assert!(
            err_str.contains("4558874500473d2ab899ee9a662345cbacbea1b604f231d8ccdd82d9dfd3b686"),
            "Expected specific key in error, got: {}",
            err_str
        );

        // eprintln!(
        //     "Consumed counter note tx: https://testnet.midenscan.com/tx/{:?}",
        //     tx_result.executed_transaction().id()
        // );

        // client
        //     .submit_transaction(tx_result)
        //     .await
        //     .expect("failed to submit the tx consuming the note");

        // client.sync_state().await.unwrap();

        // The counter contract storage value should be 1 (incremented) after the note is consumed
        // assert_counter_storage(
        //     client
        //         .get_account(counter_account.id())
        //         .await
        //         .unwrap()
        //         .unwrap()
        //         .account()
        //         .storage(),
        //     1,
        // );
    });

    env::set_current_dir(restore_dir).unwrap();
}
