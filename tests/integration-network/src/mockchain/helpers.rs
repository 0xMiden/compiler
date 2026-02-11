//! Common helper functions for mock-chain integration tests.

use std::{collections::BTreeSet, future::Future, sync::Arc};

use miden_client::{
    Word,
    account::component::BasicWallet,
    asset::FungibleAsset,
    auth::AuthSecretKey,
    crypto::FeltRng,
    note::{
        Note, NoteAssets, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    testing::{MockChain, TransactionContextBuilder},
    transaction::OutputNote,
};
use miden_core::{Felt, FieldElement, crypto::hash::Rpo256};
use miden_integration_tests::CompilerTestBuilder;
use miden_mast_package::Package;
use miden_protocol::{
    account::{
        Account, AccountBuilder, AccountComponent, AccountComponentMetadata, AccountId,
        AccountStorage, AccountStorageMode, AccountType, StorageMap, StorageSlot, StorageSlotName,
    },
    asset::Asset,
    note::PartialNote,
    transaction::TransactionScript,
};
use miden_standards::account::interface::{AccountInterface, AccountInterfaceExt};
use midenc_frontend_wasm::WasmTranslationConfig;
use rand::{SeedableRng, rngs::StdRng};

/// Converts a value's felt representation into `miden_core::Felt` elements.
pub(super) fn to_core_felts(value: &AccountId) -> Vec<Felt> {
    vec![value.prefix().as_felt(), value.suffix()]
}

// ASYNC HELPERS
// ================================================================================================

thread_local! {
    static TOKIO_RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new()
        .expect("failed to build tokio runtime for integration-network tests");
}

/// Runs the provided future to completion on a shared Tokio runtime.
pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    TOKIO_RUNTIME.with(|rt| rt.block_on(future))
}

// COMPILATION
// ================================================================================================

/// Helper to compile a Rust package to a Miden `Package`.
pub(super) fn compile_rust_package(package_path: &str, release: bool) -> Arc<Package> {
    let config = WasmTranslationConfig::default();
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(package_path, config, []);

    if release {
        builder.with_release(true);
    }

    let mut test = builder.build();
    test.compiled_package()
}

// NOTE HELPERS
// ================================================================================================

/// Configuration for creating a note.
#[derive(Debug, Clone)]
pub(super) struct NoteCreationConfig {
    /// The note type (public/private).
    pub note_type: NoteType,
    /// The note tag.
    pub tag: NoteTag,
    /// Assets carried by the note.
    pub assets: NoteAssets,
    /// Note inputs (e.g. target account id, timelock/reclaim height, etc.).
    pub inputs: Vec<Felt>,
}

impl Default for NoteCreationConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::new(0),
            assets: Default::default(),
            inputs: Default::default(),
        }
    }
}

/// Creates a note from a compiled note package without requiring a `miden_client::Client`.
pub(super) fn create_note_from_package(
    package: Arc<Package>,
    sender_id: AccountId,
    config: NoteCreationConfig,
    rng: &mut impl FeltRng,
) -> Note {
    let note_program = package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let serial_num = rng.draw_word();
    let note_inputs = NoteInputs::new(config.inputs).unwrap();
    let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

    let metadata = NoteMetadata::new(sender_id, config.note_type, config.tag);

    Note::new(config.assets, metadata, recipient)
}

// ACCOUNT COMPONENT HELPERS
// ================================================================================================

/// Creates an account component from a compiled package's component metadata.
pub(super) fn account_component_from_package(
    package: Arc<Package>,
    storage_slots: Vec<StorageSlot>,
) -> AccountComponent {
    let metadata = AccountComponentMetadata::try_from(package.as_ref())
        .expect("no account component metadata present");
    AccountComponent::new(package.unwrap_library().as_ref().clone(), storage_slots)
        .unwrap()
        .with_metadata(metadata)
}

// BASIC WALLET HELPERS
// ================================================================================================

