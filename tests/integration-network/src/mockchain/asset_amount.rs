//! Mock-chain tests for the typed fungible-asset amount API (`AssetAmount`).
//!
//! Unlike the unit tests in `miden-base-sys`, which decode hand-built asset encodings, these
//! tests execute the on-chain `AssetAmount` API inside a real transaction: the note script
//! decodes amounts from kernel-built assets and checks its arithmetic against the kernel's own
//! vault bookkeeping.

use std::{path::Path, sync::Arc};

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_integration_test_support::{cargo_proj::Project, project};

use super::support::{
    account_cargo_toml_for, account_miden_project_toml_with_interface,
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    note_cargo_toml_for_dependency, note_miden_project_toml_for_dependency, note_script_root,
};

/// Project name of the generated wallet account component.
const WALLET_NAME: &str = "asset-amount-wallet";
/// Miden package name of the generated wallet account component.
const WALLET_PACKAGE: &str = "miden:asset-amount-wallet";

/// Wallet account component consumed by the amount-check note.
///
/// The kernel restricts vault reads to the account context, so the component exposes the typed
/// vault amount as an account procedure; returning `AssetAmount` also exercises the WIT
/// `asset-amount` core type across the component boundary at run time.
const AMOUNT_WALLET_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{Asset, AssetAmount, Word, active_account, component, component_storage};

#[component_storage]
struct AmountWalletStorage;

/// API of the amount-check wallet account component.
#[component]
trait AmountWallet {
    /// Adds an asset to the account vault.
    #[account_procedure]
    fn receive_asset(&mut self, asset: Asset);
    /// Returns the typed amount currently held in the vault under `asset_key`.
    #[account_procedure]
    fn vault_amount(&self, asset_key: Word) -> AssetAmount;
}

#[component]
impl AmountWallet for AmountWalletStorage {
    fn receive_asset(&mut self, asset: Asset) {
        self.add_asset(asset);
    }

    fn vault_amount(&self, asset_key: Word) -> AssetAmount {
        Asset::new(asset_key, active_account::get_asset(asset_key)).amount()
    }
}
"#;

/// On-chain note script exercising the `AssetAmount` API against live kernel state.
///
/// For every note asset it decodes the typed amount from the kernel-built encoding, receives the
/// asset into the wallet, and verifies the vault-amount delta with checked arithmetic,
/// comparisons, and integer conversion. Any violated assertion aborts the transaction.
const ASSET_AMOUNT_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AssetAmount, Word, account, active_note, note};

/// Native account of the note: exposes the amount-wallet component methods.
#[account(asset_amount_wallet::AmountWallet)]
pub struct Wallet;

/// A note that transfers its assets to the consuming account while verifying the typed
/// asset-amount API against the transaction kernel's view of the vault.
#[note]
struct AssetAmountNote;

#[note]
impl AssetAmountNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        let assets = active_note::get_initial_assets();
        for asset in assets {
            // Decode the typed amount from the kernel-built fungible asset encoding.
            let amount = asset.amount();
            assert!(amount > AssetAmount::ZERO);

            let key = asset.key;
            let before = account.vault_amount(key);
            account.receive_asset(asset);
            let after = account.vault_amount(key);

            // The vault amount must grow by exactly the decoded amount (checked addition).
            assert_eq!(after, before + amount);
            // Checked subtraction inverts the addition.
            assert_eq!(after - amount, before);
            assert_eq!(after - before, amount);
            // Amounts order and convert like integers.
            assert!(before < after);
            assert_eq!(after.as_u64(), before.as_u64() + amount.as_u64());
        }
    }
}
"#;

/// Generates and compiles the wallet account component project.
///
/// The returned [`Project`] keeps the generated directory alive: the dependent note project
/// resolves the wallet dependency from that path.
fn build_wallet_project() -> (Project, Arc<Package>) {
    let wallet_project = project(WALLET_NAME)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(
                WALLET_NAME,
                WALLET_PACKAGE,
                "amount-wallet",
            ),
        )
        .file("Cargo.toml", &account_cargo_toml_for(WALLET_NAME, WALLET_PACKAGE))
        .file("src/lib.rs", AMOUNT_WALLET_SOURCE)
        .build();
    let wallet_package = compile_rust_package(wallet_project.root(), true);
    (wallet_project, wallet_package)
}

/// Generates and compiles a note project with the given source, depending on the generated
/// wallet component.
fn compile_note_package(note_name: &str, source: &str, wallet_root: &Path) -> Arc<Package> {
    let note_package_name = format!("miden:{note_name}");
    let note_project = project(note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                note_name,
                &note_package_name,
                WALLET_PACKAGE,
                wallet_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(note_name, WALLET_PACKAGE, wallet_root),
        )
        .file("src/lib.rs", source)
        .build();
    compile_rust_package(note_project.root(), true)
}

/// Tests the on-chain `AssetAmount` API (`Asset::amount`, checked `+`/`-`, ordering, `as_u64`)
/// against kernel-built assets and vault amounts on a mock chain.
///
/// Flow:
/// - The faucet emits two amount-check notes carrying different fungible amounts
/// - The wallet consumes both notes in one transaction, so the note script checks the typed
///   arithmetic once against a zero starting vault amount and once against a non-zero one
/// - The committed vault must hold the sum of both amounts
#[test]
fn asset_amount_api_matches_kernel_balances() {
    // Compile the contracts first (before creating any runtime)
    let (wallet_project, wallet_package) = build_wallet_project();
    let note_package =
        compile_note_package("asset-amount-note", ASSET_AMOUNT_NOTE_SOURCE, wallet_project.root());

    let wallet_component =
        AccountComponent::from_package(&wallet_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "TEST",
            max_supply,
            None,
        )
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting two amount-check notes from the faucet ===");
    let first_amount = 100_000u64;
    let second_amount = 25_000u64;
    let mut note_rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let notes = [first_amount, second_amount].map(|amount| {
        let mint_asset = FungibleAsset::new(faucet_id, amount).unwrap();
        NoteBuilder::new(faucet_id, &mut note_rng)
            .package((*note_package).clone())
            .add_assets([Asset::from(mint_asset)])
            .build()
            .unwrap()
    });

    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_tx_script = build_send_notes_script(&faucet_account, &notes);
    let mint_tx_context_builder = chain
        .build_tx_context(faucet_id, &[], &[])
        .unwrap()
        .tx_script(mint_tx_script.into())
        .extend_expected_output_notes(
            notes.iter().cloned().map(RawOutputNote::Full).collect::<Vec<_>>(),
        );
    execute_tx(&mut chain, mint_tx_context_builder);

    eprintln!("\n=== Step 2: Alice consumes both notes; the scripts assert the amount API ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let consume_tx_context_builder = chain
        .build_tx_context(alice_id, &[notes[0].id(), notes[1].id()], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs]);
    execute_tx(&mut chain, consume_tx_context_builder);

    eprintln!("\n=== Step 3: Checking Alice's committed vault holds the checked sum ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, first_amount + second_amount);
}
