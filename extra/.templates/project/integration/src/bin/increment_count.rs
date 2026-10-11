use integration::helpers::{
    build_project_in_dir, counter_storage_slot, create_account_from_package,
    create_basic_wallet_account, setup_client, AccountCreationConfig, ClientSetup,
    COUNTER_STORAGE_KEY,
};

use anyhow::{Context, Result};
use miden_client::{account::component::InitStorageData, transaction::TransactionRequestBuilder};
use miden_standards::testing::note::NoteBuilder;
use std::{path::Path, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    // instantiate client
    let ClientSetup {
        mut client,
        keystore,
    } = setup_client().await?;

    let sync_summary = client.sync_state().await?;
    println!("Latest block: {}", sync_summary.block_num);

    // Build contracts
    let counter_package = Arc::new(
        build_project_in_dir(Path::new("../contracts/counter-account"), true)
            .context("Failed to build counter account contract")?,
    );
    let note_package = Arc::new(
        build_project_in_dir(Path::new("../contracts/increment-note"), true)
            .context("Failed to build increment note contract")?,
    );

    // Create the counter account with initial component storage.
    let counter_storage_slot = counter_storage_slot()?;
    let mut init_storage_data = InitStorageData::default();
    init_storage_data
        .insert_map_entry(counter_storage_slot, COUNTER_STORAGE_KEY, 0_u64)
        .context("Failed to seed counter storage")?;
    let counter_cfg = AccountCreationConfig {
        init_storage_data,
        ..Default::default()
    };

    // create counter account
    let counter_account =
        create_account_from_package(&mut client, counter_package.clone(), counter_cfg)
            .await
            .context("Failed to create counter account")?;

    // Create a separate sender account using only the BasicWallet component
    let sender_cfg = AccountCreationConfig::default();
    let sender_account = create_basic_wallet_account(&mut client, keystore.clone(), sender_cfg)
        .await
        .context("Failed to create sender wallet account")?;
    println!("Sender account ID: {:?}", sender_account.id().to_hex());

    // Build the increment note directly from the compiled package.
    let counter_note = NoteBuilder::new(sender_account.id(), client.rng())
        .package((*note_package).clone())
        .tag(0)
        .build()
        .context("Failed to create counter note from package")?;
    println!("Counter note hash: {:?}", counter_note.id().to_hex());

    // build and submit transaction to publish note
    let note_publish_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![counter_note.clone()])
        .build()
        .context("Failed to build note publish transaction request")?;

    let note_publish_tx_id = client
        .submit_new_transaction(sender_account.id(), note_publish_request)
        .await
        .context("Failed to create note publish transaction")?;

    client
        .sync_state()
        .await
        .context("Failed to sync state after publishing note")?;

    println!(
        "Note publish transaction ID: {:?}",
        note_publish_tx_id.to_hex()
    );

    let consume_note_request = TransactionRequestBuilder::new()
        .input_notes([(counter_note.clone(), None)])
        .build()
        .context("Failed to build consume note transaction request")?;

    let consume_tx_id = client
        .submit_new_transaction(counter_account.id(), consume_note_request)
        .await
        .context("Failed to create consume note transaction")?;

    println!("Consume transaction ID: {:?}", consume_tx_id.to_hex());

    Ok(())
}