/// Builds an account builder for an existing basic-wallet account based on the provided component
/// package.
pub(super) fn build_existing_basic_wallet_account_builder(
    wallet_package: Arc<Package>,
    with_std_basic_wallet: bool,
    seed: [u8; 32],
) -> AccountBuilder {
    let wallet_component = account_component_from_package(wallet_package, vec![]);

    let mut builder = AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(wallet_component);

    if with_std_basic_wallet {
        builder = builder.with_component(BasicWallet);
    }

    builder
}

/// Asserts that the account vault contains a fungible asset from the expected faucet with the
/// expected total amount.
pub(super) fn assert_account_has_fungible_asset(
    account: &Account,
    expected_faucet_id: AccountId,
    expected_amount: u64,
) {
    let found_asset = account.vault().assets().find_map(|asset| match asset {
        Asset::Fungible(fungible_asset) if fungible_asset.faucet_id() == expected_faucet_id => {
            Some(fungible_asset)
        }
        _ => None,
    });

    match found_asset {
        Some(fungible_asset) => assert_eq!(
            fungible_asset.amount(),
            expected_amount,
            "Found asset from faucet {expected_faucet_id} but amount {} doesn't match expected \
             {expected_amount}",
            fungible_asset.amount()
        ),
        None => {
            panic!("Account does not contain a fungible asset from faucet {expected_faucet_id}")
        }
    }
}

/// Builds a `send_notes` transaction script for accounts that support a standard note creation
/// interface (e.g. basic wallets and basic fungible faucets).
pub(super) fn build_send_notes_script(account: &Account, notes: &[Note]) -> TransactionScript {
    let partial_notes = notes.iter().cloned().map(PartialNote::from).collect::<Vec<_>>();

    AccountInterface::from_account(account)
        .build_send_notes_script(&partial_notes, None)
        .expect("failed to build send_notes transaction script")
}

/// Executes a transaction context against the chain and commits it in the next block.
pub(super) fn execute_tx(chain: &mut MockChain, tx_context_builder: TransactionContextBuilder) {
    let tx_context = tx_context_builder.build().unwrap();
    let executed_tx = block_on(tx_context.execute()).unwrap_or_else(|err| panic!("{err}"));
    chain.add_pending_executed_transaction(&executed_tx).unwrap();
    chain.prove_next_block().unwrap();
}

/// Builds a transaction context which transfers an asset from `sender_id` to `recipient_id` using
/// the custom transaction script package.
///
/// Builds the transaction context by constructing the same advice-map + script-arg commitment
/// expected by the tx script, without requiring a `miden_client::Client`.
///
/// The caller provides an RNG used to generate a unique note serial number, to avoid accidental
/// note ID collisions across multiple transfers.
pub(super) fn build_asset_transfer_tx(
    chain: &MockChain,
    sender_id: AccountId,
    recipient_id: AccountId,
    asset: FungibleAsset,
    p2id_note_package: Arc<Package>,
    tx_script_package: Arc<Package>,
    rng: &mut impl FeltRng,
) -> (TransactionContextBuilder, Note) {
    let note_program = p2id_note_package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let tx_script_program = tx_script_package.unwrap_program();
    let tx_script = TransactionScript::from_parts(
        tx_script_program.mast_forest().clone(),
        tx_script_program.entrypoint(),
    );

    let serial_num = rng.draw_word();
    let inputs = NoteInputs::new(to_core_felts(&recipient_id)).unwrap();
    let note_recipient = NoteRecipient::new(serial_num, note_script, inputs);

    let config = NoteCreationConfig {
        assets: NoteAssets::new(vec![asset.into()]).unwrap(),
        ..Default::default()
    };
    let metadata = NoteMetadata::new(sender_id, config.note_type, config.tag);
    let output_note = Note::new(config.assets, metadata, note_recipient.clone());

    // Prepare commitment data
    // This must match the input layout expected by `examples/basic-wallet-tx-script`.
    let mut commitment_input: Vec<Felt> = vec![config.tag.into(), Felt::from(config.note_type)];
    let recipient_digest: [Felt; 4] = note_recipient.digest().into();
    commitment_input.extend(recipient_digest);

    let asset_arr: Word = asset.into();
    commitment_input.extend(asset_arr);
    // Ensure word alignment for `adv_load_preimage` in the tx script.
    commitment_input.extend([Felt::ZERO, Felt::ZERO]);

    let commitment_key: Word = Rpo256::hash_elements(&commitment_input);
    assert_eq!(commitment_input.len() % 4, 0, "commitment input needs to be word-aligned");

    // NOTE: passed on the stack reversed
    let mut commitment_arg = commitment_key;
    commitment_arg.reverse();

    let tx_context_builder = chain
        .build_tx_context(sender_id, &[], &[])
        .unwrap()
        .tx_script(tx_script)
        .tx_script_args(commitment_arg)
        .extend_advice_map([(commitment_key, commitment_input)])
        .extend_expected_output_notes(vec![OutputNote::Full(output_note.clone())]);

    (tx_context_builder, output_note)
}

