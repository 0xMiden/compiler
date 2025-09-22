//! Common helper functions for node tests

use std::{collections::BTreeSet, sync::Arc};

use miden_client::{
    account::{
        component::{AuthRpoFalcon512, BasicFungibleFaucet, BasicWallet},
        Account, AccountId, AccountStorageMode, AccountType, StorageSlot,
    },
    asset::{FungibleAsset, TokenSymbol},
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::{FeltRng, RpoRandomCoin, SecretKey},
    keystore::FilesystemKeyStore,
    note::{
        Note, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag,
        NoteType,
    },
    rpc::{Endpoint, TonicRpcClient},
    transaction::{TransactionRequestBuilder, TransactionScript},
    utils::Deserializable,
    Client, ClientError,
};
use miden_core::{Felt, FieldElement, Word};
use miden_integration_tests::CompilerTestBuilder;
use miden_mast_package::Package;
use miden_objects::{
    account::{
        AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
    },
    asset::Asset,
    transaction::TransactionId,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{rngs::StdRng, RngCore};

/// Test setup configuration
pub struct TestSetup {
    pub client: Client<FilesystemKeyStore<StdRng>>,
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
    let builder = ClientBuilder::new()
        .rpc(rpc_api)
        .sqlite_store(&store_path)
        .filesystem_keystore(keystore_path.to_str().unwrap())
        .in_debug_mode(miden_client::DebugMode::Enabled);
    let client = builder.build().await?;

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
    client: &mut Client<FilesystemKeyStore<StdRng>>,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    package: Arc<Package>,
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
        .with_auth_component(AuthRpoFalcon512::new(key_pair.public_key()));

    if config.with_basic_wallet {
        builder = builder.with_component(BasicWallet);
    }

    builder = builder.with_component(account_component);

    let (account, seed) = builder.build().unwrap_or_else(|e| {
        eprintln!("failed to build account with custom auth component: {e}");
        panic!("failed to build account with custom auth component")
    });
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Create a basic wallet account with standard RpoFalcon512 auth.
///
/// This helper does not require a component package and always adds the `BasicWallet` component.
pub async fn create_basic_wallet_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    config: AccountCreationConfig,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());

    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();

    let builder = AccountBuilder::new(init_seed)
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicWallet);

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Helper to create an account with a custom component and a custom authentication component
pub async fn create_account_with_component_and_auth_package(
    client: &mut Client,
    component_package: Arc<Package>,
    auth_component_package: Arc<Package>,
    config: AccountCreationConfig,
) -> Result<Account, ClientError> {
    // Build the main account component from its template metadata
    let account_component = match component_package.account_component_metadata_bytes.as_deref() {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                component_package.unwrap_library().as_ref().clone(),
            );

            let component =
                AccountComponent::new(template.library().clone(), config.storage_slots.clone())
                    .unwrap();

            // Use supported types from config if provided, otherwise default to RegularAccountUpdatableCode
            let supported_types = if let Some(types) = &config.supported_types {
                BTreeSet::from_iter(types.iter().cloned())
            } else {
                BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode])
            };

            component.with_supported_types(supported_types)
        }
    };

    // Build the authentication component from the compiled library (no storage)
    let mut auth_component =
        AccountComponent::new(auth_component_package.unwrap_library().as_ref().clone(), vec![])
            .unwrap();

    // Ensure auth component supports the intended account type
    if let Some(types) = &config.supported_types {
        auth_component =
            auth_component.with_supported_types(BTreeSet::from_iter(types.iter().cloned()));
    } else {
        auth_component = auth_component
            .with_supported_types(BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode]));
    }

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();

    let mut builder = AccountBuilder::new(init_seed)
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_auth_component(auth_component);

    if config.with_basic_wallet {
        builder = builder.with_component(BasicWallet);
    }

    builder = builder.with_component(account_component);

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    // No keystore key needed for no-auth auth component

    Ok(account)
}

