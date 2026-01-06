//! Counter contract test using an auth component compiled from Rust (RPO-Falcon512)
//!
//! This test ensures that an account which does not possess the correct
//! RPO-Falcon512 secret key cannot create notes on behalf of the counter
//! contract account that uses the Rust-compiled auth component.

use std::{borrow::Borrow, collections::BTreeSet, sync::Arc};

use miden_client::{
    Word,
    account::component::BasicWallet,
    auth::{AuthSecretKey, BasicAuthenticator, PublicKeyCommitment},
    crypto::{FeltRng, RpoRandomCoin, rpo_falcon512::SecretKey},
    note::{
        Note, NoteAssets, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript,
        NoteTag, NoteType,
    },
    testing::MockChain,
    transaction::OutputNote,
    utils::Deserializable,
};
use miden_core::{Felt, FieldElement};
use miden_lib::account::interface::AccountInterface;
use miden_mast_package::{Package, SectionId};
use miden_objects::account::{
    Account, AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
    AccountId, AccountStorageMode, AccountType, StorageMap, StorageSlot,
};
use rand::{SeedableRng, rngs::StdRng};

use super::helpers::compile_rust_package;

/// Asserts the counter value stored in the counter contract component's storage map.
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

/// Builds an existing counter account using a Rust-compiled RPO-Falcon512 authentication component.
///
/// Returns the account along with the generated secret key which can authenticate transactions for
/// this account.
fn build_counter_account_with_rust_rpo_auth(
    component_package: Arc<Package>,
    auth_component_package: Arc<Package>,
) -> (Account, SecretKey) {
    let counter_component_metadata = component_package.sections.iter().find_map(|section| {
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
                component_package.unwrap_library().as_ref().clone(),
            );

            // Initialize the counter storage to 1 at key [0,0,0,1]
            let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
            let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
            let storage_slots =
                vec![StorageSlot::Map(StorageMap::with_entries([(key, value)]).unwrap())];

            AccountComponent::new(template.library().clone(), storage_slots)
                .unwrap()
                .with_supported_types(supported_types.clone())
        }
    };

    // Build the Rust-compiled auth component with public key commitment in slot 0.
    let mut rng = StdRng::seed_from_u64(1);
    let secret_key = SecretKey::with_rng(&mut rng);
    let pk_commitment: Word = PublicKeyCommitment::from(secret_key.public_key()).into();
    let auth_component = AccountComponent::new(
        auth_component_package.unwrap_library().as_ref().clone(),
        vec![StorageSlot::Value(pk_commitment)],
    )
    .unwrap()
    .with_supported_types(supported_types);

    let seed = [0_u8; 32];
    let account = AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .with_component(counter_component)
        .build_existing()
        .expect("failed to build counter account");

    (account, secret_key)
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

/// Builds a `send_notes` transaction script for a basic wallet account.
///
/// The resulting script creates the provided output notes and triggers the account's auth
/// component when output notes are produced.
fn build_send_notes_script(
    account: &Account,
    notes: &[Note],
) -> miden_objects::transaction::TransactionScript {
    let partial_notes =
        notes.iter().map(miden_objects::note::PartialNote::from).collect::<Vec<_>>();
    AccountInterface::from(account)
        .build_send_notes_script(&partial_notes, None, false)
        .expect("failed to build send_notes transaction script")
}

/// Verify that another client (without the RPO-Falcon512 key) cannot create notes for
/// the counter account which uses the Rust-compiled RPO-Falcon512 authentication component.
#[test]
pub fn test_counter_contract_rust_auth_blocks_unauthorized_note_creation() {
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);
    let rpo_auth_package =
        compile_rust_package("../../examples/auth-component-rpo-falcon512", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let (counter_account, secret_key) =
            build_counter_account_with_rust_rpo_auth(contract_package, rpo_auth_package);
        let counter_account_id = counter_account.id();

        let mut builder = MockChain::builder();
        builder
            .add_account(counter_account)
            .expect("failed to add counter account to mock chain builder");

        let mut chain = builder.build().expect("failed to build mock chain");
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        let counter_account = chain.committed_account(counter_account_id).unwrap().clone();
        eprintln!(
            "Counter account (Rust RPO-Falcon512 auth) ID: {:?}",
            counter_account.id().to_hex()
        );

        assert_counter_storage(chain.committed_account(counter_account.id()).unwrap().storage(), 1);

        // Positive check: original client (with the key) can create a note
        let mut rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
        let own_note = create_note_from_package(
            note_package.clone(),
            counter_account.id(),
            NoteTag::from_account_id(counter_account.id()),
            &mut rng,
        );
        let tx_script = build_send_notes_script(&counter_account, std::slice::from_ref(&own_note));
        let authenticator = BasicAuthenticator::new(&[AuthSecretKey::RpoFalcon512(secret_key)]);

        let tx_context_builder = chain
            .build_tx_context(counter_account.clone(), &[], &[])
            .unwrap()
            .tx_script(tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(own_note.clone())])
            .authenticator(Some(authenticator));
        let tx_context = tx_context_builder.build().unwrap();
        let executed_tx = tx_context
            .execute()
            .await
            .expect("authorized client should be able to create a note");
        assert_eq!(executed_tx.output_notes().num_notes(), 1);
        assert_eq!(executed_tx.output_notes().get_note(0).id(), own_note.id());

        chain.add_pending_executed_transaction(&executed_tx).unwrap();
        chain.prove_next_block().unwrap();

        // Negative check: without the RPO-Falcon512 key, creating output notes should fail.
        let counter_account = chain.committed_account(counter_account_id).unwrap().clone();
        let forged_note = create_note_from_package(
            note_package,
            counter_account.id(),
            NoteTag::from_account_id(counter_account.id()),
            &mut rng,
        );
        let tx_script =
            build_send_notes_script(&counter_account, std::slice::from_ref(&forged_note));

        let tx_context_builder = chain
            .build_tx_context(counter_account, &[], &[])
            .unwrap()
            .tx_script(tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(forged_note)])
            .authenticator(None);
        let tx_context = tx_context_builder.build().unwrap();

        let result = tx_context.execute().await;
        assert!(
            result.is_err(),
            "Unauthorized executor unexpectedly created a transaction for the counter account"
        );
    });
}