// COUNTER CONTRACT HELPERS
// ================================================================================================

fn counter_storage_slot_name() -> StorageSlotName {
    StorageSlotName::new("miden::component::miden_counter_contract::count_map")
        .expect("counter storage slot name should be valid")
}

fn auth_public_key_slot_name() -> StorageSlotName {
    StorageSlotName::new("miden::component::miden_auth_component_rpo_falcon512::owner_public_key")
        .expect("auth component storage slot name should be valid")
}

/// Asserts the counter value stored in the counter contract's storage map at `storage_slot`.
pub(super) fn assert_counter_storage(
    counter_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    expected: u64,
) {
    // according to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

    let word = counter_account_storage
        .get_map_item(storage_slot, counter_contract_storage_key)
        .expect("Failed to get counter value from storage slot");

    let val = word.last().unwrap();
    assert_eq!(
        val.as_int(),
        expected,
        "Counter value mismatch. Expected: {}, Got: {}",
        expected,
        val.as_int()
    );
}

/// Builds an account builder for an existing public counter account containing the counter
/// contract component and a custom authentication component compiled as a package library.
pub(super) fn build_existing_counter_account_builder_with_auth_package(
    contract_package: Arc<Package>,
    auth_component_package: Arc<Package>,
    auth_storage_slots: Vec<StorageSlot>,
    counter_storage_slots: Vec<StorageSlot>,
    seed: [u8; 32],
) -> AccountBuilder {
    let supported_types = BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode]);
    let auth_component = AccountComponent::new(
        auth_component_package.unwrap_library().as_ref().clone(),
        auth_storage_slots,
    )
    .unwrap()
    .with_supported_types(supported_types);
    let counter_component = account_component_from_package(contract_package, counter_storage_slots);

    AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .with_component(counter_component)
}

/// Builds an existing counter account using a Rust-compiled RPO-Falcon512 authentication component.
///
/// Returns the account along with the generated secret key which can authenticate transactions for
/// this account.
pub(super) fn build_counter_account_with_rust_rpo_auth(
    component_package: Arc<Package>,
    auth_component_package: Arc<Package>,
    seed: [u8; 32],
) -> (Account, AuthSecretKey) {
    let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let counter_storage_slots = vec![StorageSlot::with_map(
        counter_storage_slot_name(),
        StorageMap::with_entries([(key, value)]).unwrap(),
    )];

    let mut rng = StdRng::seed_from_u64(1);
    let secret_key = AuthSecretKey::new_falcon512_rpo_with_rng(&mut rng);
    let pk_commitment: Word = secret_key.public_key().to_commitment().into();

    let auth_storage_slots =
        vec![StorageSlot::with_value(auth_public_key_slot_name(), pk_commitment)];

    let account = build_existing_counter_account_builder_with_auth_package(
        component_package,
        auth_component_package,
        auth_storage_slots,
        counter_storage_slots,
        seed,
    )
    .build_existing()
    .expect("failed to build counter account");

    (account, secret_key)
}
