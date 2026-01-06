//! Counter contract test module

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

    // The counter contract is in slot 1 when deployed, auth_component takes slot 0
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

/// Creates an account builder for an existing public account containing the counter contract
/// component.
fn build_counter_account_builder(
    contract_package: Arc<Package>,
    storage_slots: Vec<StorageSlot>,
) -> AccountBuilder {
    let account_component_metadata = contract_package.sections.iter().find_map(|section| {
        if section.id == SectionId::ACCOUNT_COMPONENT_METADATA {
            Some(section.data.borrow())
        } else {
            None
        }
    });

    let component = match account_component_metadata {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                contract_package.unwrap_library().as_ref().clone(),
            );

            let supported_types = BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode]);
            AccountComponent::new(template.library().clone(), storage_slots)
                .unwrap()
                .with_supported_types(supported_types)
        }
    };

    let seed = [0_u8; 32];
    AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(BasicWallet)
        .with_component(component)
}

/// Creates a note from a compiled note package without requiring a `Client` RNG.
fn create_note_from_package(package: Arc<Package>, sender_id: AccountId, tag: NoteTag) -> Note {
    let note_program = package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let serial_num = RpoRandomCoin::new(note_program.hash()).draw_word();
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

/// Tests the counter contract deployment and note consumption workflow on a mock chain.
#[test]
pub fn test_counter_contract_mockchain() {
    // Compile the contracts first (before creating any runtime)
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
        let storage_slots =
            vec![StorageSlot::Map(StorageMap::with_entries([(key, value)]).unwrap())];

        let mut builder = MockChain::builder();
        let counter_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_counter_account_builder(contract_package, storage_slots),
                AccountState::Exists,
            )
            .expect("failed to add counter account to mock chain builder");

        let counter_note = create_note_from_package(
            note_package,
            counter_account.id(),
            NoteTag::from_account_id(counter_account.id()),
        );
        builder.add_output_note(miden_client::transaction::OutputNote::Full(counter_note.clone()));

        let mut chain = builder.build().expect("failed to build mock chain");
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        eprintln!("Counter account ID: {:?}", counter_account.id().to_hex());

        // The counter contract storage value should be zero after the account creation
        assert_counter_storage(chain.committed_account(counter_account.id()).unwrap().storage(), 1);

        // Consume the note to increment the counter
        let tx_context_builder = chain
            .build_tx_context(counter_account.clone(), &[counter_note.id()], &[])
            .unwrap();
        let tx_context = tx_context_builder.build().unwrap();
        let executed_tx = tx_context.execute().await.unwrap();
        chain.add_pending_executed_transaction(&executed_tx).unwrap();
        chain.prove_next_block().unwrap();

        // The counter contract storage value should be 1 (incremented) after the note is consumed
        assert_counter_storage(chain.committed_account(counter_account.id()).unwrap().storage(), 2);
    });
}
