//! Counter contract test with no-auth authentication component

use std::{borrow::Borrow, collections::BTreeSet, sync::Arc};

use miden_client::{
    Word,
    account::component::BasicWallet,
    crypto::{FeltRng, RpoRandomCoin},
    note::{
        Note, NoteAssets, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript,
        NoteTag, NoteType,
    },
    testing::{AccountState, Auth, MockChain},
    transaction::OutputNote,
    utils::Deserializable,
};
use miden_core::{Felt, FieldElement};
use miden_mast_package::{Package, SectionId};
use miden_objects::account::{
    AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
    AccountId, AccountStorageMode, AccountType, StorageMap, StorageSlot,
};

use super::helpers::compile_rust_package;

/// Asserts the counter value stored in the counter contract component's storage map.
fn assert_counter_storage(
    counter_account_storage: &miden_client::account::AccountStorage,
    expected: u64,
) {
    // according to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

    // With no-auth auth component (no storage), the counter component occupies slot 0
    let word = counter_account_storage
        .get_map_item(0, counter_contract_storage_key)
        .expect("Failed to get counter value from storage slot 0");

    let val = word.last().unwrap();
    assert_eq!(
        val.as_int(),
        expected,
        "Counter value mismatch. Expected: {}, Got: {}",
        expected,
        val.as_int()
    );
}

/// Creates an account builder for an existing public account containing the counter contract and a
/// no-auth authentication component.
fn build_counter_account_builder(
    contract_package: Arc<Package>,
    auth_component_package: Arc<Package>,
    storage_slots: Vec<StorageSlot>,
) -> AccountBuilder {
    let counter_component_metadata = contract_package.sections.iter().find_map(|section| {
        if section.id == SectionId::ACCOUNT_COMPONENT_METADATA {
            Some(section.data.borrow())
        } else {
            None
        }
    });

    let supported_types = BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode]);

    let counter_component = match counter_component_metadata {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                contract_package.unwrap_library().as_ref().clone(),
            );

            AccountComponent::new(template.library().clone(), storage_slots)
                .unwrap()
                .with_supported_types(supported_types.clone())
        }
    };

    let auth_component =
        AccountComponent::new(auth_component_package.unwrap_library().as_ref().clone(), vec![])
            .unwrap()
            .with_supported_types(supported_types);

    let seed = [0_u8; 32];
    AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .with_component(counter_component)
}

/// Creates a note from a compiled note package without requiring a `Client` RNG.
fn create_note_from_package(
    package: Arc<Package>,
    sender_id: AccountId,
    tag: NoteTag,
    rng: &mut impl FeltRng,
) -> Note {
    let note_program = package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let serial_num = rng.draw_word();
    let recipient = NoteRecipient::new(serial_num, note_script, NoteInputs::new(vec![]).unwrap());

    let metadata = NoteMetadata::new(
        sender_id,
        NoteType::Public,
        tag,
        NoteExecutionHint::always(),
        Felt::ZERO,
    )
    .unwrap();

    Note::new(NoteAssets::default(), metadata, recipient)
}

/// Tests the counter contract with a "no-auth" authentication component.
///
/// Flow:
/// - Build counter account using `examples/auth-component-no-auth` as the auth component
/// - Build a separate sender account (basic wallet)
/// - Sender issues a counter note to the network
/// - Counter account consumes the note without requiring authentication/signature
#[test]
pub fn test_counter_contract_no_auth_mockchain() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);
    let no_auth_auth_component =
        compile_rust_package("../../examples/auth-component-no-auth", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let storage_slots =
            vec![StorageSlot::Map(StorageMap::with_entries([(key, value)]).unwrap())];

        let mut builder = MockChain::builder();

        let counter_account =
            build_counter_account_builder(contract_package, no_auth_auth_component, storage_slots)
                .build_existing()
                .expect("failed to build counter account");
        builder
            .add_account(counter_account.clone())
            .expect("failed to add counter account to mock chain builder");
        eprintln!("Counter account (no-auth) ID: {:?}", counter_account.id().to_hex());

        // Create a separate sender account using only the BasicWallet component
        let seed = [1_u8; 32];
        let sender_builder = AccountBuilder::new(seed)
            .account_type(AccountType::RegularAccountUpdatableCode)
            .storage_mode(AccountStorageMode::Public)
            .with_component(BasicWallet);
        let sender_account = builder
            .add_account_from_builder(Auth::BasicAuth, sender_builder, AccountState::Exists)
            .expect("failed to add sender account to mock chain builder");
        eprintln!("Sender account ID: {:?}", sender_account.id().to_hex());

        // Sender creates the counter note (note script increments counter's storage on consumption)
        let mut rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
        let counter_note = create_note_from_package(
            note_package.clone(),
            sender_account.id(),
            NoteTag::from_account_id(counter_account.id()),
            &mut rng,
        );
        eprintln!("Counter note hash: {:?}", counter_note.id().to_hex());
        builder.add_output_note(OutputNote::Full(counter_note.clone()));

        let mut chain = builder.build().expect("failed to build mock chain");
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        assert_counter_storage(chain.committed_account(counter_account.id()).unwrap().storage(), 1);

        // Consume the note with the counter account (no signature/auth required).
        let tx_context_builder = chain
            .build_tx_context(counter_account.clone(), &[counter_note.id()], &[])
            .unwrap();
        let tx_context = tx_context_builder.build().unwrap();
        let executed_tx = tx_context.execute().await.unwrap();
        chain.add_pending_executed_transaction(&executed_tx).unwrap();
        chain.prove_next_block().unwrap();

        // The counter contract storage value should be 2 after the note is consumed
        assert_counter_storage(chain.committed_account(counter_account.id()).unwrap().storage(), 2);
    });
}
