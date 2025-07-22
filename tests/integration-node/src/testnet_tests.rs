//! This module provides accommodation for the integration tests that execute against the Miden
//! testnet

use std::{env, sync::Arc};

use miden_client::{
    account::{
        component::{BasicWallet, RpoFalcon512},
        Account, AccountStorage, AccountStorageMode, AccountType, StorageMap, StorageSlot,
    },
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
    Client, ClientError, Felt, Word,
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

use crate::node_tests::helpers::wait_for_notes;

/// Helper to create a basic account
#[allow(dead_code)]
async fn create_basic_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
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
    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicWallet)
        .with_component(account_component);
    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
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
            .rpc(rpc_api)
            .filesystem_keystore(keystore_path.to_str().unwrap())
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

        let tag =
            NoteTag::for_public_use_case(0, 0, miden_client::note::NoteExecutionMode::Network)
                .unwrap();
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
            .own_output_notes(vec![OutputNote::Full(counter_note.clone())])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(counter_account.id(), note_request).await.unwrap();
        let executed_transaction = tx_result.executed_transaction();
        // dbg!(executed_transaction.output_notes());

        assert_eq!(executed_transaction.output_notes().num_notes(), 1);

        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), counter_note.id());
        let create_note_tx_id = executed_transaction.id();
        client
            .submit_transaction(tx_result)
            .await
            .expect("failed to submit the tx creating the note");
        eprintln!(
            "Created counter note tx: https://testnet.midenscan.com/tx/{create_note_tx_id:?}"
        );

        wait_for_notes(&mut client, &counter_account.id(), 1).await.unwrap();

        // Consume the note to increment the counter
        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(counter_note, None)])
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

        client.sync_state().await.unwrap();

        // The counter contract storage value should be 1 (incremented) after the note is consumed
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
    });

    env::set_current_dir(restore_dir).unwrap();
}
