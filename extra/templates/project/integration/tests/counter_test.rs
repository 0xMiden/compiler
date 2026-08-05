use std::{path::Path, sync::Arc};

use anyhow::Context;
use integration::helpers::{build_project_in_dir, counter_storage_slot, COUNTER_STORAGE_KEY};
use miden_client::{
    account::{component::InitStorageData, AccountBuilder, AccountComponent, AccountType},
    auth::AuthSchemeId,
    crypto::RandomCoin,
    note::NoteScript,
    transaction::RawOutputNote,
    Word,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{AccountState, Auth, MockChain};

#[tokio::test]
async fn counter_test() -> anyhow::Result<()> {
    // Test that after executing the increment note, the counter value is incremented by 1
    let mut builder = MockChain::builder();

    // Create note sender account
    let sender = builder.add_existing_wallet(Auth::BasicAuth {
        auth_scheme: AuthSchemeId::Falcon512Poseidon2,
    })?;

    // Build contracts
    let contract_package = Arc::new(build_project_in_dir(
        Path::new("../contracts/counter-account"),
        true,
    )?);
    let note_package = Arc::new(build_project_in_dir(
        Path::new("../contracts/increment-note"),
        true,
    )?);

    // Create the counter account with its initial storage through the component schema.
    let counter_storage_slot = counter_storage_slot()?;
    let mut init_storage_data = InitStorageData::default();
    init_storage_data.insert_map_entry(counter_storage_slot.clone(), COUNTER_STORAGE_KEY, 0_u64)?;

    let counter_component = AccountComponent::from_package(&contract_package, &init_storage_data)
        .context("failed to build account component from counter package")?;
    let counter_account = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthSchemeId::Falcon512Poseidon2,
        },
        AccountBuilder::new([3_u8; 32])
            .account_type(AccountType::Public)
            .with_component(counter_component),
        AccountState::Exists,
    )?;

    let mut note_rng = RandomCoin::new(Word::from(
        NoteScript::from_package(note_package.as_ref())
            .context("failed to build note script from package")?
            .root(),
    ));
    let counter_note = NoteBuilder::new(sender.id(), &mut note_rng)
        .package((*note_package).clone())
        .build()
        .context("failed to build counter note from package")?;

    // add counter account and note to mockchain
    builder.add_output_note(RawOutputNote::Full(counter_note.clone()));

    // Build the mock chain
    let mut mock_chain = builder.build()?;

    // Build the transaction context
    let tx_context = mock_chain
        .build_tx_context(counter_account.clone(), &[counter_note.id()], &[])?
        .build()?;

    // Execute the transaction
    let executed_transaction = tx_context.execute().await?;

    // Add the executed transaction to the mockchain
    mock_chain.add_pending_executed_transaction(&executed_transaction)?;
    mock_chain.prove_next_block()?;

    // Get the count from the updated counter account
    let count = mock_chain
        .committed_account(counter_account.id())?
        .storage()
        .get_map_item(&counter_storage_slot, COUNTER_STORAGE_KEY)
        .expect("Failed to get counter value from storage slot");

    // Map values are returned as scalar words in `[value, 0, 0, 0]` layout.
    assert_eq!(
        count[0].as_canonical_u64(),
        1,
        "Count value is not equal to 1"
    );
    Ok(())
}
