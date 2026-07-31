//! Common helper functions for mock-chain integration tests.

use std::{collections::BTreeMap, future::Future, path::Path, sync::Arc};

use miden_client::{
    Word,
    account::{
        StorageMapKey,
        component::{BasicWallet, InitStorageData, StorageValueName, WordValue},
    },
    asset::{FungibleAsset, StorageSlotContent},
    auth::AuthSecretKey,
    crypto::FeltRng,
    note::{Note, NoteType},
    transaction::RawOutputNote,
};
use miden_core::Felt;
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_mast_package::{Package, TargetType};
use miden_protocol::{
    account::{
        Account, AccountBuilder, AccountComponent, AccountId, AccountStorage, AccountType,
        StorageSlot, StorageSlotName,
    },
    asset::{Asset, AssetAmount},
    note::{NoteScript, PartialNote},
    transaction::{TransactionMeasurements, TransactionScript},
};
use miden_standards::{testing::note::NoteBuilder, tx_script::SendNotesTransactionScript};
use miden_testing::{MockChain, TransactionContextBuilder};
use miden_tx_script_args::{EncodedScriptArgs, ScriptArgs};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::CompilerTestBuilder;
use rand::{SeedableRng, rngs::StdRng};

/// Host-side mirror of the transaction-script arguments declared in
/// `examples/basic-wallet-tx-script`, spelled in the felt-repr primitives its field types encode
/// to (`Tag`/`NoteType` = one felt, `Recipient` = one word, `Asset` = key and value words); what
/// must match the script's struct is the felt-repr wire sequence, not the Rust fields.
#[derive(FromFeltRepr, ToFeltRepr)]
struct TxScriptArgs {
    tag: miden_field::Felt,
    note_type: miden_field::Felt,
    recipient: miden_field::Word,
    asset_key: miden_field::Word,
    asset_value: miden_field::Word,
}

/// Converts a value's felt representation into `miden_core::Felt` elements.
pub(crate) fn to_core_felts(value: &AccountId) -> Vec<Felt> {
    vec![value.prefix().as_felt(), value.suffix()]
}

// FIELD <-> PROTOCOL FELT CONVERSIONS
// ================================================================================================

/// Converts a protocol felt into the field-crate felt type.
pub(crate) fn to_field_felt(value: Felt) -> miden_field::Felt {
    miden_field::Felt::new(value.as_canonical_u64()).expect("protocol felt must be canonical")
}

/// Converts four protocol felts into the field-crate word type.
pub(crate) fn to_field_word(felts: [Felt; 4]) -> miden_field::Word {
    miden_field::Word::new(felts.map(to_field_felt))
}

/// Converts field-crate felts into protocol felts.
pub(crate) fn from_field_felts(felts: &[miden_field::Felt]) -> Vec<Felt> {
    felts.iter().map(|felt| Felt::new_unchecked(felt.as_canonical_u64())).collect()
}

/// Converts a field-crate word into a protocol word.
pub(crate) fn from_field_word(word: miden_field::Word) -> Word {
    Word::new([
        Felt::new_unchecked(word[0].as_canonical_u64()),
        Felt::new_unchecked(word[1].as_canonical_u64()),
        Felt::new_unchecked(word[2].as_canonical_u64()),
        Felt::new_unchecked(word[3].as_canonical_u64()),
    ])
}

/// Asserts the scalar counter value stored in an account's storage map at `storage_key`.
pub(crate) fn assert_counter_storage_at_key(
    account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    storage_key: Word,
    expected: u64,
) {
    let storage_key = StorageMapKey::from_raw(storage_key);
    let word = account_storage
        .get_map_item(storage_slot, storage_key)
        .expect("failed to get counter value from storage slot");

    // `AccountStorage` exposes scalar felt values as `[felt, 0, 0, 0]`.
    let value = word[0].as_canonical_u64();
    assert_eq!(value, expected, "counter value mismatch: expected {expected}, got {value}");
}

// ASYNC HELPERS
// ================================================================================================

thread_local! {
    static TOKIO_RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new()
        .expect("failed to build tokio runtime for integration-network tests");
}

/// Runs the provided future to completion on a shared Tokio runtime.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    TOKIO_RUNTIME.with(|rt| rt.block_on(future))
}

// COMPILATION
// ================================================================================================

