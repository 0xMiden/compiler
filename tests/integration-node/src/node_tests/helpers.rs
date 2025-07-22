//! Common helper functions for node tests

use std::{sync::Arc, time::Duration};

use miden_client::{
    account::{
        component::{BasicWallet, RpoFalcon512},
        Account, AccountStorageMode, AccountType,
    },
    auth::AuthSecretKey,
    crypto::SecretKey,
    keystore::FilesystemKeyStore,
    Client, ClientError,
};
use miden_objects::account::AccountBuilder;
use rand::{rngs::StdRng, RngCore};
use tokio::time::sleep;

/// Helper to create a basic account
#[allow(dead_code)]
pub async fn create_basic_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicWallet);
    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore.add_key(&AuthSecretKey::RpoFalcon512(key_pair)).unwrap();

    Ok(account)
}

/// Helper to wait until an account has the expected number of consumable notes
#[allow(dead_code)]
pub async fn wait_for_notes(
    client: &mut Client,
    account_id: &miden_client::account::AccountId,
    expected: usize,
) -> Result<(), ClientError> {
    let mut try_num = 0;
    loop {
        client.sync_state().await?;
        let notes = client.get_consumable_notes(None).await?;
        if notes.len() >= expected {
            eprintln!("Found {} consumable notes for account {}", notes.len(), account_id.to_hex());
            break;
        }
        eprintln!(
            "{} consumable notes found for account {}. Waiting...",
            notes.len(),
            account_id.to_hex()
        );
        if try_num > 10 {
            panic!("waiting for too long");
        } else {
            try_num += 1;
        }
        sleep(Duration::from_secs(3)).await;
    }
    Ok(())
}