pub async fn create_fungible_faucet_account(
    client: &mut Client<FilesystemKeyStore<StdRng>>,
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
        .with_auth_component(AuthRpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicFungibleFaucet::new(token_symbol, decimals, max_supply).unwrap());

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Helper to compile a Rust package to Miden
pub fn compile_rust_package(package_path: &str, release: bool) -> Arc<Package> {
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
    client: &mut Client<FilesystemKeyStore<StdRng>>,
    package: Arc<Package>,
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

/// Helper function to assert that an account contains a specific fungible asset
/// The account may have other assets as well
pub async fn assert_account_has_fungible_asset(
    client: &mut Client<FilesystemKeyStore<StdRng>>,
    account_id: AccountId,
    expected_faucet_id: AccountId,
    expected_amount: u64,
) {
    let account_record = client
        .get_account(account_id)
        .await
        .expect("Failed to get account")
        .expect("Account not found");

    let account_state: miden_objects::account::Account = account_record.into();

    // Look for the specific fungible asset in the vault
    let found_asset = account_state.vault().assets().find_map(|asset| {
        if let Asset::Fungible(fungible_asset) = asset {
            if fungible_asset.faucet_id() == expected_faucet_id {
                Some(fungible_asset)
            } else {
                None
            }
        } else {
            None
        }
    });

    match found_asset {
        Some(fungible_asset) => {
            assert_eq!(
                fungible_asset.amount(),
                expected_amount,
                "Found asset from faucet {expected_faucet_id} but amount {} doesn't match \
                 expected {expected_amount}",
                fungible_asset.amount()
            );
        }
        None => {
            panic!("Account does not contain a fungible asset from faucet {expected_faucet_id}");
        }
    }
}

/// Configuration for sending assets between accounts
pub struct AssetTransferConfig {
    pub note_type: NoteType,
    pub tag: NoteTag,
    pub execution_hint: NoteExecutionHint,
    pub aux: Felt,
}

impl Default for AssetTransferConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::for_local_use_case(0, 0).unwrap(),
            execution_hint: NoteExecutionHint::always(),
            aux: Felt::ZERO,
        }
    }
}

/// Helper function to send assets from one account to another using a transaction script
///
/// This function creates a p2id note for the recipient and executes a transaction script
/// to send the specified asset amount.
///
/// # Arguments
/// * `client` - The client instance
/// * `sender_account_id` - The account ID of the sender
/// * `recipient_account_id` - The account ID of the recipient
/// * `asset` - The fungible asset to transfer
/// * `note_package` - The compiled note package (e.g., p2id-note)
/// * `tx_script_package` - The compiled transaction script package
/// * `config` - Optional configuration for the transfer
///
/// # Returns
/// A tuple containing the transaction ID and the created Note for the recipient
pub async fn send_asset_to_account(
    client: &mut Client<FilesystemKeyStore<StdRng>>,
    sender_account_id: AccountId,
    recipient_account_id: AccountId,
    asset: FungibleAsset,
    note_package: Arc<Package>,
    tx_script_package: Arc<Package>,
    config: Option<AssetTransferConfig>,
) -> Result<(TransactionId, Note), ClientError> {
    let config = config.unwrap_or_default();

    // Create the p2id note for the recipient
    let p2id_note = create_note_from_package(
        client,
        note_package,
        sender_account_id,
        NoteCreationConfig {
            assets: miden_client::note::NoteAssets::new(vec![asset.into()]).unwrap(),
            inputs: vec![recipient_account_id.prefix().as_felt(), recipient_account_id.suffix()],
            note_type: config.note_type,
            tag: config.tag,
            execution_hint: config.execution_hint,
            aux: config.aux,
        },
    );

    let tx_script_program = tx_script_package.unwrap_program();
    let tx_script = TransactionScript::from_parts(
        tx_script_program.mast_forest().clone(),
        tx_script_program.entrypoint(),
    );

    // Prepare note recipient
    let program_hash = tx_script_program.hash();
    let serial_num = RpoRandomCoin::new(program_hash).draw_word();
    let inputs = NoteInputs::new(vec![
        recipient_account_id.prefix().as_felt(),
        recipient_account_id.suffix(),
    ])
    .unwrap();
    let note_recipient = NoteRecipient::new(serial_num, p2id_note.script().clone(), inputs);

    // Prepare commitment data
    let mut input: Vec<Felt> = vec![
        config.tag.into(),
        config.aux,
        config.note_type.into(),
        config.execution_hint.into(),
    ];
    let recipient_digest: [Felt; 4] = note_recipient.digest().into();
    input.extend(recipient_digest);

    let asset_arr: Word = asset.into();
    input.extend(asset_arr);

    let mut commitment: [Felt; 4] = miden_core::crypto::hash::Rpo256::hash_elements(&input).into();

    assert_eq!(input.len() % 4, 0, "input needs to be word-aligned");

    // Prepare advice map
    let mut advice_map = std::collections::BTreeMap::new();
    advice_map.insert(commitment.into(), input.clone());

    let recipients = vec![note_recipient.clone()];

    // NOTE: passed on the stack reversed
    commitment.reverse();

    let tx_request = TransactionRequestBuilder::new()
        .custom_script(tx_script)
        .script_arg(miden_core::Word::new(commitment))
        .expected_output_recipients(recipients)
        .extend_advice_map(advice_map)
        .build()
        .unwrap();

    let tx = client.new_transaction(sender_account_id, tx_request).await?;
    let tx_id = tx.executed_transaction().id();

    client.submit_transaction(tx).await?;

    // Create the Note that the recipient will consume
    let assets = miden_client::note::NoteAssets::new(vec![asset.into()]).unwrap();
    let metadata = NoteMetadata::new(
        sender_account_id,
        config.note_type,
        config.tag,
        config.execution_hint,
        config.aux,
    )
    .unwrap();
    let recipient_note = Note::new(assets, metadata, note_recipient);

    Ok((tx_id, recipient_note))
}
