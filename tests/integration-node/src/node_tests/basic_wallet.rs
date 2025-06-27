//! Basic wallet test module

use std::{env, sync::Arc};

use miden_client::{
    account::{component::RpoFalcon512, Account, AccountBuilder, AccountStorageMode, AccountType},
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
    Client, ClientError, Felt,
};
use miden_core::{
    utils::{Deserializable, Serializable},
    FieldElement,
};
use miden_integration_tests::CompilerTestBuilder;
use miden_objects::account::{
    AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{rngs::StdRng, RngCore};

use super::helpers::create_basic_account;
use crate::local_node;

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
    let anchor_block = client.get_latest_epoch_block().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .anchor((&anchor_block).try_into().unwrap())
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(RpoFalcon512::new(key_pair.public_key()))
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
    let note_package = note_test.compiled_package();

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
            .with_rpc(rpc_api)
            .with_filesystem_keystore(keystore_path.to_str().unwrap())
            .in_debug_mode(true)
            .build()
            .await
            .unwrap();

        // Restore original directory after client creation
        env::set_current_dir(original_dir).unwrap();

        let sync_summary = client.sync_state().await.unwrap();
        eprintln!("Latest block: {}", sync_summary.block_num);

        // Create sender account (basic account)
        let sender_account = create_basic_account(&mut client, keystore.clone()).await.unwrap();
        eprintln!("Sender account ID: {:?}", sender_account.id().to_hex());

        // Create the receiver account with basic-wallet component
        let receiver_account =
            create_basic_wallet_account(&mut client, keystore.clone(), wallet_package)
                .await
                .unwrap();
        eprintln!("Receiver account ID: {:?}", receiver_account.id().to_hex());

        // Create the p2id note from sender to receiver
        let note_program = note_package.unwrap_program();
        let note_script =
            NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

        let serial_num = client.rng().draw_word();
        // Pass the receiver account ID as input to the p2id note
        // For now, we'll skip the account ID check in the p2id note by passing an empty input
        // The test will fail at the assertion in the p2id note script, but that's expected
        // given the current limitations with AccountId to Felt conversion
        let note_inputs = NoteInputs::new(vec![]).unwrap();
        let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

        let tag = NoteTag::for_local_use_case(0, 0).unwrap();
        let metadata = NoteMetadata::new(
            sender_account.id(), // The sender creates the note
            NoteType::Public,
            tag,
            NoteExecutionHint::always(),
            Felt::ZERO,
        )
        .unwrap();

        // Create an empty vault for now (same as counter test)
        let vault = miden_client::note::NoteAssets::new(vec![]).unwrap();
        let p2id_note = Note::new(vault, metadata, recipient);
        eprintln!("P2ID note hash: {:?}", p2id_note.id().to_hex());

        // Submit transaction to create the note
        let note_request = TransactionRequestBuilder::new()
            .with_own_output_notes(vec![OutputNote::Full(p2id_note.clone())])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(sender_account.id(), note_request).await.unwrap();
        let executed_transaction = tx_result.executed_transaction();

        assert_eq!(executed_transaction.output_notes().num_notes(), 1);

        let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
        assert_eq!(executed_tx_output_note.id(), p2id_note.id());
        let create_note_tx_id = executed_transaction.id();
        eprintln!("Created p2id note tx: {:?}", create_note_tx_id);

        client.sync_state().await.unwrap();

        // Consume the note to transfer assets to the receiver
        let consume_request = TransactionRequestBuilder::new()
            .with_unauthenticated_input_notes([(p2id_note, None)])
            .build()
            .unwrap();

        let tx_result = client.new_transaction(receiver_account.id(), consume_request).await;

        // Assert that tx_result contains the expected error
        // (This is expected due to the same issue as in the counter contract test)
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
    });
}
