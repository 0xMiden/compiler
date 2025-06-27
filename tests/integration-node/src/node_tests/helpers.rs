//! Common helper functions for node tests

use std::{sync::Arc, time::Duration};

use miden_client::{
    account::{
        component::{BasicWallet, RpoFalcon512},
        Account, AccountBuilder, AccountStorageMode, AccountType,
    },
    auth::AuthSecretKey,
    crypto::SecretKey,
    keystore::FilesystemKeyStore,
    Client, ClientError,
};
use rand::{rngs::StdRng, RngCore};
use tokio::time::sleep;

/// Helper to create a basic account
pub async fn create_basic_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    let anchor_block = client.get_latest_epoch_block().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .anchor((&anchor_block).try_into().unwrap())
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(RpoFalcon512::new(key_pair.public_key()))
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
    account_id: &miden_client::account::Account,
    expected: usize,
) -> Result<(), ClientError> {
    let mut try_num = 0;
    loop {
        client.sync_state().await?;
        let notes = client.get_consumable_notes(None).await?;
        if notes.len() >= expected {
            break;
        }
        eprintln!(
            "{} consumable notes found for account {}. Waiting...",
            notes.len(),
            account_id.id().to_hex()
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
