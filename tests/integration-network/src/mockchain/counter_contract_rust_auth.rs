//! Counter contract test using an auth component compiled from Rust (RPO-Falcon512)
//!
//! This test ensures that an account which does not possess the correct
//! RPO-Falcon512 secret key cannot create notes on behalf of the counter
//! contract account that uses the Rust-compiled auth component.

use miden_client::{
    auth::BasicAuthenticator, crypto::RpoRandomCoin, note::NoteTag, testing::MockChain,
    transaction::OutputNote,
};
use miden_protocol::account::StorageSlotName;

use super::helpers::{
    NoteCreationConfig, assert_counter_storage, block_on, build_counter_account_with_rust_rpo_auth,
    build_send_notes_script, compile_rust_package, create_note_from_package,
};

/// Verify that another client (without the RPO-Falcon512 key) cannot create notes for
/// the counter account which uses the Rust-compiled RPO-Falcon512 authentication component.
#[ignore = "until https://github.com/0xMiden/compiler/issues/904 is fixed"]
#[test]
pub fn test_counter_contract_rust_auth_blocks_unauthorized_note_creation() {
    let contract_package = compile_rust_package("../../examples/counter-contract", true);
    let note_package = compile_rust_package("../../examples/counter-note", true);
    let rpo_auth_package =
        compile_rust_package("../../examples/auth-component-rpo-falcon512", true);

    let (counter_account, secret_key) =
        build_counter_account_with_rust_rpo_auth(contract_package, rpo_auth_package, [0_u8; 32]);
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

    let counter_storage_slot =
        StorageSlotName::new("miden::component::miden_counter_contract::count_map").unwrap();
    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        1,
    );

    // Positive check: original client (with the key) can create a note
    let mut rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
    let own_note = create_note_from_package(
        note_package.clone(),
        counter_account.id(),
        NoteCreationConfig {
            tag: NoteTag::with_account_target(counter_account.id()),
            ..Default::default()
        },
        &mut rng,
    );
    let tx_script = build_send_notes_script(&counter_account, std::slice::from_ref(&own_note));
    let authenticator = BasicAuthenticator::new(std::slice::from_ref(&secret_key));

    let tx_context_builder = chain
        .build_tx_context(counter_account.clone(), &[], &[])
        .unwrap()
        .tx_script(tx_script)
        .extend_expected_output_notes(vec![OutputNote::Full(own_note.clone())])
        .authenticator(Some(authenticator));
    let tx_context = tx_context_builder.build().unwrap();
    let executed_tx =
        block_on(tx_context.execute()).expect("authorized client should be able to create a note");
    assert_eq!(executed_tx.output_notes().num_notes(), 1);
    assert_eq!(executed_tx.output_notes().get_note(0).id(), own_note.id());

    chain.add_pending_executed_transaction(&executed_tx).unwrap();
    chain.prove_next_block().unwrap();

    // Negative check: without the RPO-Falcon512 key, creating output notes should fail.
    let counter_account = chain.committed_account(counter_account_id).unwrap().clone();
    let forged_note = create_note_from_package(
        note_package,
        counter_account.id(),
        NoteCreationConfig {
            tag: NoteTag::with_account_target(counter_account.id()),
            ..Default::default()
        },
        &mut rng,
    );
    let tx_script = build_send_notes_script(&counter_account, std::slice::from_ref(&forged_note));

    let tx_context_builder = chain
        .build_tx_context(counter_account, &[], &[])
        .unwrap()
        .tx_script(tx_script)
        .extend_expected_output_notes(vec![OutputNote::Full(forged_note)])
        .authenticator(None);
    let tx_context = tx_context_builder.build().unwrap();

    let result = block_on(tx_context.execute());
    assert!(
        result.is_err(),
        "Unauthorized executor unexpectedly created a transaction for the counter account"
    );
}