pub(crate) fn compile_rust_package(project_path: impl AsRef<Path>, release: bool) -> Arc<Package> {
    let project_path = project_path.as_ref();
    let config = WasmTranslationConfig::default();
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(project_path, config, []);

    if release {
        builder.with_release(true);
    }

    let mut test = builder.build();
    let package = test.compile_package();
    let profile = if release { "release" } else { "debug" };
    package
        .write_masp_file(project_path.join("target").join("miden").join(profile))
        .expect("failed to persist compiled Miden package");

    package
}

/// Returns the root of the note script exported by the compiled package.
pub(crate) fn note_script_root(package: &Package) -> Word {
    NoteScript::from_package(package)
        .expect("compiled package should contain exactly one note script export")
        .root()
        .into()
}

/// Builds a transaction script from a compiled transaction-script package.
pub(crate) fn transaction_script_from_package(package: &Package) -> TransactionScript {
    assert_eq!(
        package.kind,
        TargetType::TransactionScript,
        "expected a transaction-script package"
    );

    TransactionScript::from_library(package).expect("invalid transaction-script package")
}

/// Builds a transaction script from a compiled transaction-script package, linking in the MAST
/// forests of the provided Miden package dependencies.
///
/// The compiler links Miden package dependencies as libraries: calls into them are external MAST
/// nodes referencing the dependency's procedure digests. Merging the dependency forests into the
/// transaction script's forest makes those procedures resolvable during execution.
pub(crate) fn transaction_script_from_package_with_deps(
    package: &Package,
    dependencies: &[&Package],
) -> TransactionScript {
    let base = transaction_script_from_package(package);
    if dependencies.is_empty() {
        return base;
    }

    let entrypoint_digest: Word = base.root().into();
    let dep_forests: Vec<_> = dependencies.iter().map(|dep| dep.mast_forest().clone()).collect();
    let mut forests = vec![base.mast()];
    forests.extend(dep_forests);

    let (merged, _root_map) = miden_core::mast::MastForest::merge(forests.iter().map(Arc::as_ref))
        .expect("failed to merge dependency MAST forests into the transaction script");
    let entrypoint = merged
        .find_procedure_root(entrypoint_digest)
        .expect("transaction script entrypoint should survive the MAST forest merge");

    TransactionScript::from_parts(Arc::new(merged), entrypoint)
}

// ================================================================================================
// ACCOUNT COMPONENT HELPERS
// ================================================================================================

/// Asserts that the account vault contains a fungible asset from the expected faucet with the
/// expected total amount.
pub(crate) fn assert_account_has_fungible_asset(
    account: &Account,
    expected_faucet_id: AccountId,
    expected_amount: u64,
) {
    let expected_amount =
        AssetAmount::new(expected_amount).expect("expected amount should be a valid asset amount");
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
            fungible_asset.amount().as_u64()
        ),
        None => {
            panic!("Account does not contain a fungible asset from faucet {expected_faucet_id}")
        }
    }
}

/// Builds a `send_notes` transaction script for accounts that support a standard note creation
/// interface (e.g. basic wallets and basic fungible faucets).
pub(crate) fn build_send_notes_script(
    account: &Account,
    notes: &[Note],
) -> SendNotesTransactionScript {
    let partial_notes = notes.iter().cloned().map(PartialNote::from).collect::<Vec<_>>();

    SendNotesTransactionScript::new(&account.code_interface(), &partial_notes).unwrap()
}

/// Applies encoded transaction-script arguments to a transaction context.
///
/// Word-mode arguments pass through the script-args word directly; commitment-mode arguments
/// hash the preimage into the script-args word and register the matching advice-map entry.
pub(crate) fn apply_script_args(
    builder: TransactionContextBuilder,
    args: &impl ScriptArgs,
) -> TransactionContextBuilder {
    match args.encode() {
        EncodedScriptArgs::Word(word) => builder.tx_script_args(from_field_word(word)),
        EncodedScriptArgs::Preimage(felts) => {
            let preimage = from_field_felts(&felts);
            let args_word: Word = miden_core::crypto::hash::Poseidon2::hash_elements(&preimage);
            builder.tx_script_args(args_word).extend_advice_map([(args_word, preimage)])
        }
    }
}

/// Executes a transaction context and expects the execution to fail, returning the error message.
pub(crate) fn execute_tx_expect_failure(tx_context_builder: TransactionContextBuilder) -> String {
    let tx_context = tx_context_builder.build().unwrap();
    match block_on(tx_context.execute()) {
        Ok(_) => panic!("expected transaction execution to fail"),
        Err(err) => err.to_string(),
    }
}

