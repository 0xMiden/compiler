//! Common helper functions for node tests

use std::{collections::BTreeSet, sync::Arc};

use miden_client::{
    account::{
        component::{BasicFungibleFaucet, BasicWallet, RpoFalcon512},
        Account, AccountId, AccountStorageMode, AccountType, StorageSlot,
    },
    asset::TokenSymbol,
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::{FeltRng, SecretKey},
    keystore::FilesystemKeyStore,
    note::{
        Note, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag,
        NoteType,
    },
    rpc::{Endpoint, TonicRpcClient},
    utils::Deserializable,
    Client, ClientError,
};
use miden_core::{Felt, FieldElement};
use miden_integration_tests::CompilerTestBuilder;
use miden_objects::account::{
    AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{rngs::StdRng, RngCore};

/// Test setup configuration
pub struct TestSetup {
    pub client: Client,
    pub keystore: Arc<FilesystemKeyStore<StdRng>>,
}

/// Initialize test infrastructure with client, keystore, and temporary directory
pub async fn setup_test_infrastructure(
    temp_dir: &temp_dir::TempDir,
    node_handle: &crate::local_node::SharedNodeHandle,
) -> Result<TestSetup, Box<dyn std::error::Error>> {
    let rpc_url = node_handle.rpc_url().to_string();

    // Initialize RPC connection
    let endpoint = Endpoint::try_from(rpc_url.as_str()).expect("Failed to create endpoint");
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(TonicRpcClient::new(&endpoint, timeout_ms));

    // Initialize keystore
    let keystore_path = temp_dir.path().join("keystore");
    let keystore = Arc::new(FilesystemKeyStore::<StdRng>::new(keystore_path.clone()).unwrap());

    // Initialize client
    let store_path = temp_dir.path().join("store.sqlite3").to_str().unwrap().to_string();
    let client = ClientBuilder::new()
        .rpc(rpc_api)
        .sqlite_store(&store_path)
        .filesystem_keystore(keystore_path.to_str().unwrap())
        .in_debug_mode(true)
        .build()
        .await?;

    Ok(TestSetup { client, keystore })
}

/// Configuration for creating an account with a custom component
pub struct AccountCreationConfig {
    pub account_type: AccountType,
    pub storage_mode: AccountStorageMode,
    pub storage_slots: Vec<StorageSlot>,
    pub supported_types: Option<Vec<AccountType>>,
    pub with_basic_wallet: bool,
}

impl Default for AccountCreationConfig {
    fn default() -> Self {
        Self {
            account_type: AccountType::RegularAccountUpdatableCode,
            storage_mode: AccountStorageMode::Public,
            storage_slots: vec![],
            supported_types: None,
            with_basic_wallet: true,
        }
    }
}

/// Helper to create an account with a custom component from a package
pub async fn create_account_with_component(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    package: Arc<miden_mast_package::Package>,
    config: AccountCreationConfig,
) -> Result<Account, ClientError> {
    let account_component = match package.account_component_metadata_bytes.as_deref() {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template =
                AccountComponentTemplate::new(metadata, package.unwrap_library().as_ref().clone());

            let component =
                AccountComponent::new(template.library().clone(), config.storage_slots).unwrap();

            // Use supported types from config if provided, otherwise default to RegularAccountUpdatableCode
            let supported_types = if let Some(types) = config.supported_types {
                BTreeSet::from_iter(types)
            } else {
                BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode])
            };

            component.with_supported_types(supported_types)
        }
    };

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());

    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();

    let mut builder = AccountBuilder::new(init_seed)
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()));

    if config.with_basic_wallet {
        builder = builder.with_component(BasicWallet);
    }

    builder = builder.with_component(account_component);

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

pub async fn create_fungible_faucet_account(
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

/// Helper to compile a Rust package to Miden
pub fn compile_rust_package(package_path: &str, release: bool) -> Arc<miden_mast_package::Package> {
    let config = WasmTranslationConfig::default();
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(package_path, config, []);

    if release {
        builder.with_release(true);
    }

    let mut test = builder.build();
    test.compiled_package()
}

/// Configuration for creating a note
pub struct NoteCreationConfig {
    pub note_type: NoteType,
    pub tag: NoteTag,
    pub assets: miden_client::note::NoteAssets,
    pub inputs: Vec<Felt>,
    pub execution_hint: NoteExecutionHint,
    pub aux: Felt,
}

impl Default for NoteCreationConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::for_local_use_case(0, 0).unwrap(),
            assets: Default::default(),
            inputs: Default::default(),
            execution_hint: NoteExecutionHint::always(),
            aux: Felt::ZERO,
        }
    }
}

/// Helper to create a note from a compiled package
pub fn create_note_from_package(
    client: &mut Client,
    package: Arc<miden_mast_package::Package>,
    sender_id: AccountId,
    config: NoteCreationConfig,
) -> Note {
    let note_program = package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let serial_num = client.rng().draw_word();
    let note_inputs = NoteInputs::new(config.inputs).unwrap();
    let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

    let metadata = NoteMetadata::new(
        sender_id,
        config.note_type,
        config.tag,
        config.execution_hint,
        config.aux,
    )
    .unwrap();

    Note::new(config.assets, metadata, recipient)
}
