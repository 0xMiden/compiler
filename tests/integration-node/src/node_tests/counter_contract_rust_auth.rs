//! Counter contract test using an auth component compiled from Rust (RPO-Falcon512)
//!
//! This test ensures that an account which does not possess the correct
//! RPO-Falcon512 secret key cannot create notes on behalf of the counter
//! contract account that uses the Rust-compiled auth component.

use miden_client::{
    account::StorageMap,
    auth::AuthSecretKey,
    keystore::FilesystemKeyStore,
    transaction::{OutputNote, TransactionRequestBuilder},
    utils::Deserializable,
    Client, DebugMode, Word,
};
use miden_core::{Felt, FieldElement};
use rand::{rngs::StdRng, RngCore};

use super::helpers::*;
use crate::local_node::ensure_shared_node;

fn assert_counter_storage(
    counter_account_storage: &miden_client::account::AccountStorage,
    expected: u64,
) {
    // According to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

    // With RPO-Falcon512 auth component occupying slot 0, the counter component is at slot 1.
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

/// Build a counter account from the counter component package and the
/// Rust-compiled RPO-Falcon512 auth component package.
async fn create_counter_account_with_rust_rpo_auth(
    client: &mut Client<FilesystemKeyStore<StdRng>>,
    component_package: std::sync::Arc<miden_mast_package::Package>,
    auth_component_package: std::sync::Arc<miden_mast_package::Package>,
    keystore: std::sync::Arc<FilesystemKeyStore<StdRng>>,
) -> Result<(miden_client::account::Account, Word), miden_client::ClientError> {
    use std::collections::BTreeSet;

    use miden_objects::account::{
        AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
        AccountStorageMode, AccountType, StorageSlot,
    };

    // Build counter component from template/metadata with initial storage
    let account_component = match component_package.account_component_metadata_bytes.as_deref() {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                component_package.unwrap_library().as_ref().clone(),
            );

            // Initialize the counter storage to 1 at key [0,0,0,1]
            let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
            let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
            let storage = vec![StorageSlot::Map(StorageMap::with_entries([(key, value)]).unwrap())];

            let component = AccountComponent::new(template.library().clone(), storage).unwrap();
            component.with_supported_types(BTreeSet::from_iter([
                AccountType::RegularAccountUpdatableCode,
            ]))
        }
    };

    // Build the Rust-compiled auth component with public key commitment in slot 0
    let key_pair = miden_client::crypto::SecretKey::with_rng(client.rng());
    let pk_commitment = miden_objects::Word::from(key_pair.public_key());
    let mut auth_component = AccountComponent::new(
        auth_component_package.unwrap_library().as_ref().clone(),
        vec![StorageSlot::Value(pk_commitment)],
    )
    .unwrap();
    auth_component = auth_component
        .with_supported_types(BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode]));

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let _ = client.sync_state().await?;

    let (account, seed) = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(miden_client::account::component::BasicWallet)
        .with_component(account_component)
        .build()
        .unwrap();

    client.add_account(&account, Some(seed), false).await?;

    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok((account, seed))
}

/// Verify that another client (without the RPO-Falcon512 key) cannot create notes for
/// the counter account which uses the Rust-compiled RPO-Falcon512 authentication component.
#[test]
#[ignore = "until migrated to miden client v0.11"]
pub fn test_counter_contract_rust_auth_blocks_unauthorized_note_creation() {
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);
    let rpo_auth_package =
        compile_rust_package("../../examples/auth-component-rpo-falcon512", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let temp_dir = temp_dir::TempDir::with_prefix("test_counter_contract_rust_auth_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        let (counter_account, counter_seed) = create_counter_account_with_rust_rpo_auth(
            &mut client,
            contract_package.clone(),
            rpo_auth_package.clone(),
            keystore.clone(),
        )
        .await
        .unwrap();
        eprintln!(
            "Counter account (Rust RPO-Falcon512 auth) ID: {:?}",
            counter_account.id().to_hex()
        );

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

        // Positive check: original client (with the key) can create a note
        let own_note = create_note_from_package(
            &mut client,
            note_package.clone(),
            counter_account.id(),
            NoteCreationConfig::default(),
        );
        let own_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(own_note.clone())])
            .build()
            .unwrap();
        let ok_tx = client
            .new_transaction(counter_account.id(), own_request)
            .await
            .expect("authorized client should be able to create a note");
        assert_eq!(ok_tx.executed_transaction().output_notes().num_notes(), 1);
        assert_eq!(ok_tx.executed_transaction().output_notes().get_note(0).id(), own_note.id());
        client.submit_transaction(ok_tx).await.unwrap();

        // Create a separate client with its own empty keystore (no key for counter account)
        let attacker_dir = temp_dir::TempDir::with_prefix("attacker_client_")
            .expect("Failed to create temp directory");
        let rpc_url = node_handle.rpc_url().to_string();
        let endpoint = miden_client::rpc::Endpoint::try_from(rpc_url.as_str()).unwrap();
        let rpc_api =
            std::sync::Arc::new(miden_client::rpc::TonicRpcClient::new(&endpoint, 10_000));
        let attacker_store_path =
            attacker_dir.path().join("store.sqlite3").to_str().unwrap().to_string();
        let attacker_keystore_path = attacker_dir.path().join("keystore");

        let mut attacker_client = miden_client::builder::ClientBuilder::new()
            .rpc(rpc_api)
            .sqlite_store(&attacker_store_path)
            .filesystem_keystore(attacker_keystore_path.to_str().unwrap())
            .in_debug_mode(DebugMode::Enabled)
            .build()
            .await
            .unwrap();

        // The attacker needs the account record locally to attempt building a tx
        // Reuse the same account object; seed is not needed for reading/state queries
        attacker_client
            .add_account(&counter_account, Some(counter_seed), false)
            .await
            .expect("failed to add account to attacker client");

        // Attacker tries to create an output note on behalf of the counter account
        // (origin = counter_account.id()), but does not have the required secret key.
        let forged_note = create_note_from_package(
            &mut attacker_client,
            note_package.clone(),
            counter_account.id(),
            NoteCreationConfig::default(),
        );

        let forged_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(forged_note.clone())])
            .build()
            .unwrap();

        let result = attacker_client.new_transaction(counter_account.id(), forged_request).await;

        assert!(
            result.is_err(),
            "Unauthorized client unexpectedly created a transaction for the counter account"
        );
    });
}