/// Executes a transaction context against the chain and commits it in the next block.
///
/// Returns the transaction measurements captured during execution.
pub(crate) fn execute_tx(
    chain: &mut MockChain,
    tx_context_builder: TransactionContextBuilder,
) -> TransactionMeasurements {
    let tx_context = tx_context_builder.build().unwrap();
    let executed_tx = block_on(tx_context.execute()).unwrap_or_else(|err| panic!("{err}"));

    let measurements = executed_tx.measurements().clone();

    chain.add_pending_executed_transaction(&executed_tx).unwrap();
    chain.prove_next_block().unwrap();

    measurements
}

/// Builds a transaction context which transfers an asset from `sender_id` to `recipient_id` using
/// the custom transaction script package.
///
/// Builds the transaction context by constructing the same advice-map + script-arg commitment
/// expected by the tx script, without requiring a `miden_client::Client`.
///
/// The caller provides an RNG used to generate a unique note serial number, to avoid accidental
/// note ID collisions across multiple transfers.
pub(crate) fn build_asset_transfer_tx(
    chain: &MockChain,
    sender_id: AccountId,
    recipient_id: AccountId,
    asset: FungibleAsset,
    p2id_note_package: Arc<Package>,
    tx_script_package: Arc<Package>,
    rng: &mut impl FeltRng,
) -> (TransactionContextBuilder, Note) {
    let tx_script = transaction_script_from_package(&tx_script_package);

    let serial_num = rng.draw_word();
    let faucet_id = asset.faucet_id();

    let asset: Asset = asset.into();
    let output_note = NoteBuilder::new(sender_id, rng)
        .serial_number(serial_num)
        .package((*p2id_note_package).clone())
        .note_storage(to_core_felts(&recipient_id))
        .unwrap()
        .add_assets([asset])
        .tag(0)
        .build()
        .unwrap();

    // Build the script arguments through the mirror `TxScriptArgs` struct — the same encoding the
    // tx script decodes, so the host-side layout cannot drift from the guest side.
    let recipient_digest: [Felt; 4] = output_note.recipient().digest().into();
    let asset_elements = asset.as_elements();
    let asset_key: [Felt; 4] = asset_elements[..4].try_into().unwrap();
    let asset_value: [Felt; 4] = asset_elements[4..].try_into().unwrap();
    let script_args = TxScriptArgs {
        tag: to_field_felt(Felt::ZERO),
        note_type: to_field_felt(Felt::from(NoteType::Public)),
        recipient: to_field_word(recipient_digest),
        asset_key: to_field_word(asset_key),
        asset_value: to_field_word(asset_value),
    };

    let tx_context_builder = chain
        .build_tx_context(sender_id, &[], &[])
        .unwrap()
        .foreign_accounts(vec![chain.get_foreign_account_inputs(faucet_id).unwrap()])
        .tx_script(tx_script);
    let tx_context_builder = apply_script_args(tx_context_builder, &script_args)
        .extend_expected_output_notes(vec![RawOutputNote::Full(output_note.clone())]);

    (tx_context_builder, output_note)
}

// COUNTER CONTRACT HELPERS
// ================================================================================================

/// Returns the storage slot name used by the counter contract's storage map.
///
/// Slot names derive from the `[lib].namespace` interface segment, so this tracks the
/// `counter-contract` interface of the `counter-contract` example.
pub(crate) fn counter_storage_slot_name() -> StorageSlotName {
    StorageSlotName::new("counter_contract::counter_contract::count_map")
        .expect("counter storage slot name should be valid")
}

fn auth_public_key_slot_name() -> StorageSlotName {
    StorageSlotName::new("auth_component_rpo_falcon512::auth_component::owner_public_key")
        .expect("auth component storage slot name should be valid")
}

pub const COUNTER_CONTRACT_STORAGE_KEY: Word =
    Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

/// Asserts the counter value stored in the counter contract's storage map at `storage_slot`.
pub(crate) fn assert_counter_storage(
    counter_account_storage: &AccountStorage,
    storage_slot: &StorageSlotName,
    expected: u64,
) {
    let key = StorageMapKey::from_raw(COUNTER_CONTRACT_STORAGE_KEY);
    let word = counter_account_storage
        .get_map_item(storage_slot, key)
        .expect("Failed to get counter value from storage slot");

    // `AccountStorage` exposes scalar felt values as `[felt, 0, 0, 0]`.
    let val = word[0];
    assert_eq!(
        val.as_canonical_u64(),
        expected,
        "Counter value mismatch. Expected: {}, Got: {}",
        expected,
        val.as_canonical_u64()
    );
}

/// Builds an account builder for an existing public counter account containing the counter
/// contract component and a custom authentication component compiled as a package library.
pub(crate) fn build_existing_counter_account_builder_with_auth_package(
    counter_component: AccountComponent,
    auth_component_package: Arc<Package>,
    auth_storage_slots: Vec<StorageSlot>,
    seed: [u8; 32],
) -> AccountBuilder {
    let mut values = BTreeMap::default();
    let mut map_entries = BTreeMap::<_, Vec<(WordValue, WordValue)>>::default();
    for slot in auth_storage_slots {
        let (name, content) = slot.into_parts();
        match content {
            StorageSlotContent::Value(value) => {
                values.insert(StorageValueName::from_slot_name(&name), value.into());
            }
            StorageSlotContent::Map(map) => {
                let entries = map.into_entries();
                map_entries.entry(name).or_default().extend(
                    entries
                        .into_iter()
                        .map(|(k, v)| (k.as_word().into(), v.into()))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
    let init_storage_data =
        InitStorageData::new(values, map_entries).expect("invalid init storage data");
    let auth_component =
        AccountComponent::from_package(&auth_component_package, &init_storage_data).unwrap();

    AccountBuilder::new(seed)
        .account_type(AccountType::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .with_component(counter_component)
}

/// Builds an existing counter account using a Rust-compiled RPO-Falcon512 authentication component.
///
/// Returns the account along with the generated secret key which can authenticate transactions for
/// this account.
pub(crate) fn build_counter_account_with_rust_rpo_auth(
    component_package: Arc<Package>,
    auth_component_package: Arc<Package>,
    seed: [u8; 32],
) -> (Account, AuthSecretKey) {
    let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let mut counter_init_storage_data = InitStorageData::default();
    counter_init_storage_data
        .insert_map_entry(counter_storage_slot_name(), key, 1_u64)
        .expect("failed to insert counter map entry");

    let counter_component =
        AccountComponent::from_package(&component_package, &counter_init_storage_data).unwrap();

    let mut rng = StdRng::seed_from_u64(1);
    let secret_key = AuthSecretKey::new_falcon512_poseidon2_with_rng(&mut rng);
    let pk_commitment: Word = secret_key.public_key().to_commitment().into();

    let auth_storage_slots =
        vec![StorageSlot::with_value(auth_public_key_slot_name(), pk_commitment)];

    let account = build_existing_counter_account_builder_with_auth_package(
        counter_component,
        auth_component_package,
        auth_storage_slots,
        seed,
    )
    .build_existing()
    .expect("failed to build counter account");

    (account, secret_key)
}

#[cfg(test)]
mod tests {
    use miden_tx_script_args::ScriptArgs;

    use super::TxScriptArgs;

    /// Pins the mirror's encoded size so layout drift vs the guest struct in
    /// `examples/basic-wallet-tx-script` fails loudly (tag + note type + recipient word +
    /// asset key and value words = 14 felts).
    #[test]
    fn tx_script_arg_mirror_has_the_guest_layout_size() {
        assert_eq!(<TxScriptArgs as ScriptArgs>::FIXED_LEN, Some(14));
    }

    /// Pins the mirror's wire sequence with distinct sentinels, catching same-length field
    /// reorders that the size pin cannot.
    #[test]
    fn tx_script_arg_mirror_has_the_guest_wire_sequence() {
        let felt = |value: u64| miden_field::Felt::new(value).unwrap();
        let word = |base: u64| {
            miden_field::Word::new([felt(base), felt(base + 1), felt(base + 2), felt(base + 3)])
        };

        let args = TxScriptArgs {
            tag: felt(1),
            note_type: felt(2),
            recipient: word(10),
            asset_key: word(20),
            asset_value: word(30),
        };

        let expected: Vec<miden_field::Felt> =
            [1, 2, 10, 11, 12, 13, 20, 21, 22, 23, 30, 31, 32, 33]
                .into_iter()
                .map(felt)
                .collect();
        assert_eq!(miden_field_repr::ToFeltRepr::to_felt_repr(&args), expected);
    }
}
